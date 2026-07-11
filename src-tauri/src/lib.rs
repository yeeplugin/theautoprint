mod models;
mod polling;
mod printer;
mod storage;

use models::{PrintLog, WooSettings};
use storage::AppStateWrapper;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tauri::Emitter;
use std::io::{Read, Write};
use std::net::TcpListener;

#[derive(serde::Serialize, serde::Deserialize)]
struct LoginPayload {
    username: String,
    password: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct LoginResponse {
    success: bool,
    client_id: String,
    socket_token: String,
    user_display_name: String,
    user_email: String,
}

#[derive(Clone, serde::Serialize)]
struct SsoSuccessPayload {
    client_id: String,
    socket_token: String,
}

// ============================================================
// Tauri Commands
// ============================================================

/// Save App settings
#[tauri::command]
fn save_settings(
    state: tauri::State<'_, AppStateWrapper>,
    settings: WooSettings,
) -> Result<String, String> {
    let mut s = state.state.lock().map_err(|e| e.to_string())?;
    s.settings = settings;
    drop(s);
    state.save()?;
    Ok("Settings saved".to_string())
}

/// Get current settings
#[tauri::command]
fn get_settings(state: tauri::State<'_, AppStateWrapper>) -> Result<WooSettings, String> {
    let s = state.state.lock().map_err(|e| e.to_string())?;
    Ok(s.settings.clone())
}

/// List available printers
#[tauri::command]
fn list_printers() -> Result<Vec<models::PrinterInfo>, String> {
    printer::list_printers()
}

/// Get print logs
#[tauri::command]
fn get_print_logs(state: tauri::State<'_, AppStateWrapper>) -> Result<Vec<PrintLog>, String> {
    let s = state.state.lock().map_err(|e| e.to_string())?;
    Ok(s.print_logs.clone())
}

/// Clear print logs
#[tauri::command]
fn clear_print_logs(state: tauri::State<'_, AppStateWrapper>) -> Result<(), String> {
    let mut s = state.state.lock().map_err(|e| e.to_string())?;
    s.print_logs.clear();
    drop(s);
    state.save()?;
    Ok(())
}

/// Print a raw job pushed from the WebSocket broker.
///
/// When the broker targets a specific printer (`printer_name`), print only
/// there; otherwise (legacy broker) fall back to every selected printer.
#[tauri::command]
async fn print_raw_job(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppStateWrapper>,
    job_id: String,
    title: String,
    content_type: String,
    content: String,
    printer_name: Option<String>,
) -> Result<PrintLog, String> {
    let printers = match printer_name {
        Some(name) if !name.is_empty() => vec![name],
        _ => {
            let s = state.state.lock().map_err(|e| e.to_string())?;
            s.settings.selected_printers.clone()
        }
    };

    if printers.is_empty() {
        return Err("No printers selected".to_string());
    }

    let mut last_log = None;
    for printer_name in &printers {
        let log = polling::execute_raw_print(&app, &job_id, &title, &content_type, &content, printer_name).await;
        last_log = Some(log);
    }

    if let Some(log) = last_log {
        if log.status == "success" {
            Ok(log)
        } else {
            Err(log.message)
        }
    } else {
        Err("Print job execution failed".to_string())
    }
}

/// Log a custom event (e.g. connection success or failure) to the print logs
#[tauri::command]
fn add_log_entry(
    state: tauri::State<'_, AppStateWrapper>,
    order_number: String,
    status: String,
    message: String,
    printer_name: String,
) -> Result<(), String> {
    let mut s = state.state.lock().map_err(|e| e.to_string())?;
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    
    let log = PrintLog {
        order_id: 0,
        order_number,
        printed_at: now,
        printer_name,
        status,
        message,
    };
    
    s.print_logs.push(log);
    if s.print_logs.len() > 200 {
        let split_at = s.print_logs.len() - 200;
        s.print_logs = s.print_logs.split_off(split_at);
    }
    
    drop(s);
    state.save()?;
    Ok(())
}

/// Get the computer/device hostname name
#[tauri::command]
fn get_computer_name() -> Result<String, String> {
    // 1. Try Windows environment variable
    if let Ok(name) = std::env::var("COMPUTERNAME") {
        return Ok(name);
    }
    
    // 2. Try native libc gethostname on Unix (macOS, Linux)
    #[cfg(unix)]
    {
        extern "C" {
            fn gethostname(name: *mut std::os::raw::c_char, len: usize) -> std::os::raw::c_int;
        }

        let mut buf = vec![0; 256];
        let len = buf.len();
        unsafe {
            if gethostname(buf.as_mut_ptr() as *mut std::os::raw::c_char, len) == 0 {
                if let Ok(c_str) = std::ffi::CStr::from_ptr(buf.as_ptr() as *const std::os::raw::c_char).to_str() {
                    let name = c_str.trim().to_string();
                    if !name.is_empty() {
                        return Ok(name);
                    }
                }
            }
        }
    }

    // 3. Fallback to HOSTNAME env variable
    if let Ok(name) = std::env::var("HOSTNAME") {
        return Ok(name);
    }

    Ok("Unknown Computer".to_string())
}

/// Start a temporary HTTP server on localhost to receive SSO callback from browser
#[tauri::command]
async fn start_sso_server(app_handle: tauri::AppHandle) -> Result<u16, String> {
    // Bind to localhost on a random free port (port 0 requests a free port from OS)
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to start local listener: {}", e))?;
    let port = listener.local_addr().unwrap().port();

    // Listen on background task
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0; 2048];
            if let Ok(bytes_read) = stream.read(&mut buffer) {
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);
                
