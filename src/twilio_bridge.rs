//! Twilio <-> Vertex AI bridge logic.
//! Handles the protocol translation and audio pipeline.

use crate::audio_util;
use crate::models::*;
use crate::error::IronclawError;
use base64::Engine as _;
use tokio_tungstenite::tungstenite::Message;
use tracing::info;

/// State for a single Twilio-Vertex bridge.
pub struct TwilioBridge {
    pub stream_sid: Option<String>,
    pub call_sid: Option<String>,
    pub agent_id: String,
}

impl TwilioBridge {
    pub fn new(agent_id: String) -> Self {
        Self {
            stream_sid: None,
            call_sid: None,
            agent_id,
        }
    }

    /// Process a message from Twilio and return an optional message for Vertex.
    pub fn handle_twilio_message(&mut self, text: &str) -> Result<Option<Message>, IronclawError> {
        let event: TwilioEvent = serde_json::from_str(text)
            .map_err(|e| IronclawError::WebSocket(format!("Failed to parse Twilio JSON: {}", e)))?;

        match event {
            TwilioEvent::Connected { .. } => {
                info!(agent_id = %self.agent_id, "Twilio stream connected");
                Ok(None)
            }
            TwilioEvent::Start { start, stream_sid, .. } => {
                info!(
                    agent_id = %self.agent_id,
                    stream_sid = %stream_sid,
                    call_sid = %start.call_sid,
                    "Twilio stream started"
                );
                self.stream_sid = Some(stream_sid);
                self.call_sid = Some(start.call_sid);
                Ok(None)
            }
            TwilioEvent::Media { media, .. } => {
                // 1. Decode base64 mu-law
                let raw_mulaw = base64::engine::general_purpose::STANDARD
                    .decode(&media.payload)
                    .map_err(|e| IronclawError::WebSocket(format!("Invalid base64 in Twilio media: {}", e)))?;

                // 2. Convert mu-law -> PCM16 (8kHz)
                let pcm16_8k = audio_util::mulaw_to_pcm16(&raw_mulaw);

                // 3. Upsample 8kHz -> 16kHz
                let pcm16_16k = audio_util::upsample_8_to_16(&pcm16_8k);

                // 4. Wrap in Vertex RealtimeInput
                let b64_pcm = base64::engine::general_purpose::STANDARD.encode(
                    unsafe {
                        std::slice::from_raw_parts(
                            pcm16_16k.as_ptr() as *const u8,
                            pcm16_16k.len() * 2
                        )
                    }
                );

                let vertex_msg = serde_json::json!({
                    "realtimeInput": {
                        "mediaChunks": [{
                            "mimeType": "audio/pcm;rate=16000",
                            "data": b64_pcm
                        }]
                    }
                });

                Ok(Some(Message::Text(vertex_msg.to_string().into())))
            }
            TwilioEvent::Stop { .. } => {
                info!(agent_id = %self.agent_id, "Twilio stream stopped");
                Ok(None)
            }
            TwilioEvent::Mark { .. } => Ok(None),
        }
    }

    /// Process a binary audio chunk from Vertex and return a message for Twilio.
    pub fn handle_vertex_audio(&self, b64_data: &str) -> Result<Option<Message>, IronclawError> {
        let stream_sid = match &self.stream_sid {
            Some(sid) => sid,
            None => return Ok(None),
        };

        // 1. Decode Vertex base64 PCM16 (usually 24kHz or 16kHz)
        let raw_pcm_bytes = base64::engine::general_purpose::STANDARD
            .decode(b64_data)
            .map_err(|e| IronclawError::WebSocket(format!("Invalid base64 from Vertex: {}", e)))?;

        let pcm16: Vec<i16> = raw_pcm_bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        // 2. Downsample (Assuming 16kHz -> 8kHz for now, Vertex Gemini Live is flexible)
        // If Vertex sends 24kHz, we'd need a 3:1 decimation. 16kHz is 2:1.
        let pcm16_8k = audio_util::downsample_16_to_8(&pcm16);

        // 3. Convert PCM16 -> mu-law
        let mulaw = audio_util::pcm16_to_mulaw(&pcm16_8k);

        // 4. Encode as base64
        let b64_mulaw = base64::engine::general_purpose::STANDARD.encode(&mulaw);

        // 5. Wrap in Twilio Media message
        let twilio_msg = serde_json::json!({
            "event": "media",
            "streamSid": stream_sid,
            "media": {
                "payload": b64_mulaw
            }
        });

        Ok(Some(Message::Text(twilio_msg.to_string().into())))
    }
}
