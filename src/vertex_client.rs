//! Vertex AI Gemini Live WebSocket client.
//!
//! Establishes bidirectional native audio streaming using
//! Google Cloud Application Default Credentials (ADC).
//! Implements the Bio DynamX reliability patterns:
//! - Consolidated setup messages (no rapid turnComplete:false)
//! - Silent PCM16 keep-alive heartbeats
//! - Immediate tool responses (never batched)

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
};
use tracing::{debug, error, info, warn};

use crate::error::IronclawError;
use crate::models::*;

/// Handle to a live Vertex AI voice session.
pub struct VertexVoiceStream {
    /// Sender half — push audio/text messages to Gemini.
    pub tx: mpsc::Sender<Message>,

    /// Join handle for the read loop task.
    pub read_handle: tokio::task::JoinHandle<()>,

    /// Join handle for the write loop task.
    pub write_handle: tokio::task::JoinHandle<()>,
}

/// Spawn a new Vertex AI voice agent session.
///
/// 1. Authenticates via GCP ADC
/// 2. Opens a WebSocket to the Gemini Live endpoint
/// 3. Sends the consolidated setup message
/// 4. Returns sender/receiver handles for bidirectional streaming
pub async fn spawn_vertex_voice_agent(
    profile: &AgentProfile,
    on_server_message: mpsc::Sender<String>,
) -> Result<VertexVoiceStream, IronclawError> {
    // ── 1. Authenticate via ADC ──────────────────────────────
    let token = get_access_token().await?;

    // ── 1. Establish WebSocket Connection ─────────────────────
    let endpoint = format!(
        "wss://{}-aiplatform.googleapis.com/ws/google.cloud.aiplatform.v1beta1.LlmBidiService/BidiGenerateContent",
        profile.vertex_ai_config.location
    );

    info!(
        agent_id = %profile.agent_id,
        endpoint = %endpoint,
        "Connecting to Vertex AI Gemini Live..."
    );

    // ── 3. Connect ───────────────────────────────────────────
    let url = format!("{}?access_token={}", endpoint, token);
    let (ws_stream, response) = connect_async(&url)
        .await
        .map_err(|e| IronclawError::WebSocket(format!("Connection failed: {}", e)))?;

    info!(
        status = ?response.status(),
        "WebSocket connected to Vertex AI"
    );

    // ── 4. Split into read/write halves ──────────────────────
    let (mut write, mut read) = ws_stream.split();

    // ── 5. Send consolidated setup message ───────────────────
    //    (Reliability Pattern #2: single message, turnComplete:true equivalent)
    let setup = build_setup_message(profile);
    let setup_json = serde_json::to_string(&setup)?;

    info!(agent_id = %profile.agent_id, setup_payload = %setup_json, "Sending setup message");

    write
        .send(Message::Text(setup_json.into()))
        .await
        .map_err(|e| IronclawError::WebSocket(format!("Setup send failed: {}", e)))?;

    info!(agent_id = %profile.agent_id, "Setup message sent, awaiting handshake...");

    // ── 5. Wait for Handshake (setupComplete) ────────────────
    match read.next().await {
        Some(Ok(Message::Text(text))) => {
            info!(agent_id = %profile.agent_id, "Vertex Initial Handshake: {}", &text[..text.len().min(200)]);
            
            // Trigger a formal greeting with a text prompt.
            // This ensures Gemini has something to respond to immediately.
            let trigger = serde_json::json!({
                "clientContent": {
                    "turns": [{
                        "role": "user",
                        "parts": [{ "text": "Hello! Please introduce yourself and wait for my request." }]
                    }],
                    "turnComplete": true
                }
            });
            write.send(Message::Text(trigger.to_string().into())).await.ok();
            info!("Sent formal greeting trigger to Gemini");
        }
        Some(res) => {
            error!("Unexpected handshake result: {:?}", res);
            return Err(IronclawError::WebSocket("Handshake failed".to_string()));
        }
        None => {
            error!("Vertex closed connection during handshake");
            return Err(IronclawError::WebSocket("Connection closed".to_string()));
        }
    }

    // ── 6. Create the message channel ────────────────────────
    let (tx, mut rx) = mpsc::channel::<Message>(256);

    // ── 7. Spawn write loop ──────────────────────────────────
    let write_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = write.send(msg).await {
                error!("Write loop error: {}", e);
                break;
            }
        }
        debug!("Write loop terminated");
    });

    // ── 8. Spawn read loop ───────────────────────────────────
    let agent_id = profile.agent_id.clone();
    let read_handle = tokio::spawn(async move {
        while let Some(result) = read.next().await {
            match result {
                Ok(Message::Text(text)) => {
                    info!(agent_id = %agent_id, "Vertex Protocol Message: {}", &text[..text.len().min(200)]);
                    if let Err(e) = on_server_message.send(text.to_string()).await {
                        error!("Failed to forward server message: {}", e);
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
                    info!(agent_id = %agent_id, "Binary frame from Vertex ({} bytes)", data.len());
                    // Forward raw binary bytes encoded as base64. 
                    // main.rs will see this doesn't start with '{' and wrap it for the web client.
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                    if let Err(e) = on_server_message.send(b64).await {
                        error!("Failed to forward binary chunk: {}", e);
                        break;
                    }
                }
                Ok(Message::Close(frame)) => {
                    warn!(
                        agent_id = %agent_id,
                        frame = ?frame,
                        "WebSocket closed by server"
                    );
                    break;
                }
                Ok(Message::Ping(data)) => {
                    debug!("Ping received, pong auto-sent by tungstenite");
                    let _ = data; // tungstenite handles pong automatically
                }
                Ok(_) => {} // Pong, Frame — ignored
                Err(e) => {
                    error!(agent_id = %agent_id, "Read error: {}", e);
                    break;
                }
            }
        }
        debug!(agent_id = %agent_id, "Read loop terminated");
    });

    Ok(VertexVoiceStream {
        tx,
        read_handle,
        write_handle,
    })
}

