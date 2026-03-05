//! Concurrent session manager for the Ironclaw voice agent factory.
//!
//! Maps incoming calls to the correct YAML profile dynamically,
//! manages session lifecycle, and provides concurrency-safe
//! operations on the active session pool.
//!
//! Designed to handle hundreds of simultaneous voice sessions
//! using Tokio's async runtime with Arc<RwLock<>> shared state.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info};
use uuid::Uuid;

use crate::error::IronclawError;
use crate::hot_reload::AgentRegistry;
use crate::models::{SessionStatus, VoiceSession};
use crate::vertex_client::{self, VertexVoiceStream};

/// Thread-safe session pool: session_id → VoiceSession.
pub type SessionPool = Arc<RwLock<HashMap<Uuid, VoiceSession>>>;

/// Handles to active WebSocket streams, keyed by session_id.
type StreamPool = Arc<RwLock<HashMap<Uuid, VertexVoiceStream>>>;

/// The central session manager that coordinates voice sessions.
pub struct SessionManager {
    /// Registry of available agent profiles (shared with hot-reload).
    pub agent_registry: AgentRegistry,

    /// Pool of active voice sessions.
    sessions: SessionPool,

    /// Pool of active WebSocket stream handles.
    streams: StreamPool,
}

impl SessionManager {
    /// Create a new session manager with the given agent registry.
    pub fn new(agent_registry: AgentRegistry) -> Self {
        Self {
            agent_registry,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawn a new voice session for the given agent_id.
    ///
    /// 1. Looks up the agent profile in the registry
    /// 2. Creates a VoiceSession in Spawning state
    /// 3. Connects to Vertex AI via WebSocket
    /// 4. Transitions to Live state
    /// 5. Returns the session ID and a receiver for server messages
    pub async fn spawn_session(
        &self,
        agent_id: &str,
        caller_id: Option<String>,
    ) -> Result<(Uuid, mpsc::Receiver<String>), IronclawError> {
        // ── 1. Resolve the agent profile ─────────────────────
        let profile = {
            let reg = self.agent_registry.read().await;
            reg.get(agent_id)
                .cloned()
                .ok_or_else(|| IronclawError::AgentNotFound(agent_id.to_string()))?
        };

        // ── 2. Create session record ─────────────────────────
        let mut session = VoiceSession::new(agent_id, caller_id);
        let session_id = session.session_id;

        info!(
            session_id = %session_id,
            agent_id = %agent_id,
            "Spawning new voice session"
        );

        // ── 3. Connect to Vertex AI ──────────────────────────
        session.transition(SessionStatus::Connected);

        let (server_tx, server_rx) = mpsc::channel(256);

        let stream = vertex_client::spawn_vertex_voice_agent(&profile, server_tx).await?;

        session.transition(SessionStatus::SetupSent);

        // ── 4. Store session and stream ──────────────────────
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id, session);
        }
        {
            let mut streams = self.streams.write().await;
            streams.insert(session_id, stream);
        }

        // ── 5. Transition to Live ────────────────────────────
        self.update_status(session_id, SessionStatus::Live).await?;

        info!(
            session_id = %session_id,
            agent_id = %agent_id,
            "🎙️ Voice session LIVE"
        );

        Ok((session_id, server_rx))
    }

    /// Send a message to an active session's Vertex AI stream.
    pub async fn send_to_session(
        &self,
        session_id: Uuid,
        message: tokio_tungstenite::tungstenite::Message,
    ) -> Result<(), IronclawError> {
        let streams = self.streams.read().await;
        let stream = streams
            .get(&session_id)
            .ok_or_else(|| IronclawError::SessionNotFound(session_id.to_string()))?;

        stream
            .tx
            .send(message)
            .await
            .map_err(|e| IronclawError::WebSocket(format!("Failed to send: {}", e)))?;

        Ok(())
    }

