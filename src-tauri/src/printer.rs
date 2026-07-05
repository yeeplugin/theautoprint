use crate::models::PrinterInfo;
use printers::common::base::job::PrinterJobOptions;
use printers::common::base::printer::PrinterState;
use std::path::Path;

const DEBUG_DIR: &str = "/Applications/web/app/woo-auto-printer/debug_prints";

/// Simulated/virtual printers used for debugging: output is written to disk
/// instead of being sent to a physical device.
fn is_simulated(printer_name: &str) -> bool {
    printer_name.starts_with("Simulated_") || printer_name.starts_with("Engine_")
}

fn save_debug_file(printer_name: &str, ext: &str, write: impl FnOnce(&Path) -> std::io::Result<()>) -> Result<(), String> {
    let debug_dir = Path::new(DEBUG_DIR);
    if !debug_dir.exists() {
        let _ = std::fs::create_dir_all(debug_dir);
    }
    let file_path = debug_dir.join(format!("{}_print.{}", printer_name, ext));
    write(&file_path).map_err(|e| format!("Failed to save debug print file: {}", e))?;
    println!("📄 [Simulated Print] Đã lưu bản in ra: {:?}", file_path);
    Ok(())
}

fn state_to_status(state: &PrinterState) -> String {
    match state {
        PrinterState::READY => "Idle",
        PrinterState::PRINTING => "Printing",
        PrinterState::PAUSED => "Paused",
        PrinterState::OFFLINE => "Offline",
        PrinterState::UNKNOWN => "Unknown",
    }
    .to_string()
}

/// List available printers using native OS APIs:
/// CUPS (libcups) on macOS/Linux, winspool on Windows. No shell commands.
pub fn list_printers() -> Result<Vec<PrinterInfo>, String> {
    let printers = printers::get_printers()
        .into_iter()
        .map(|p| PrinterInfo {
            id: p.system_name.clone(),
            name: p.name.clone(),
            is_default: p.is_default,
            status: state_to_status(&p.state),
            // Default media size is not exposed by the cross-platform API; the UI
            // falls back to "Custom" when these are None.
            page_width: None,
            page_height: None,
        })
        .collect();

    Ok(printers)
}

/// Print an HTML string.
///
/// HTML has no native, dependency-free print path (it must be rendered by a
/// browser engine first). The correct architecture is to render HTML -> PDF on
/// the broker server and push a `pdf_uri`/`pdf_base64` job, which
/// [`print_pdf`] handles natively. This function only supports the simulated
/// debug printers; real HTML jobs return an actionable error.
pub fn print_html(printer_name: &str, html_content: &str) -> Result<(), String> {
    if is_simulated(printer_name) {
        return save_debug_file(printer_name, "html", |path| std::fs::write(path, html_content));
    }

    Err(
        "HTML printing is not supported natively on the client. Configure the broker to render \
         the order to PDF and send a pdf_base64 or pdf_uri job instead."
            .to_string(),
    )
}

/// Print a local PDF file using native OS printing.
///
/// - macOS/Linux: `cupsPrintFile` via libcups. CUPS runs the file through its
///   filter chain, so PDFs are rendered and printed correctly.
/// - Windows: PDFium renders each page and sends it to the printer via GDI
///   (winspool). No Edge/Chrome/PowerShell involved.
pub fn print_pdf(printer_name: &str, pdf_file_path: &Path) -> Result<(), String> {
    if is_simulated(printer_name) {
        return save_debug_file(printer_name, "pdf", |path| {
            std::fs::copy(pdf_file_path, path).map(|_| ())
        });
    }

    #[cfg(not(windows))]
    {
        let printer = printers::get_printer_by_name(printer_name)
            .ok_or_else(|| format!("Printer '{}' not found", printer_name))?;
        let path_str = pdf_file_path
            .to_str()
            .ok_or_else(|| "Invalid PDF path".to_string())?;
        printer
            .print_file(path_str, PrinterJobOptions::none())
            .map(|_| ())
            .map_err(|e| format!("PDF print failed: {:?}", e))
    }

    #[cfg(windows)]
    {
        use winprint::printer::{FilePrinter, PdfiumPrinter, PrinterDevice};
        use winprint::ticket::PrintTicketBuilder;

        let device = PrinterDevice::all()
            .map_err(|e| format!("Failed to enumerate printers: {}", e))?
            .into_iter()
            .find(|d| d.name() == printer_name)
            .ok_or_else(|| format!("Printer '{}' not found", printer_name))?;

        let ticket = PrintTicketBuilder::new(&device)
            .map_err(|e| format!("Failed to create print ticket: {}", e))?
            .build()
            .map_err(|e| format!("Failed to build print ticket: {}", e))?;

        PdfiumPrinter::new(device)
            .print(pdf_file_path, ticket)
            .map_err(|e| format!("PDF print failed: {:?}", e))
    }
}

