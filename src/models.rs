//! Data models for the Ironclaw Voice Agent Factory.
//!
//! These models define the YAML-driven agent profile schema,
//! session state machine, and Vertex AI protocol messages.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────
// AGENT PROFILE (loaded from YAML files in profiles/)
// ─────────────────────────────────────────────────────────────

/// Configuration for the Vertex AI connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VertexAiConfig {
    /// The Gemini model identifier (e.g. "gemini-2.5-flash-native-audio-preview").
    pub model: String,

    /// Voice persona name (e.g. "Kore", "Puck", "Charon").
    pub voice: String,

    /// GCP region (e.g. "us-central1").
    pub location: String,
}

/// A complete agent profile loaded from a YAML template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// Unique identifier for this agent configuration.
    pub agent_id: String,

    /// Vertex AI connection settings.
    pub vertex_ai_config: VertexAiConfig,

    /// The neuro-optimized system prompt (Triune Brain hierarchy).
    pub neuro_system_prompt: String,

    /// Capabilities this agent can invoke (tool names).
    #[serde(default)]
    pub capabilities: Vec<String>,

    /// Optional display name for logging/UI.
    #[serde(default)]
    pub display_name: Option<String>,

    /// Optional metadata tags for routing.
    #[serde(default)]
    pub tags: Vec<String>,
}

// ─────────────────────────────────────────────────────────────
// SESSION STATE MACHINE
// ─────────────────────────────────────────────────────────────

/// Lifecycle states for a voice session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Session object created, not yet connected.
    Spawning,
    /// WebSocket TCP handshake complete.
    Connected,
    /// Setup message sent, awaiting setupComplete.
    SetupSent,
    /// Microphone/audio pipeline active, agent is live.
    Live,
    /// Graceful shutdown in progress.
    Draining,
    /// Session fully terminated.
    Closed,
    /// An error occurred.
    Error,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawning => write!(f, "spawning"),
            Self::Connected => write!(f, "connected"),
            Self::SetupSent => write!(f, "setup_sent"),
            Self::Live => write!(f, "live"),
            Self::Draining => write!(f, "draining"),
            Self::Closed => write!(f, "closed"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// A live voice session between a caller and a Vertex AI agent.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceSession {
    /// Unique session identifier.
    pub session_id: Uuid,

    /// The agent profile driving this session.
    pub agent_id: String,

    /// Current lifecycle status.
    pub status: SessionStatus,

    /// When the session was created.
    pub created_at: DateTime<Utc>,

    /// When the session last changed status.
    pub updated_at: DateTime<Utc>,

    /// Optional caller identifier for multi-tenant routing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_id: Option<String>,

    /// Accumulated turn count for diagnostics.
    pub turn_count: u32,
}

impl VoiceSession {
    /// Create a new session in the Spawning state.
    pub fn new(agent_id: &str, caller_id: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            session_id: Uuid::new_v4(),
            agent_id: agent_id.to_string(),
            status: SessionStatus::Spawning,
            created_at: now,
            updated_at: now,
            caller_id,
            turn_count: 0,
        }
    }

    /// Transition to a new status, updating the timestamp.
    pub fn transition(&mut self, new_status: SessionStatus) {
        tracing::info!(
            session_id = %self.session_id,
            from = %self.status,
            to = %new_status,
            "Session state transition"
        );
        self.status = new_status;
        self.updated_at = Utc::now();
    }
}

// ─────────────────────────────────────────────────────────────
// VERTEX AI WEBSOCKET PROTOCOL MESSAGES
// ─────────────────────────────────────────────────────────────

/// The initial setup message sent to Gemini Live after WebSocket open.
#[derive(Debug, Serialize)]
pub struct SetupMessage {
    pub setup: SetupPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupPayload {
    pub model: String,
    pub generation_config: GenerationConfig,
    pub system_instruction: SystemInstruction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realtime_input_config: Option<RealtimeInputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_config: Option<RuntimeConfig>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfig {
    pub audio_configuration: AudioConfiguration,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfiguration {
    pub start_sensitivity: String,
    pub end_sensitivity: String,
}


#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeInputConfig {
    pub automatic_activity_detection: AutomaticActivityDetection,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticActivityDetection {
    pub disabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    pub response_modalities: Vec<String>,
    pub speech_config: SpeechConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechConfig {
    pub voice_config: VoiceConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfig {
    pub prebuilt_voice_config: PrebuiltVoiceConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrebuiltVoiceConfig {
    pub voice_name: String,
}

#[derive(Debug, Serialize)]
pub struct SystemInstruction {
    pub parts: Vec<TextPart>,
}

#[derive(Debug, Serialize)]
pub struct TextPart {
    pub text: String,
}

/// Audio input message for streaming microphone data to Gemini.
#[derive(Debug, Serialize)]
pub struct RealtimeInput {
    #[serde(rename = "realtimeInput")]
    pub realtime_input: MediaChunksWrapper,
}

#[derive(Debug, Serialize)]
pub struct MediaChunksWrapper {
    #[serde(rename = "mediaChunks")]
    pub media_chunks: Vec<MediaChunk>,
}

#[derive(Debug, Serialize)]
pub struct MediaChunk {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String, // base64-encoded PCM16
}

/// Client content message for injecting text context mid-session.
#[derive(Debug, Serialize)]
pub struct ClientContent {
    #[serde(rename = "clientContent")]
    pub client_content: ClientContentPayload,
}

#[derive(Debug, Serialize)]
pub struct ClientContentPayload {
    pub turns: Vec<Turn>,
    #[serde(rename = "turnComplete")]
    pub turn_complete: bool,
}

#[derive(Debug, Serialize)]
pub struct Turn {
    pub role: String,
    pub parts: Vec<TextPart>,
}

// ─────────────────────────────────────────────────────────────
// TWILIO MEDIA STREAM MODELS
// ─────────────────────────────────────────────────────────────

/// Encompasses all possible events sent from Twilio during a Media Stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum TwilioEvent {
    Connected {
        protocol: String,
        version: String,
    },
    Start {
        sequence_number: String,
        start: TwilioStart,
        stream_sid: String,
    },
    Media {
        sequence_number: String,
        media: TwilioMedia,
        stream_sid: String,
    },
    Stop {
        sequence_number: String,
        stop: TwilioStop,
        stream_sid: String,
    },
    Mark {
        sequence_number: String,
        mark: TwilioMark,
        stream_sid: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwilioStart {
    pub account_sid: String,
    pub call_sid: String,
    pub stream_sid: String,
    pub tracks: Vec<String>,
    #[serde(default)]
    pub custom_parameters: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwilioMedia {
    pub track: String,
    pub chunk: String,
    pub timestamp: String,
    pub payload: String, // Base64 encoded mu-law audio
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwilioStop {
    pub account_sid: String,
    pub call_sid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwilioMark {
    pub name: String,
}

/// Messages sent back to Twilio from Ironclaw.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum IronclawToTwilio {
    Media {
        media: TwilioMediaOut,
        stream_sid: String,
    },
    Clear {
        stream_sid: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwilioMediaOut {
    pub payload: String, // Base64 encoded mu-law audio
}

// ─────────────────────────────────────────────────────────────
// HEALTHCHECK RESPONSE
// ─────────────────────────────────────────────────────────────

/// Response for the /healthz endpoint.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub active_profiles: usize,
    pub active_sessions: usize,
    pub uptime_seconds: u64,
}
