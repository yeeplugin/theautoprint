use crate::models::AppState;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// Get the app data directory
fn get_data_dir() -> PathBuf {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("the-auto-print");
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).ok();
    }
    data_dir
}

/// Get path to the settings file
fn get_settings_path() -> PathBuf {
    get_data_dir().join("settings.json")
}

/// Load app state from disk
pub fn load_state() -> AppState {
    let path = get_settings_path();
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<AppState>(&content) {
                Ok(state) => return state,
                Err(e) => {
                    eprintln!("Error parsing settings: {}", e);
                }
            },
            Err(e) => {
                eprintln!("Error reading settings: {}", e);
            }
        }
    }
    AppState::default()
}

/// Save app state to disk
pub fn save_state(state: &AppState) -> Result<(), String> {
    let path = get_settings_path();
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("Failed to write settings: {}", e))?;
    Ok(())
}

/// Thread-safe app state wrapper
pub struct AppStateWrapper {
    pub state: Mutex<AppState>,
}

impl AppStateWrapper {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(load_state()),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let state = self.state.lock().map_err(|e| e.to_string())?;
        save_state(&state)
    }
}