/// Print a local image file (PNG/JPEG/BMP/GIF/TIFF) using native OS printing.
///
/// - macOS/Linux: CUPS filter chain rasterizes the image (`cupsPrintFile`).
/// - Windows: winprint's ImagePrinter decodes via WIC and prints through
///   Direct2D/GDI — same native path as PDF, no subprocess.
pub fn print_image(printer_name: &str, image_file_path: &Path) -> Result<(), String> {
    if is_simulated(printer_name) {
        let ext = image_file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_string();
        return save_debug_file(printer_name, &ext, |path| {
            std::fs::copy(image_file_path, path).map(|_| ())
        });
    }

    #[cfg(not(windows))]
    {
        let printer = printers::get_printer_by_name(printer_name)
            .ok_or_else(|| format!("Printer '{}' not found", printer_name))?;
        let path_str = image_file_path
            .to_str()
            .ok_or_else(|| "Invalid image path".to_string())?;
        printer
            .print_file(path_str, PrinterJobOptions::none())
            .map(|_| ())
            .map_err(|e| format!("Image print failed: {:?}", e))
    }

    #[cfg(windows)]
    {
        use winprint::printer::{FilePrinter, ImagePrinter, PrinterDevice};
        use winprint::ticket::PrintTicketBuilder;

        let device = PrinterDevice::all()
            .map_err(|e| format!("Failed to enumerate printers: {}", e))?
            .into_iter()
            .find(|d| d.name() == printer_name)
            .ok_or_else(|| format!("Printer '{}' not found", printer_name))?;

        let ticket = PrintTicketBuilder::new(&device)
            .map_err(|e| format!("Failed to create print ticket: {}", e))?
            .build()
            .map_err(|e| format!("Failed to build print ticket: {}", e))?;

        ImagePrinter::new(device)
            .print(image_file_path, ticket)
            .map_err(|e| format!("Image print failed: {:?}", e))
    }
}

/// Sniff the image format from magic bytes so temp files get a correct
/// extension (helps CUPS mime detection and debug output naming).
pub fn detect_image_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else if bytes.starts_with(b"BM") {
        "bmp"
    } else if bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        "tiff"
    } else {
        "png"
    }
}

/// Send a raw byte payload (e.g. ESC/POS commands) directly to a printer.
///
/// Thermal receipt printers accept ESC/POS as a raw byte stream, which is far
/// faster and more reliable than rendering HTML. On CUPS the job is marked as
/// `application/vnd.cups-raw` to bypass filtering; winspool prints RAW by
/// default.
pub fn print_raw(printer_name: &str, data: &[u8]) -> Result<(), String> {
    if is_simulated(printer_name) {
        return save_debug_file(printer_name, "bin", |path| std::fs::write(path, data));
    }

    let printer = printers::get_printer_by_name(printer_name)
        .ok_or_else(|| format!("Printer '{}' not found", printer_name))?;

    let raw_properties = [("document-format", "application/vnd.cups-raw")];
    let options = PrinterJobOptions {
        name: Some("YeePrint raw job"),
        raw_properties: &raw_properties,
        ..PrinterJobOptions::none()
    };

    printer
        .print(data, options)
        .map(|_| ())
        .map_err(|e| format!("Raw print failed: {:?}", e))
}
