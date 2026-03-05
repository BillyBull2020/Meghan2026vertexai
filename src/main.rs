mod audio_util;
mod error;
mod hot_reload;
mod models;
mod session_manager;
mod twilio_bridge;
mod web_bridge;
mod vertex_client;

use axum::{
    extract::{Query, State, WebSocketUpgrade, ws::WebSocket},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use clap::Parser;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info};
use futures_util::{SinkExt, StreamExt};

/// Ironclaw Voice Agent Factory CLI
#[derive(Parser, Debug)]
#[command(name = "ironclaw", version, about = "Bio DynamX Clonable Voice Agent Factory")]
struct Args {
    #[arg(short, long, default_value = "./profiles")]
    profiles_dir: PathBuf,

    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value = "8080")]
    port: u16,

    #[arg(long, default_value = "false")]
    json_logs: bool,

    #[arg(long, default_value = "300")]
    gc_interval_secs: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let args = Args::parse();

    // ── 1. Initialize Logging ────────────────────────────────
    if args.json_logs {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
            .init();
    }

    info!("╔══════════════════════════════════════════════════════╗");
    info!("║   🦾 IRONCLAW — Bio DynamX Voice Agent Factory     ║");
    info!("║   Memory-safe • Multi-tenant • Zero-downtime       ║");
    info!("╚══════════════════════════════════════════════════════╝");

    // ── 2. Initialize Registry & Session Manager ─────────────
    let registry = hot_reload::new_registry();
    hot_reload::load_all_profiles(&registry, &args.profiles_dir).await?;
    hot_reload::watch_profiles_directory(registry.clone(), args.profiles_dir.clone()).await?;

    let session_manager = Arc::new(session_manager::SessionManager::new(registry.clone()));

    // ── 3. Build Axum Router ─────────────────────────────────
    let app = Router::new()
        .route("/healthz", get(health_handler))
        .route("/status", get(status_handler))
        .route("/twiml", get(twiml_handler).post(twiml_handler))
        .route("/media-stream", get(twilio_ws_handler))
        .route("/web-session", get(web_ws_handler))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(session_manager.clone());

    // ── 4. Start Server ──────────────────────────────────────
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    
    info!(address = %addr, "🚀 Ironclaw server online — ready for voice sessions");

    axum::serve(listener, app).await?;

    Ok(())
}

// ── HTTP HANDLERS ────────────────────────────────────────────

async fn health_handler(State(sm): State<Arc<session_manager::SessionManager>>) -> impl IntoResponse {
    let health = models::HealthResponse {
        status: "healthy".to_string(),
        active_profiles: sm.profile_count().await,
        active_sessions: sm.active_count().await,
        uptime_seconds: 0, // Simplified for now
    };
    axum::Json(health)
}

async fn status_handler(State(sm): State<Arc<session_manager::SessionManager>>) -> impl IntoResponse {
    let sessions = sm.list_sessions().await;
    let profiles = {
        let reg = sm.agent_registry.read().await;
        reg.keys().cloned().collect::<Vec<String>>()
    };
    
    axum::Json(serde_json::json!({
        "engine": "ironclaw",
        "profiles_loaded": profiles.len(),
        "available_profiles": profiles,
        "active_sessions": sessions.len(),
        "sessions": sessions,
    }))
}

/// TwiML endpoint for Twilio WebHooks.
/// Returns the `<Stream>` TwiML to connect a call to Ironclaw.
async fn twiml_handler(
    Query(params): Query<HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let agent_id = params.get("agent_id").cloned().unwrap_or("aria_receptionist_01".to_string());
    
    // Attempt to detect the public host (e.g. ironclaw-factory-xyz.a.run.app)
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:8080");

    let protocol = if host.contains("localhost") { "ws" } else { "wss" };

    let twiml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Response>
    <Say>Connecting to {} via Ironclaw.</Say>
    <Connect>
        <Stream url="{}://{}/media-stream?agent_id={}" />
    </Connect>
</Response>"#,
        agent_id, protocol, host, agent_id
    );

    Response::builder()
        .header("Content-Type", "text/xml")
        .body(twiml)
        .unwrap()
}

