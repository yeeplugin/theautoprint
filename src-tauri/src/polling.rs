use crate::models::PrintLog;
use crate::storage::AppStateWrapper;
use crate::printer;
use tauri::{AppHandle, Manager};

/// Print a raw job pushed from the WebSocket broker
pub async fn execute_raw_print(
    app: &AppHandle,
    job_id: &str,
    title: &str,
    content_type: &str,
    content: &str,
    printer_name: &str,
) -> PrintLog {
    let state = app.state::<AppStateWrapper>();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let result = if content_type == "pdf_uri" {
        println!("📥 WS Job: Downloading PDF from: {}", content);
        
        // Try downloading publicly without authentication using reqwest
        match reqwest::get(content).await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.bytes().await {
                        Ok(bytes) => {
                            let temp_dir = std::env::temp_dir();
                            let temp_file = temp_dir.join(format!("woo_raw_job_{}_{}.pdf", job_id, printer_name.replace(' ', "_")));
                            
                            match std::fs::write(&temp_file, bytes) {
                                Ok(_) => {
                                    let print_res = printer::print_pdf(printer_name, &temp_file);
                                    let _ = std::fs::remove_file(&temp_file);
                                    print_res
                                }
                                Err(e) => Err(format!("Failed to write temp PDF file: {}", e)),
                            }
                        }
                        Err(err) => Err(format!("Failed to read PDF response bytes: {}", err)),
                    }
                } else {
                    Err(format!("Server returned error code: {}", resp.status()))
                }
            }
            Err(e) => Err(format!("Failed to download PDF: {}", e)),
        }
    } else if content_type == "image_uri" {
        println!("📥 WS Job: Downloading image from: {}", content);
        match reqwest::get(content).await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.bytes().await {
                        Ok(bytes) => {
                            let ext = printer::detect_image_ext(&bytes);
                            let temp_dir = std::env::temp_dir();
                            let temp_file = temp_dir.join(format!("woo_raw_job_{}_{}.{}", job_id, printer_name.replace(' ', "_"), ext));
                            match std::fs::write(&temp_file, bytes) {
                                Ok(_) => {
                                    let print_res = printer::print_image(printer_name, &temp_file);
                                    let _ = std::fs::remove_file(&temp_file);
                                    print_res
                                }
                                Err(e) => Err(format!("Failed to write temp image file: {}", e)),
                            }
                        }
                        Err(err) => Err(format!("Failed to read image response bytes: {}", err)),
                    }
                } else {
                    Err(format!("Server returned error code: {}", resp.status()))
                }
            }
            Err(e) => Err(format!("Failed to download image: {}", e)),
        }
    } else if content_type == "image_base64" {
        println!("📥 WS Job: Decoding base64 image");
        use base64::Engine;
        match base64::prelude::BASE64_STANDARD.decode(content.trim()) {
            Ok(img_bytes) => {
                let ext = printer::detect_image_ext(&img_bytes);
                let temp_dir = std::env::temp_dir();
                let temp_file = temp_dir.join(format!("woo_raw_job_{}_{}.{}", job_id, printer_name.replace(' ', "_"), ext));
                match std::fs::write(&temp_file, img_bytes) {
                    Ok(_) => {
                        let print_res = printer::print_image(printer_name, &temp_file);
                        let _ = std::fs::remove_file(&temp_file);
                        print_res
                    }
                    Err(e) => Err(format!("Failed to write temp image file: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to decode base64 image data: {}", e)),
        }
    } else if content_type == "raw_base64" {
        // ESC/POS (or other printer-native) bytes, base64-encoded. Sent straight
        // to the device as a raw job — ideal for thermal receipt printers.
        println!("📥 WS Job: Decoding base64 raw payload");
        use base64::Engine;
        match base64::prelude::BASE64_STANDARD.decode(content.trim()) {
            Ok(raw_bytes) => printer::print_raw(printer_name, &raw_bytes),
            Err(e) => Err(format!("Failed to decode base64 raw data: {}", e)),
        }
    } else if content_type == "pdf_base64" {
        println!("📥 WS Job: Decoding base64 PDF");
        use base64::Engine;
        match base64::prelude::BASE64_STANDARD.decode(content.trim()) {
            Ok(pdf_bytes) => {
                let temp_dir = std::env::temp_dir();
                let temp_file = temp_dir.join(format!("woo_raw_job_{}_{}.pdf", job_id, printer_name.replace(' ', "_")));
                
                match std::fs::write(&temp_file, pdf_bytes) {
                    Ok(_) => {
                        let print_res = printer::print_pdf(printer_name, &temp_file);
                        let _ = std::fs::remove_file(&temp_file);
                        print_res
                    }
                    Err(e) => Err(format!("Failed to write temp PDF file: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to decode base64 print data: {}", e)),
        }
    } else {
        // Default to HTML printing
        printer::print_html(printer_name, content)
    };

    let log = match result {
        Ok(_) => PrintLog {
            order_id: 0,
            order_number: title.to_string(),
            printed_at: now,
            printer_name: printer_name.to_string(),
            status: "success".to_string(),
            message: format!("Job '{}' printed successfully on {}", title, printer_name),
        },
        Err(e) => PrintLog {
            order_id: 0,
            order_number: title.to_string(),
            printed_at: now,
            printer_name: printer_name.to_string(),
            status: "error".to_string(),
            message: format!("Job '{}' failed on {}: {}", title, printer_name, e),
        },
    };

    // Save log to state
    {
        let mut s = state.state.lock().unwrap();
        s.print_logs.push(log.clone());
        if s.print_logs.len() > 200 {
            let split_at = s.print_logs.len() - 200;
            s.print_logs = s.print_logs.split_off(split_at);
        }
    }
    state.save().ok();

    log
}
