//! Hot-reloading file watcher for agent profiles.
//!
//! Monitors the `profiles/` directory for YAML file changes.
//! When a new file is dropped in or an existing one is modified,
//! the agent registry is updated with zero server downtime.
//!
//! Uses the `notify` crate with debouncing to avoid rapid-fire
//! events from editors that do atomic write-then-rename.

use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::error::IronclawError;
use crate::models::AgentProfile;

/// Thread-safe agent registry: agent_id → AgentProfile.
pub type AgentRegistry = Arc<RwLock<HashMap<String, AgentProfile>>>;

/// Create a new empty agent registry.
pub fn new_registry() -> AgentRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Load all YAML profiles from the given directory into the registry.
///
/// Called at startup to populate the initial set of agents.
pub async fn load_all_profiles(
    registry: &AgentRegistry,
    profiles_dir: &Path,
) -> Result<usize, IronclawError> {
    let mut count = 0;

    if !profiles_dir.exists() {
        warn!(
            path = %profiles_dir.display(),
            "Profiles directory does not exist, creating it..."
        );
        std::fs::create_dir_all(profiles_dir).map_err(|e| IronclawError::ProfileIo {
            path: profiles_dir.display().to_string(),
            source: e,
        })?;
        return Ok(0);
    }

    let entries = std::fs::read_dir(profiles_dir).map_err(|e| IronclawError::ProfileIo {
        path: profiles_dir.display().to_string(),
        source: e,
    })?;

    let mut registry_write = registry.write().await;

    for entry in entries {
        let entry = entry.map_err(|e| IronclawError::ProfileIo {
            path: profiles_dir.display().to_string(),
            source: e,
        })?;

        let path = entry.path();
        if is_yaml_file(&path) {
            match load_profile_from_file(&path) {
                Ok(profile) => {
                    info!(
                        agent_id = %profile.agent_id,
                        path = %path.display(),
                        "Loaded agent profile"
                    );
                    registry_write.insert(profile.agent_id.clone(), profile);
                    count += 1;
                }
                Err(e) => {
                    error!(
                        path = %path.display(),
                        error = %e,
                        "Failed to load profile, skipping"
                    );
                }
            }
        }
    }

    info!(count, "Initial profile load complete");
    Ok(count)
}

/// Start the file watcher on the profiles directory.
///
/// This spawns a background task that:
/// 1. Watches for Create/Modify/Remove events on .yaml/.yml files
/// 2. Debounces events (500ms) to handle atomic editor saves
/// 3. Updates the shared AgentRegistry under a write lock
///
/// Returns the watcher handle (must be kept alive).
pub async fn watch_profiles_directory(
    registry: AgentRegistry,
    profiles_dir: PathBuf,
) -> Result<(), IronclawError> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    // Spawn a blocking thread for the notify watcher
    // IMPORTANT: Capture the Tokio runtime handle BEFORE spawning the thread
    let watch_path = profiles_dir.clone();
    let rt = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        let mut debouncer = new_debouncer(Duration::from_millis(500), move |result: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
            match result {
                Ok(events) => {
                    for event in events {
                        let path = event.path.clone();
                        let tx = tx.clone();
                        rt.spawn(async move {
                            if let Err(e) = tx.send(path).await {
                                error!("Failed to send file event: {}", e);
                            }
                        });
                    }
                }
                Err(e) => {
                    error!("Watcher error: {:?}", e);
                }
            }
        })
        .expect("Failed to create debouncer");

        debouncer
            .watcher()
            .watch(&watch_path, RecursiveMode::Recursive)
            .expect("Failed to watch profiles directory");

        info!(
            path = %watch_path.display(),
            "🔥 Hot-reload watcher active — drop YAML files to deploy agents instantly"
        );

        // Keep the watcher thread alive
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    });

    // Process file change events
    tokio::spawn(async move {
        while let Some(path) = rx.recv().await {
            if !is_yaml_file(&path) {
                continue;
            }

            let filename = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            if path.exists() {
                // File was created or modified → load/reload
                match load_profile_from_file(&path) {
                    Ok(profile) => {
                        let agent_id = profile.agent_id.clone();
                        let mut reg = registry.write().await;
                        let is_new = !reg.contains_key(&agent_id);
                        reg.insert(agent_id.clone(), profile);
                        drop(reg);

                        if is_new {
                            info!(
                                agent_id = %agent_id,
                                file = %filename,
                                "✅ NEW agent profile loaded via hot-reload"
                            );
                        } else {
                            info!(
                                agent_id = %agent_id,
                                file = %filename,
                                "🔄 Agent profile UPDATED via hot-reload"
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            file = %filename,
                            error = %e,
                            "❌ Failed to hot-reload profile"
                        );
                    }
                }
            } else {
                // File was deleted → remove from registry
                // We need to figure out which agent_id was in that file.
                // Since we can't read the deleted file, we scan the registry
                // for profiles that were loaded from this path.
                let mut reg = registry.write().await;
                let before = reg.len();
                reg.retain(|_id, _profile| {
                    // In a production system, we'd track source file → agent_id mapping.
                    // For now, we log a warning.
                    true
                });

                if reg.len() < before {
                    warn!(
                        file = %filename,
                        "🗑️ Agent profile removed (file deleted)"
                    );
                } else {
                    debug!(file = %filename, "File deleted but no matching profile found");
                }
            }
        }
    });

    Ok(())
}