                // Parse path line, e.g. GET /callback?client_id=test&token=abc HTTP/1.1
                if let Some(path_line) = request.lines().next() {
                    let parts: Vec<&str> = path_line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let path = parts[1];
                        if let Ok(parsed_url) = reqwest::Url::parse(&format!("http://localhost{}", path)) {
                            let mut client_id = String::new();
                            let mut socket_token = String::new();
                            
                            for (key, val) in parsed_url.query_pairs() {
                                if key == "client_id" {
                                    client_id = val.to_string();
                                } else if key == "token" {
                                    socket_token = val.to_string();
                                }
                            }
                            
                            if !client_id.is_empty() && !socket_token.is_empty() {
                                let response_body = r#"
                                    <!DOCTYPE html>
                                    <html>
                                    <head>
                                        <title>The Auto Print Login Success</title>
                                        <style>
                                            body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background-color: #f8fafc; color: #1e293b; }
                                            .card { background: white; padding: 40px; border-radius: 16px; box-shadow: 0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1); text-align: center; max-width: 400px; width: 100%; }
                                            h1 { color: #10b981; font-size: 24px; margin-bottom: 16px; }
                                            p { color: #64748b; font-size: 14px; line-height: 1.6; }
                                        </style>
                                    </head>
                                    <body>
                                        <div class="card">
                                            <h1>✓ Authenticated Successfully</h1>
                                            <p>Your The Auto Print Desktop App is now connected. You can close this browser tab and return to the application.</p>
                                        </div>
                                    </body>
                                    </html>
                                "#;
                                
                                let response = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    response_body.trim().len(),
                                    response_body.trim()
                                );
                                
                                let _ = stream.write_all(response.as_bytes());
                                let _ = stream.flush();
                                
                                // Emit SSO login success event to the Javascript frontend
                                let _ = app_handle.emit("sso_login_success", SsoSuccessPayload {
                                    client_id,
                                    socket_token,
                                });
                                return;
                            }
                        }
                    }
                }
            }
            
            // Fallback response for failed auth
            let response = "HTTP/1.1 400 Bad Request\r\nContent-Length: 15\r\nConnection: close\r\n\r\nInvalid request";
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    Ok(port)
}

/// Natively opens a URL in the user's default browser.
///
/// Uses the tauri-plugin-opener plugin, which calls the OS shell-open API
/// (ShellExecuteW on Windows, LSOpenCFURLRef on macOS, xdg-open on Linux)
/// directly — no cmd/PowerShell subprocess is spawned.
#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Fetch the Broker URL (hardcoded for theautoprint.com)
#[tauri::command]
fn fetch_broker_url() -> Result<String, String> {
    Ok("wss://theautoprint.com/yp-broker/".to_string())
}

/// Authenticate user via WordPress REST API using Application Passwords (bypasses CORS policy of Webview)
#[tauri::command]
async fn login_user(payload: LoginPayload) -> Result<LoginResponse, String> {
    let client = reqwest::Client::new();
    let res = client.post("https://theautoprint.com/wp-json/yeeprint-auth/v1/me")
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = res.status();
    let body = res.text().await.map_err(|e| e.to_string())?;

    if status.is_success() {
        let login_res: LoginResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        Ok(login_res)
    } else {
        if let Ok(json_err) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(msg) = json_err.get("message") {
                return Err(msg.as_str().unwrap_or("Authentication failed").to_string());
            }
        }
        Err(format!("Authentication failed with HTTP code {}", status))
    }
}

// ============================================================
// Entry Point
// ============================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .manage(AppStateWrapper::new())
        .setup(|app| {
            // Dyn load pdfium.dll on Windows so that winprint/pdfium-render can find it inside MS Store / MSIX sandbox
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::ffi::OsStrExt;
                use tauri::Manager;

                // The DLL is bundled at the resource-dir ROOT ("pdfium.dll" in the
                // tauri.conf resources map), which on Windows is the exe dir.
                // Probe every plausible location; first successful load wins.
                let mut candidates: Vec<std::path::PathBuf> = Vec::new();
                if let Ok(p) = app.path().resolve("pdfium.dll", tauri::path::BaseDirectory::Resource) {
                    candidates.push(p);
                }
                // Older bundles: "../resources/pdfium.dll" landed in _up_/resources/.
                if let Ok(p) = app.path().resolve("_up_/resources/pdfium.dll", tauri::path::BaseDirectory::Resource) {
                    candidates.push(p);
                }
                if let Ok(exe) = std::env::current_exe() {
                    if let Some(dir) = exe.parent() {
                        candidates.push(dir.join("pdfium.dll"));
                    }
                }
                let mut loaded = false;
                for resource_path in candidates {
                    if !resource_path.exists() {
                        continue;
                    }
                    let wide: Vec<u16> = resource_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                    unsafe {
                        extern "system" {
                            fn LoadLibraryW(lpLibFileName: *const u16) -> *mut std::ffi::c_void;
                        }
                        let handle = LoadLibraryW(wide.as_ptr());
                        if !handle.is_null() {
                            println!("Successfully loaded pdfium.dll from: {:?}", resource_path);
                            loaded = true;
                            break;
                        } else {
                            eprintln!("Failed to load pdfium.dll from: {:?}", resource_path);
                        }
                    }
                }
                if !loaded {
                    eprintln!("pdfium.dll not found in any known location — PDF printing will fail.");
                }
            }

            let quit_i = MenuItem::with_id(app, "quit", "Exit Application", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Open Window", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            save_settings,
            get_settings,
            list_printers,
            get_print_logs,
            clear_print_logs,
            print_raw_job,
            add_log_entry,
            fetch_broker_url,
            login_user,
            get_computer_name,
            start_sso_server,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