    /// Start a silent keep-alive loop for a session during long tool calls.
    ///
    /// Returns a handle that can be aborted when the tool call completes.
    pub async fn start_keepalive(
        &self,
        session_id: Uuid,
    ) -> Result<tokio::task::JoinHandle<()>, IronclawError> {
        let streams = self.streams.read().await;
        let stream = streams
            .get(&session_id)
            .ok_or_else(|| IronclawError::SessionNotFound(session_id.to_string()))?;

        let tx = stream.tx.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let silence = vertex_client::build_silence_keepalive();
                if tx.send(silence).await.is_err() {
                    debug!(
                        session_id = %session_id,
                        "Keep-alive loop ended (channel closed)"
                    );
                    break;
                }
                debug!(session_id = %session_id, "💓 Silent keep-alive sent");
            }
        });

        Ok(handle)
    }

    /// Gracefully close a voice session.
    pub async fn close_session(&self, session_id: Uuid) -> Result<(), IronclawError> {
        // Transition to Draining
        self.update_status(session_id, SessionStatus::Draining)
            .await?;

        // Remove and drop the stream (closes WebSocket)
        {
            let mut streams = self.streams.write().await;
            if let Some(stream) = streams.remove(&session_id) {
                // Abort the read/write tasks
                stream.read_handle.abort();
                stream.write_handle.abort();
                debug!(session_id = %session_id, "Stream handles aborted");
            }
        }

        // Transition to Closed
        self.update_status(session_id, SessionStatus::Closed)
            .await?;

        info!(session_id = %session_id, "🔒 Voice session CLOSED");
        Ok(())
    }

    /// Get a snapshot of a session's current state.
    pub async fn get_session(&self, session_id: Uuid) -> Option<VoiceSession> {
        let sessions = self.sessions.read().await;
        sessions.get(&session_id).cloned()
    }

    /// Get a snapshot of all active sessions.
    pub async fn list_sessions(&self) -> Vec<VoiceSession> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Count active (non-Closed) sessions.
    pub async fn active_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .filter(|s| s.status != SessionStatus::Closed && s.status != SessionStatus::Error)
            .count()
    }

    /// Get the number of registered agent profiles.
    pub async fn profile_count(&self) -> usize {
        let reg = self.agent_registry.read().await;
        reg.len()
    }

    /// Update a session's status.
    async fn update_status(
        &self,
        session_id: Uuid,
        new_status: SessionStatus,
    ) -> Result<(), IronclawError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| IronclawError::SessionNotFound(session_id.to_string()))?;

        session.transition(new_status);
        Ok(())
    }

    /// Perform garbage collection — remove Closed sessions older than `max_age`.
    pub async fn gc(&self, max_age: chrono::Duration) {
        let cutoff = chrono::Utc::now() - max_age;
        let mut sessions = self.sessions.write().await;
        let before = sessions.len();

        sessions.retain(|_id, session| {
            !(session.status == SessionStatus::Closed && session.updated_at < cutoff)
        });

        let removed = before - sessions.len();
        if removed > 0 {
            info!(removed, "🧹 Garbage collected stale sessions");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hot_reload::new_registry;
    use crate::models::{AgentProfile, VertexAiConfig};

    fn test_profile() -> AgentProfile {
        AgentProfile {
            agent_id: "test_session_agent".to_string(),
            vertex_ai_config: VertexAiConfig {
                model: "gemini-2.5-flash-native-audio-preview".to_string(),
                voice: "Kore".to_string(),
                location: "us-central1".to_string(),
            },
            neuro_system_prompt: "Test prompt.".to_string(),
            capabilities: vec![],
            display_name: None,
            tags: vec![],
        }
    }

    #[tokio::test]
    async fn test_session_manager_creation() {
        let registry = new_registry();
        {
            let mut reg = registry.write().await;
            let profile = test_profile();
            reg.insert(profile.agent_id.clone(), profile);
        }

        let manager = SessionManager::new(registry);
        assert_eq!(manager.profile_count().await, 1);
        assert_eq!(manager.active_count().await, 0);
    }

    #[tokio::test]
    async fn test_agent_not_found() {
        let registry = new_registry();
        let manager = SessionManager::new(registry);

        let result = manager.spawn_session("nonexistent", None).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            IronclawError::AgentNotFound(id) => assert_eq!(id, "nonexistent"),
            other => panic!("Expected AgentNotFound, got: {:?}", other),
        }
    }
}