/// WebSocket handler for Twilio Media Streams.
async fn twilio_ws_handler(
    ws: WebSocketUpgrade,
    State(sm): State<Arc<session_manager::SessionManager>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let agent_id = params.get("agent_id").cloned().unwrap_or("aria_receptionist_01".to_string());
    ws.on_upgrade(move |socket| handle_twilio_socket(socket, sm, agent_id))
}

async fn handle_twilio_socket(
    socket: WebSocket,
    sm: Arc<session_manager::SessionManager>,
    agent_id: String,
) {
    let (mut twilio_tx, mut twilio_rx) = socket.split();

    // 1. Spawn the Vertex session
    let (session_id, mut vertex_rx) = match sm.spawn_session(&agent_id, None).await {
        Ok(res) => res,
        Err(e) => {
            error!("Failed to spawn Vertex session: {}", e);
            return;
        }
    };

    let mut bridge = twilio_bridge::TwilioBridge::new(agent_id.clone());

    // 2. Loop: Forward messages between Twilio and Vertex
    loop {
        tokio::select! {
            // Incoming from Twilio -> Forward to Vertex
            Some(msg) = twilio_rx.next() => {
                match msg {
                    Ok(axum::extract::ws::Message::Text(text)) => {
                        match bridge.handle_twilio_message(&text) {
                            Ok(Some(vertex_msg)) => {
                                // bridge.handle_twilio_message returns tokio_tungstenite::tungstenite::Message
                                if let Err(e) = sm.send_to_session(session_id, vertex_msg).await {
                                    error!("Failed to send to Vertex: {}", e);
                                    break;
                                }
                            }
                            Ok(None) => {}, // Metadata or Start/Stop
                            Err(e) => {
                                error!("Twilio bridge error: {}", e);
                                break;
                            }
                        }
                    }
                    Ok(axum::extract::ws::Message::Close(_)) => {
                        info!("Twilio closed WebSocket connection");
                        break;
                    }
                    Err(e) => {
                        error!("Twilio WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Incoming from Vertex -> Forward to Twilio
            Some(vertex_msg) = vertex_rx.recv() => {
                // Try to parse as JSON to extract audio from Vertex response
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&vertex_msg) {
                    if let Some(parts) = parsed
                        .get("serverContent")
                        .and_then(|sc| sc.get("modelTurn"))
                        .and_then(|mt| mt.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        for part in parts {
                            if let Some(b64_audio) = part
                                .get("inlineData")
                                .and_then(|id| id.get("data"))
                                .and_then(|d| d.as_str())
                            {
                                match bridge.handle_vertex_audio(b64_audio) {
                                    Ok(Some(twilio_msg)) => {
                                        if let tokio_tungstenite::tungstenite::Message::Text(text) = twilio_msg {
                                            if let Err(e) = twilio_tx.send(axum::extract::ws::Message::Text(text.as_str().into())).await {
                                                error!("Failed to send audio to Twilio: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                } else if !vertex_msg.starts_with('{') {
                    // Fallback for raw base64 (if any)
                    match bridge.handle_vertex_audio(&vertex_msg) {
                        Ok(Some(twilio_msg)) => {
                            if let tokio_tungstenite::tungstenite::Message::Text(text) = twilio_msg {
                                let _ = twilio_tx.send(axum::extract::ws::Message::Text(text.as_str().into())).await;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // 3. Cleanup
    info!(session_id = %session_id, "Closing Twilio-Vertex session");
    let _ = sm.close_session(session_id).await;
}

/// WebSocket handler for Web clients.
async fn web_ws_handler(
    ws: WebSocketUpgrade,
    State(sm): State<Arc<session_manager::SessionManager>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let agent_id = params.get("agent_id").cloned().unwrap_or("aria_receptionist_01".to_string());
    ws.on_upgrade(move |socket| handle_web_socket(socket, sm, agent_id))
}

async fn handle_web_socket(
    socket: WebSocket,
    sm: Arc<session_manager::SessionManager>,
    agent_id: String,
) {
    info!(agent_id = %agent_id, "New web-session connection request");
    let (mut web_tx, mut web_rx) = socket.split();

    // 1. Spawn the Vertex session
    let (session_id, mut vertex_rx) = match sm.spawn_session(&agent_id, None).await {
        Ok(res) => {
            info!(session_id = %res.0, "Vertex session spawned successfully for web client");
            res
        },
        Err(e) => {
            error!("Failed to spawn Vertex session for web: {}", e);
            return;
        }
    };

    let bridge = web_bridge::WebBridge::new(agent_id.clone());

    // 2. Loop: Forward messages between Web and Vertex
    loop {
        tokio::select! {
            // Incoming from Web -> Forward to Vertex
            Some(msg) = web_rx.next() => {
                match msg {
                    Ok(axum::extract::ws::Message::Text(text)) => {
                        match bridge.handle_web_message(&text) {
                            Ok(Some(vertex_msg)) => {
                                if let Err(e) = sm.send_to_session(session_id, vertex_msg).await {
                                    error!("Failed to send to Vertex: {}", e);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(axum::extract::ws::Message::Close(_)) => break,
                    _ => {}
                }
            }

            // Incoming from Vertex -> Forward to Web
            Some(vertex_msg) = vertex_rx.recv() => {
                info!("Received message from Vertex ({} bytes)", vertex_msg.len());
                // Try to parse as JSON to extract audio data from Vertex's response
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&vertex_msg) {
                    // Check for audio in serverContent.modelTurn.parts[].inlineData.data
                    if let Some(parts) = parsed
                        .get("serverContent")
                        .and_then(|sc| sc.get("modelTurn"))
                        .and_then(|mt| mt.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        info!("Found {} parts with audio data", parts.len());
                        for part in parts {
                            if let Some(b64_audio) = part
                                .get("inlineData")
                                .and_then(|id| id.get("data"))
                                .and_then(|d| d.as_str())
                            {
                                info!("Forwarding audio chunk ({} base64 chars)", b64_audio.len());
                                // Build the web-client message directly as JSON string
                                let audio_msg = serde_json::json!({
                                    "type": "audio",
                                    "data": b64_audio
                                });
                                if let Err(e) = web_tx.send(axum::extract::ws::Message::Text(audio_msg.to_string().into())).await {
                                    error!("Failed to send audio to web: {}", e);
                                    break;
                                }
                            }
                        }
                    } else {
                        // Non-audio JSON (setupComplete, toolCall, etc.) → forward as protocol
                        info!("Protocol message from Vertex: {}", &vertex_msg[..vertex_msg.len().min(200)]);
                        let web_msg = serde_json::json!({
                            "type": "protocol",
                            "data": vertex_msg
                        });
                        if let Err(e) = web_tx.send(axum::extract::ws::Message::Text(web_msg.to_string().into())).await {
                            error!("Failed to send protocol to web: {}", e);
                            break;
                        }
                    }
                } else {
                    // Non-JSON (raw base64 binary that was encoded upstream)
                    info!("Non-JSON message from Vertex ({} bytes), forwarding as audio", vertex_msg.len());
                    let audio_msg = serde_json::json!({
                        "type": "audio",
                        "data": vertex_msg
                    });
                    if let Err(e) = web_tx.send(axum::extract::ws::Message::Text(audio_msg.to_string().into())).await {
                        error!("Failed to send audio to web: {}", e);
                        break;
                    }
                }
            }
        }
    }

    let _ = sm.close_session(session_id).await;
}
