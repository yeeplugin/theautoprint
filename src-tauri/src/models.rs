use serde::{Deserialize, Serialize};

/// App Settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WooSettings {
    #[serde(default)]
    pub selected_printers: Vec<String>,
    #[serde(default)]
    pub broker_url: String,
    #[serde(default)]
    pub broker_client_id: String,
    #[serde(default)]
    pub socket_token: String,
    #[serde(default)]
    pub enable_notifications: bool,
}

impl Default for WooSettings {
    fn default() -> Self {
        Self {
            selected_printers: Vec::new(),
            broker_url: String::new(),
            broker_client_id: String::new(),
            socket_token: String::new(),
            enable_notifications: false,
        }
    }
}

/// Printer info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterInfo {
    pub name: String,
    pub is_default: bool,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub page_width: Option<String>,
    #[serde(default)]
    pub page_height: Option<String>,
}

/// Print log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintLog {
    pub order_id: u64,
    pub order_number: String,
    pub printed_at: String,
    pub printer_name: String,
    pub status: String, // "success" or "error"
    pub message: String,
}

/// App state persisted to disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub settings: WooSettings,
    pub print_logs: Vec<PrintLog>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: WooSettings::default(),
            print_logs: Vec::new(),
        }
    }
}
