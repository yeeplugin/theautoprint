# The Auto Print - Remote Printing Client

[![Tauri App](https://img.shields.io/badge/Framework-Tauri_v2-blue.svg)](https://tauri.app)
[![Vanilla JS](https://img.shields.io/badge/Frontend-Vanilla_JS-yellow.svg)](https://developer.mozilla.org/en-US/docs/Web/JavaScript)
[![Rust Backed](https://img.shields.io/badge/Backend-Rust-orange.svg)](https://www.rust-lang.org)

**The Auto Print - Remote Printing Client** is a lightweight, high-performance cross-platform desktop application built with **Tauri v2** and **Rust**. It runs silently in your system tray, listens for cloud print jobs in real-time, and sends them directly to your designated local/network printers.

It works hand-in-hand with [The Auto Print for WooCommerce](https://theautoprint.com) plugin to provide instant, hands-free order printing for kitchens, warehouses, and shops.

---

## ✨ Features

*   **Zero-Latency WebSockets**: Connects directly to the cloud broker with an advanced reconnect backoff policy and a heartbeat ping/pong watchdog to maintain a rock-solid link.
*   **System Tray Integration**: Minimizes to the system tray to run quietly in the background without cluttering your taskbar.
*   **Auto-Start on Boot**: Configurable option to automatically launch when your computer starts up (uses the native Tauri autostart plugin).
*   **Real-time Printer Scanner**: Automatically discovers and lists all connected local, network, and default printers.
*   **Multi-Printer Selection**: Select one or multiple printers to map print jobs.
*   **SSO & Passwordless Login**: Log in seamlessly using:
    *   🔑 **Google SSO** (starts a local auth listener port and completes in browser).
    *   🛡️ **Passkey (FIDO2/WebAuthn)**.
    *   📧 Traditional email/password credentials.
*   **Desktop Notifications**: Get native OS notification alerts when a new job is received.
*   **Activity Logs**: Review complete history of print tasks, connection logs, and status errors right in the app.

---

## 🛠️ Getting Started (For Developers)

### Prerequisites
To build or run this application locally, you must install the Tauri prerequisites for your operating system:
*   **Rust** (via `rustup`)
*   **Node.js** (v18 or higher)
*   **macOS**: Xcode Command Line Tools.
*   **Windows**: Microsoft Visual Studio C++ Build Tools & WebView2.
*   **Linux**: `build-essential`, `webkit2gtk`, and other library dependencies. See the [Tauri Setup Guide](https://v2.tauri.app/start/prerequisites/) for details.

### Installation

1.  Clone this repository and navigate to the project directory:
    ```bash
    cd /path/to/woo-auto-printer
    ```

2.  Install dependencies:
    ```bash
    npm install
    ```

### Development Mode

Run the app in development mode with hot-reloading:
```bash
npm run tauri dev
```

### Production Build

Compile the application into optimized native installers (e.g., `.msix`/`.exe` on Windows, `.dmg`/`.app` on macOS, `.deb` on Linux):
```bash
npm run tauri build
```

---

## 📂 Project Structure

```
├── AppxManifest.xml      # Windows packaging manifest (MSIX)
├── package.json          # Node dependencies and Tauri CLI wrapper
├── src/                  # Frontend HTML / CSS / JavaScript UI
│   ├── assets/           # UI icons & logos
│   ├── index.html        # Main app template
│   ├── main.js           # WebSocket connection & UI interaction logic
│   ├── styles.css        # Premium styling system
│   └── styles_fonts.css  # Typography configurations
└── src-tauri/            # Rust backend logic
    ├── Cargo.toml        # Rust package dependencies
    ├── src/
    │   ├── main.rs       # OS integrations, SSO servers, printer controls
    │   └── ...
    └── tauri.conf.json   # Tauri compilation configuration
```

---

## 📝 License

This project is proprietary. All rights reserved.