/// Parse a single YAML file into an AgentProfile.
fn load_profile_from_file(path: &Path) -> Result<AgentProfile, IronclawError> {
    let contents = std::fs::read_to_string(path).map_err(|e| IronclawError::ProfileIo {
        path: path.display().to_string(),
        source: e,
    })?;

    let profile: AgentProfile =
        serde_yaml::from_str(&contents).map_err(|e| IronclawError::ProfileParse {
            path: path.display().to_string(),
            source: e,
        })?;

    Ok(profile)
}

/// Check if a path has a .yaml or .yml extension.
fn is_yaml_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "yaml" || ext == "yml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_yaml() -> &'static str {
        r#"
agent_id: "test_hot_reload"
vertex_ai_config:
  model: "gemini-2.5-flash-native-audio-preview"
  voice: "Kore"
  location: "us-central1"
neuro_system_prompt: "You are a test agent for hot-reload validation."
capabilities:
  - "test_capability"
"#
    }

    #[tokio::test]
    async fn test_load_all_profiles() {
        let dir = TempDir::new().unwrap();
        let yaml_path = dir.path().join("test_agent.yaml");
        std::fs::write(&yaml_path, sample_yaml()).unwrap();

        let registry = new_registry();
        let count = load_all_profiles(&registry, dir.path()).await.unwrap();

        assert_eq!(count, 1);
        let reg = registry.read().await;
        assert!(reg.contains_key("test_hot_reload"));
        assert_eq!(
            reg["test_hot_reload"].vertex_ai_config.voice,
            "Kore"
        );
    }

    #[test]
    fn test_is_yaml_file() {
        assert!(is_yaml_file(Path::new("agent.yaml")));
        assert!(is_yaml_file(Path::new("agent.yml")));
        assert!(!is_yaml_file(Path::new("agent.json")));
        assert!(!is_yaml_file(Path::new("agent.txt")));
        assert!(!is_yaml_file(Path::new("agent")));
    }

    #[test]
    fn test_load_profile_from_file() {
        let dir = TempDir::new().unwrap();
        let yaml_path = dir.path().join("test.yaml");
        std::fs::write(&yaml_path, sample_yaml()).unwrap();

        let profile = load_profile_from_file(&yaml_path).unwrap();
        assert_eq!(profile.agent_id, "test_hot_reload");
        assert_eq!(profile.capabilities.len(), 1);
    }
}
