//! Web <-> Vertex AI bridge logic.
//! Simple protocol for direct website integration (Raw PCM16).

use crate::models::*;
use base64::Engine as _;
use tokio_tungstenite::tungstenite::Message;
use tracing::info;
use crate::error::IronclawError;

/// State for a single Web-Vertex bridge.
pub struct WebBridge {
    pub agent_id: String,
}

impl WebBridge {
    pub fn new(agent_id: String) -> Self {
        Self { agent_id }
    }

    /// Process a message from the Web client.
    /// Expected format: JSON { "type": "audio", "data": "base64_pcm16_16k" }
    pub fn handle_web_message(&self, text: &str) -> Result<Option<Message>, IronclawError> {
        let val: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| IronclawError::WebSocket(format!("Invalid Web JSON: {}", e)))?;

        let msg_type = val["type"].as_str().unwrap_or("unknown");
        
        match msg_type {
            "audio" => {
                let data = val["data"].as_str().ok_or_else(|| {
                    IronclawError::WebSocket("Missing data in audio message".to_string())
                })?;

                // Forward raw base64 PCM16 directly to Vertex
                let vertex_msg = serde_json::json!({
                    "realtimeInput": {
                        "mediaChunks": [{
                            "mimeType": "audio/pcm;rate=16000",
                            "data": data
                        }]
                    }
                });

                Ok(Some(Message::Text(vertex_msg.to_string().into())))
            }
            "ping" => Ok(None),
            _ => {
                info!(msg_type = %msg_type, "Received unknown web message type");
                Ok(None)
            }
        }
    }

    /// Process a binary audio chunk from Vertex and format for Web.
    /// Returns JSON { "type": "audio", "data": "base64_pcm16" }
    pub fn handle_vertex_audio(&self, b64_data: &str) -> Result<Option<Message>, IronclawError> {
        let web_msg = serde_json::json!({
            "type": "audio",
            "data": b64_data
        });

        Ok(Some(Message::Text(web_msg.to_string().into())))
    }

    /// Process a JSON message from Vertex (metadata/status) and format for Web.
    /// Returns JSON { "type": "protocol", "data": "original_json" }
    pub fn handle_vertex_json(&self, vertex_json: &str) -> Result<Option<Message>, IronclawError> {
        let web_msg = serde_json::json!({
            "type": "protocol",
            "data": vertex_json
        });

        Ok(Some(Message::Text(web_msg.to_string().into())))
    }
}