/// Build a 16kHz silent PCM16 chunk to act as a keep-alive/heartbeat.
/// Gemini Live expects 16kHz input. 40ms of silence = 320 bytes.
pub fn build_silence_keepalive() -> Message {
    let silence = vec![0u8; 320];
    let b64_silence = base64::engine::general_purpose::STANDARD.encode(&silence);

    let payload = serde_json::json!({
        "realtimeInput": {
            "mediaChunks": [{
                "mimeType": "audio/pcm;rate=16000",
                "data": b64_silence
            }]
        }
    });

    Message::Text(payload.to_string().into())
}

/// Build the setup message from an agent profile.
fn build_setup_message(profile: &AgentProfile) -> SetupMessage {
    let project_id = std::env::var("GOOGLE_CLOUD_PROJECT")
        .or_else(|_| std::env::var("PROJECT_ID"))
        .unwrap_or_else(|_| "bio-dynamx".to_string());

    SetupMessage {
        setup: SetupPayload {
            model: format!(
                "projects/{}/locations/{}/publishers/google/models/{}",
                project_id,
                profile.vertex_ai_config.location,
                profile.vertex_ai_config.model
            ),
            generation_config: GenerationConfig {
                response_modalities: vec!["AUDIO".to_string()],
                speech_config: SpeechConfig {
                    voice_config: VoiceConfig {
                        prebuilt_voice_config: PrebuiltVoiceConfig {
                            voice_name: profile.vertex_ai_config.voice.clone(),
                        },
                    },
                },
            },
            system_instruction: SystemInstruction {
                parts: vec![TextPart {
                    text: profile.neuro_system_prompt.clone(),
                }],
            },
            realtime_input_config: Some(RealtimeInputConfig {
                automatic_activity_detection: AutomaticActivityDetection {
                    disabled: true, // Disable VAD to prevent feedback/machine scream
                },
            }),
            runtime_config: None,
        },
    }
}


/// Inject text context into a live session (e.g., for persona handoff).
///
/// Reliability Pattern #2: Consolidated injection — single message
/// with turnComplete:true to avoid model freeze.
pub fn build_context_injection(context_blocks: &[&str]) -> Message {
    let combined = context_blocks.join("\n\n---\n\n");

    let payload = serde_json::json!({
        "clientContent": {
            "turns": [{
                "role": "user",
                "parts": [{ "text": combined }]
            }],
            "turnComplete": true
        }
    });

    Message::Text(payload.to_string().into())
}

/// Get an access token from GCP Application Default Credentials.
async fn get_access_token() -> Result<String, IronclawError> {
    // 1. Try standard ADC
    if let Ok(provider) = gcp_auth::provider().await {
        let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
        if let Ok(token) = provider.token(scopes).await {
            return Ok(token.as_str().to_string());
        }
    }

    // 2. Fallback to gcloud CLI (for local development stability)
    let output = std::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .map_err(|e| IronclawError::Auth(format!("Gcloud CLI failed: {}", e)))?;

    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    Err(IronclawError::Auth("All auth methods failed. Run 'gcloud auth application-default login'".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_setup_message() {
        let profile = AgentProfile {
            agent_id: "test_agent".to_string(),
            vertex_ai_config: VertexAiConfig {
                model: "gemini-2.5-flash-native-audio-preview".to_string(),
                voice: "Kore".to_string(),
                location: "us-central1".to_string(),
            },
            neuro_system_prompt: "You are a test agent.".to_string(),
            capabilities: vec!["schedule_appointment".to_string()],
            display_name: None,
            tags: vec![],
        };

        let msg = build_setup_message(&profile);
        let json = serde_json::to_string_pretty(&msg).unwrap();

        assert!(json.contains("gemini-2.5-flash-native-audio-preview"));
        assert!(json.contains("Kore"));
        assert!(json.contains("You are a test agent."));
        assert!(json.contains("AUDIO"));
    }

    #[test]
    fn test_silence_keepalive_is_valid_json() {
        let msg = build_silence_keepalive();
        if let Message::Text(text) = msg {
            let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert!(parsed["realtimeInput"]["mediaChunks"][0]["data"]
                .as_str()
                .is_some());
        } else {
            panic!("Expected text message");
        }
    }

    #[test]
    fn test_context_injection_consolidates() {
        let blocks = vec!["Block A", "Block B", "Block C"];
        let msg = build_context_injection(&blocks);
        if let Message::Text(text) = msg {
            let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
            let injected = parsed["clientContent"]["turns"][0]["parts"][0]["text"]
                .as_str()
                .unwrap();
            assert!(injected.contains("Block A"));
            assert!(injected.contains("---"));
            assert!(injected.contains("Block C"));
            assert!(parsed["clientContent"]["turnComplete"].as_bool().unwrap());
        } else {
            panic!("Expected text message");
        }
    }
}
