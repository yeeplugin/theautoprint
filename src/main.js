// ============================================================
// YeePrint Client - Frontend Logic
// ============================================================

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Disable the webview's default right-click menu (Reload / Inspect Element…).
// Editable fields keep their menu so users can still copy/paste.
document.addEventListener('contextmenu', (e) => {
    const t = e.target;
    const editable = t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable);
    if (!editable) {
        e.preventDefault();
    }
});

// Block the matching keyboard shortcuts too: F5 / Ctrl(Cmd)+R (reload),
// F12 / Ctrl+Shift+I / Cmd+Opt+I (devtools).
document.addEventListener('keydown', (e) => {
    const k = (e.key || '').toLowerCase();
    const reload = k === 'f5' || ((e.ctrlKey || e.metaKey) && k === 'r');
    const devtools = k === 'f12'
        || (e.ctrlKey && e.shiftKey && (k === 'i' || k === 'j' || k === 'c'))
        || (e.metaKey && e.altKey && (k === 'i' || k === 'j' || k === 'c'));
    if (reload || devtools) {
        e.preventDefault();
    }
});

// ============================================================
// State
// ============================================================

let currentPage = 'dashboard';
let printers = [];
let printedToday = 0;
let brokerSocket = null;
let brokerReconnectTimeout = null;
let brokerReconnectDelay = 5000; // grows to 60s when the server rejects us (limit/auth)
let brokerHeartbeatInterval = null;
let brokerPongWatchdog = null;
let currentBrokerUrl = '';
let currentBrokerClientId = '';
let currentBrokerToken = '';

// ============================================================
// Navigation
// ============================================================

document.querySelectorAll('.nav-item').forEach(item => {
    item.addEventListener('click', () => {
        const page = item.dataset.page;
        navigateTo(page);
    });
});

function navigateTo(page) {
    // Update nav
    document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
    const activeNav = document.querySelector(`[data-page="${page}"]`);
    if (activeNav) activeNav.classList.add('active');

    // Update page
    document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
    const pageEl = document.getElementById(`page-${page}`);
    if (pageEl) {
        pageEl.classList.add('active');
        pageEl.style.animation = 'none';
        pageEl.offsetHeight; // force reflow
        pageEl.style.animation = '';
    }

    currentPage = page;

    if (page === 'logs') {
        loadLogs();
    } else if (page === 'printer') {
        scanPrinters();
    }
}

// ============================================================
// Toast Notifications
// ============================================================

function showToast(message, type = 'info', duration = 4000) {
    const container = document.getElementById('toast-container');

    const icons = {
        success: '✅',
        error: '❌',
        warning: '⚠️',
        info: 'ℹ️',
    };

    const toast = document.createElement('div');
    toast.className = `toast ${type}`;
    toast.innerHTML = `
        <span class="toast-icon">${icons[type] || icons.info}</span>
        <span class="toast-message">${message}</span>
        <button class="toast-close" onclick="this.parentElement.remove()">✕</button>
    `;

    container.appendChild(toast);

    setTimeout(() => {
        toast.classList.add('removing');
        setTimeout(() => toast.remove(), 300);
    }, duration);
}

// ============================================================
// Settings
// ============================================================

async function loadSettings() {
    try {
        const settings = await invoke('get_settings');

        if (settings.broker_url) {
            document.getElementById('broker-url').value = settings.broker_url;
        }

        document.getElementById('notifications-toggle').checked = settings.enable_notifications || false;

        if (settings.broker_client_id) {
            document.getElementById('login-container').style.display = 'none';
            document.querySelector('.app-container').style.display = 'flex';

            document.getElementById('broker-client-id').value = settings.broker_client_id;
            document.getElementById('dash-client-id').textContent = settings.broker_client_id;

            if (settings.selected_printers) {
                updateDashboardPrinters(settings.selected_printers);
            }

            // Check autostart status
            try {
                const isAutostart = await invoke('plugin:autostart|is_enabled');
                document.getElementById('autostart-toggle').checked = isAutostart;
            } catch (e) {
                console.error('Failed to check autostart status:', e);
            }

            // Connect to WebSocket
            initWebSocket(settings);
        } else {
            document.getElementById('login-container').style.display = 'flex';
            document.querySelector('.app-container').style.display = 'none';
            disconnectBroker();
        }

    } catch (e) {
        console.error('Failed to load settings:', e);
    }
}

// Save settings button
document.getElementById('btn-save-settings').addEventListener('click', async () => {
    const btn = document.getElementById('btn-save-settings');
    btn.disabled = true;
    btn.innerHTML = '<span class="spinner"></span> Saving...';

    try {
        const brokerUrl = document.getElementById('broker-url').value.trim();
        let brokerClientId = document.getElementById('broker-client-id').value.trim();
        const notificationEnabled = document.getElementById('notifications-toggle').checked;

        if (!brokerClientId) {
            brokerClientId = 'client_' + Math.random().toString(36).substring(2, 11);
            document.getElementById('broker-client-id').value = brokerClientId;
        }

        if (notificationEnabled && Notification.permission !== 'granted') {
            await Notification.requestPermission();
        }

        const settings = {
            selected_printers: [],
            broker_url: brokerUrl,
            broker_client_id: brokerClientId,
            enable_notifications: notificationEnabled,
        };

        // Preserve selected_printers and socket_token from current state
        try {
            const current = await invoke('get_settings');
            settings.selected_printers = current.selected_printers || [];
            settings.socket_token = current.socket_token || '';
        } catch (e) { /* ignore */ }

        await invoke('save_settings', { settings });

        // Save autostart setting
        try {
            const autostartEnabled = document.getElementById('autostart-toggle').checked;
            if (autostartEnabled) {
                await invoke('plugin:autostart|enable');
            } else {
                await invoke('plugin:autostart|disable');
            }
        } catch (e) {
            console.error('Failed to update autostart setting:', e);
        }

        showToast('Settings saved successfully!', 'success');
        document.getElementById('dash-client-id').textContent = brokerClientId;

        // Apply WebSocket connection
        initWebSocket(settings);

    } catch (e) {
        showToast('Failed to save settings: ' + e, 'error');
    } finally {
        btn.disabled = false;
        btn.innerHTML = 'Save Settings';
    }
});

// ============================================================
// Cloud WebSocket Broker connection
// ============================================================

async function fetchBrokerUrl() {
    return 'wss://theautoprint.com/yp-broker/';
}

// Cached computer name. EVERY register message must include computer_name —
// the broker groups printers by it, and a register without it collapses all
// printers into one "Unknown Computer" group on the dashboard.
let cachedComputerName = null;
async function getComputerName() {
    if (cachedComputerName) return cachedComputerName;
    let name = 'Unknown Computer';
    try {
        name = await invoke('get_computer_name');
        if (name.endsWith('.local')) {
            name = name.slice(0, -6);
        }
    } catch (e) {
        console.error('Failed to get computer name:', e);
    }
    cachedComputerName = name;
    return name;
}

async function initWebSocket(settings) {
    let url = settings.broker_url;

    // Always attempt to fetch the latest URL from the WordPress site config first
    const fetchedUrl = await fetchBrokerUrl();
    if (fetchedUrl) {
        url = fetchedUrl;
        document.getElementById('broker-url').value = url;
        if (settings.broker_url !== url) {
            settings.broker_url = url;
            invoke('save_settings', { settings }).catch(e => console.error(e));
        }
    } else {
        document.getElementById('broker-url').value = settings.broker_url || '';
    }

    const clientId = settings.broker_client_id;
    const token = settings.socket_token || '';

    // Enforce selection of at least 1 printer
    if (!settings.selected_printers || settings.selected_printers.length === 0) {
        disconnectBroker();
        updateBrokerUI('disconnected', 'No Printer Selected');
        return;
    }

    if (url && clientId) {
        connectToBroker(url, clientId, token);
    } else {
        disconnectBroker();
        updateBrokerUI('disconnected', 'Not Configured');
    }
}

function connectToBroker(url, clientId, token) {
    if (!url) {
        updateBrokerUI('disconnected', 'Broker URL not configured');
        return;
    }

    if (brokerSocket && (brokerSocket.readyState === WebSocket.OPEN || brokerSocket.readyState === WebSocket.CONNECTING)) {
        if (currentBrokerUrl === url && currentBrokerClientId === clientId && currentBrokerToken === token) {
            return;
        }
        disconnectBroker();
    }

    currentBrokerUrl = url;
    currentBrokerClientId = clientId;
    currentBrokerToken = token;

    console.log(`Connecting to Cloud Server: https://theautoprint.com (Client: ${clientId})`);
    updateBrokerUI('connecting', 'Connecting...');

    try {
        let wsUrl = url;
        if (wsUrl.includes('?')) {
            wsUrl += `&client_id=${encodeURIComponent(clientId)}&token=${encodeURIComponent(token)}`;
        } else {
            wsUrl += `?client_id=${encodeURIComponent(clientId)}&token=${encodeURIComponent(token)}`;
        }
        brokerSocket = new WebSocket(wsUrl);

        brokerSocket.onopen = async () => {
            console.log('Connected to Cloud https://theautoprint.com');
            updateBrokerUI('connected', 'Online');
            brokerReconnectDelay = 5000; // healthy connection — reset backoff
            startHeartbeat();

            try {
                // Log event to print logs
                await invoke('add_log_entry', {
                    orderNumber: 'Cloud Connection',
                    status: 'success',
                    message: 'Connected successfully to https://theautoprint.com',
                    printerName: 'Cloud Server'
                });

                // Scan printers and get computer name to register up-to-date printers list
                let scannedPrinters = [];
                let computerName = "Unknown Computer";
                try {
                    scannedPrinters = await invoke('list_printers');
                } catch (e) {
                    console.error('list_printers failed:', e);
                    await invoke('add_log_entry', {
                        orderNumber: 'Printer Scan Error',
                        status: 'error',
                        message: `list_printers failed: ${e.toString()}`,
                        printerName: 'Cloud Server'
                    });
                    scannedPrinters = printers;
                }
                computerName = await getComputerName();

                const payload = {
                    type: 'register',
                    client_id: clientId,
                    computer_name: computerName,
                    printers: scannedPrinters
                };

                await invoke('add_log_entry', {
                    orderNumber: 'Cloud Sync',
                    status: 'success',
                    message: `Registering device with ${scannedPrinters.length} printers.`,
                    printerName: 'Cloud Server'
                });

                brokerSocket.send(JSON.stringify(payload));
            } catch (err) {
                console.error('Error in onopen:', err);
                await invoke('add_log_entry', {
                    orderNumber: 'Cloud Connection',
                    status: 'error',
                    message: `Connection error: ${err.toString()}`,
                    printerName: 'Cloud Server'
                }).catch(e => { });
            }
        };

        brokerSocket.onmessage = async (event) => {
            // Any inbound traffic proves the link is alive — disarm the watchdog.
            if (brokerPongWatchdog) {
                clearTimeout(brokerPongWatchdog);
                brokerPongWatchdog = null;
            }
            try {
                const data = JSON.parse(event.data);

                // Heartbeat reply — nothing else to do (watchdog already reset above).
                if (data.type === 'pong') return;

                // Server-side rejection (auth failure, plan connection limit...).
                // Surface it to the user and back off: retrying every 5s can't
                // succeed until the underlying condition changes.
                if (data.type === 'error') {
                    console.warn('Cloud error:', data.message);
                    showToast(data.message || 'Connection rejected by cloud server', 'error');
                    updateBrokerUI('error', data.code === 'connection_limit' ? 'Device limit reached' : 'Rejected');
                    brokerReconnectDelay = 60000;
                    invoke('add_log_entry', {
                        orderNumber: 'Cloud Connection',
                        status: 'error',
                        message: data.message || 'Connection rejected by cloud server',
                        printerName: 'Cloud Server'
                    }).then(() => loadLogs()).catch(() => { });
                    return;
                }

                console.log('WebSocket message received:', data);

                if (data.type === 'print_job') {
                    addActivity('print', `Received job: ${data.title}`, new Date().toLocaleTimeString('en-US'));

                    // Show desktop notification if enabled
                    try {
                        const settings = await invoke('get_settings');
                        if (settings.enable_notifications && Notification.permission === 'granted') {
                            new Notification('The Auto Print - Remote Printing', {
                                body: `New print job: ${data.title}`,
                                icon: 'icons/128x128.png'
                            });
                        }
                    } catch (err) {
                        console.error('Failed to show notification:', err);
                    }

                    try {
                        const log = await invoke('print_raw_job', {
                            jobId: data.job_id,
                            title: data.title,
                            contentType: data.content_type,
                            content: data.content,
                            // Print only on the targeted printer (broker sends it);
                            // omitted/null = legacy broker → all selected printers.
                            printerName: data.printer_name || null
                        });

                        // Reply success
                        if (brokerSocket && brokerSocket.readyState === WebSocket.OPEN) {
                            brokerSocket.send(JSON.stringify({
                                type: 'print_status',
                                job_id: data.job_id,
                                status: 'success',
                                message: log.message
                            }));
                        }

                        printedToday++;
                        document.getElementById('dash-printed').textContent = printedToday;
                        addActivity('success', `Printed successfully: ${data.title}`, new Date().toLocaleTimeString('en-US'));
                    } catch (e) {
                        // Reply error
                        if (brokerSocket && brokerSocket.readyState === WebSocket.OPEN) {
                            brokerSocket.send(JSON.stringify({
                                type: 'print_status',
                                job_id: data.job_id,
                                status: 'error',
                                message: e.toString()
                            }));
                        }
                        addActivity('error', `Print failed: ${e}`, new Date().toLocaleTimeString('en-US'));
                    }
                    loadLogs();
                }
            } catch (e) {
                console.error('Failed to handle WebSocket message:', e);
            }
        };

        brokerSocket.onclose = (event) => {
            console.log('Cloud connection closed:', event.reason);
            stopHeartbeat();
            updateBrokerUI('disconnected', 'Offline');

            // Log event to print logs
            invoke('add_log_entry', {
                orderNumber: 'Cloud Connection',
                status: 'error',
                message: `Connection closed: ${event.reason || 'No specific reason given'}`,
                printerName: 'Cloud Server'
            }).then(() => loadLogs()).catch(e => console.error(e));

            // Queue reconnection (5s normally, 60s after a server-side rejection)
            if (brokerReconnectTimeout) clearTimeout(brokerReconnectTimeout);
            brokerReconnectTimeout = setTimeout(() => {
                connectToBroker(currentBrokerUrl, currentBrokerClientId, currentBrokerToken);
            }, brokerReconnectDelay);
        };

        brokerSocket.onerror = (error) => {
            console.error('WebSocket error:', error);
            updateBrokerUI('error', 'Error');

            // Log event to print logs
            invoke('add_log_entry', {
                orderNumber: 'Cloud Connection',
                status: 'error',
                message: 'Connection error encountered on https://theautoprint.com',
                printerName: 'Cloud Server'
            }).then(() => loadLogs()).catch(e => console.error(e));
        };

    } catch (e) {
        console.error('Failed to create WebSocket:', e);
        updateBrokerUI('error', e.toString());
    }
}

function disconnectBroker() {
    stopHeartbeat();
    if (brokerReconnectTimeout) {
        clearTimeout(brokerReconnectTimeout);
        brokerReconnectTimeout = null;
    }
    if (brokerSocket) {
        brokerSocket.onopen = null;
        brokerSocket.onmessage = null;
        brokerSocket.onclose = null;
        brokerSocket.onerror = null;

        if (brokerSocket.readyState === WebSocket.OPEN || brokerSocket.readyState === WebSocket.CONNECTING) {
            brokerSocket.close();
        }
        brokerSocket = null;
    }
    console.log('Disconnected from Cloud Broker');
}

// ── Heartbeat: keep the connection alive through proxies and detect a silently
// dead link. We send an app-level ping every 25s; the broker replies with a
// pong. Any message resets the watchdog, so if no reply arrives within 10s we
// assume the link is dead and force a reconnect (onclose -> reconnect).
function startHeartbeat() {
    stopHeartbeat();
    brokerHeartbeatInterval = setInterval(() => {
        if (!brokerSocket || brokerSocket.readyState !== WebSocket.OPEN) return;
        try {
            brokerSocket.send(JSON.stringify({ type: 'ping' }));
        } catch (e) {
            console.error('Heartbeat send failed:', e);
        }
        if (brokerPongWatchdog) clearTimeout(brokerPongWatchdog);
        brokerPongWatchdog = setTimeout(() => {
            console.warn('No response from broker — link is dead, forcing reconnect');
            if (brokerSocket) {
                try { brokerSocket.close(); } catch (e) { }
            }
        }, 10000);
    }, 25000);
}

function stopHeartbeat() {
    if (brokerHeartbeatInterval) {
        clearInterval(brokerHeartbeatInterval);
        brokerHeartbeatInterval = null;
    }
    if (brokerPongWatchdog) {
        clearTimeout(brokerPongWatchdog);
        brokerPongWatchdog = null;
    }
}

function updateBrokerUI(status, message) {
    const dashStatus = document.getElementById('dash-status');

    if (status === 'connected') {
        dashStatus.innerHTML = `<span class="pulse-dot"></span>Online`;
    } else if (status === 'connecting') {
        dashStatus.innerHTML = `<span class="pulse-dot connecting"></span>Connecting`;
    } else {
        dashStatus.innerHTML = `<span class="pulse-dot stopped"></span>Offline`;
    }
}

// ============================================================
// Printers Management
// ============================================================

async function scanPrinters() {
    const btn = document.getElementById('btn-scan-printers');

    if (btn) {
        btn.disabled = true;
        btn.innerHTML = '<span class="spinner"></span> Scanning...';
    }

    try {
        printers = await invoke('list_printers');
        await renderPrinters();

        const settings = await invoke('get_settings');
        if ((!settings.selected_printers || settings.selected_printers.length === 0) && printers.length > 0) {
            settings.selected_printers = [printers[0].name];
            await invoke('save_settings', { settings });
            await renderPrinters();
        }

        // Update dashboard printers list
        updateDashboardPrinters(settings.selected_printers);
        initWebSocket(settings);

        // Update printers in broker if online — computer_name is REQUIRED, else
        // the broker regroups everything under "Unknown Computer".
        if (brokerSocket && brokerSocket.readyState === WebSocket.OPEN && currentBrokerClientId) {
            brokerSocket.send(JSON.stringify({
                type: 'register',
                client_id: currentBrokerClientId,
                computer_name: await getComputerName(),
                printers: printers
            }));
        }
    } catch (e) {
        showToast('Failed to scan printers: ' + e, 'error');
    } finally {
        if (btn) {
            btn.disabled = false;
            btn.innerHTML = '<span class="material-icons-outlined">search</span> Scan Printers';
        }
    }
}

async function renderPrinters() {
    const grid = document.getElementById('printers-grid');
    let selectedPrinters = [];

    try {
        const settings = await invoke('get_settings');
        selectedPrinters = settings.selected_printers || [];
    } catch (e) { /* ignore */ }

    if (printers.length === 0) {
        grid.innerHTML = `
            <div class="empty-state">
                <span class="empty-icon">🖨️</span>
                <p>No printers found. Check your connections and try again.</p>
            </div>
        `;
        return;
    }

    grid.innerHTML = printers.map(p => {
        const isSelected = selectedPrinters.includes(p.name);
        const status = p.status || 'Unknown';
        const w = p.page_width || 'Custom';
        const h = p.page_height || '';
        const sizeStr = h ? `${w} × ${h}` : w;

        return `
            <div class="printer-card ${isSelected ? 'selected' : ''}"
                 onclick="togglePrinter('${p.name.replace(/'/g, "\\'")}')"
                 data-printer="${p.name}">
                 <div class="printer-card-header">
                     <span class="printer-card-icon">🖨️</span>
                     ${p.is_default ? '<span class="printer-default-badge">⭐ Default</span>' : ''}
                 </div>
                 <div class="printer-name">${p.name}</div>
                 <div class="printer-details-grid">
                     <div class="printer-detail-item"><strong>ID:</strong> <span>${p.id || 'N/A'}</span></div>
                     <div class="printer-detail-item"><strong>Status:</strong> <span class="status-badge ${status.toLowerCase()}">${status}</span></div>
                     <div class="printer-detail-item"><strong>Paper Size:</strong> <span>${sizeStr}</span></div>
                 </div>
            </div>
        `;
    }).join('');
}

async function togglePrinter(printerName) {
    try {
        const settings = await invoke('get_settings');
        settings.selected_printers = settings.selected_printers || [];

        const index = settings.selected_printers.indexOf(printerName);
        if (index > -1) {
            // Prevent deselecting the last selected printer
            if (settings.selected_printers.length <= 1) {
                showToast('You must select at least one printer for the app to run!', 'warning');
                return;
            }
            settings.selected_printers.splice(index, 1);
            showToast(`Unselected printer: ${printerName}`, 'info');
        } else {
            settings.selected_printers.push(printerName);
            showToast(`Selected printer: ${printerName}`, 'success');
        }

        await invoke('save_settings', { settings });

        // Update DOM selected state
        const cards = document.querySelectorAll('.printer-card');
        cards.forEach(card => {
            const name = card.dataset.printer;
            if (settings.selected_printers.includes(name)) {
                card.classList.add('selected');
            } else {
                card.classList.remove('selected');
            }
        });

        updateDashboardPrinters(settings.selected_printers);
    } catch (e) {
        showToast('Failed to select printer: ' + e, 'error');
    }
}

function updateDashboardPrinters(selectedList) {
    const el = document.getElementById('dash-printer');
    if (selectedList && selectedList.length > 0) {
        el.textContent = selectedList.join(', ');
        el.style.fontSize = selectedList.length > 2 ? '11px' : '14px';
    } else {
        el.textContent = 'None selected';
        el.style.fontSize = '14px';
    }
}

// ============================================================
// Logs
// ============================================================

async function loadLogs() {
    const list = document.getElementById('logs-list');
    if (!list) return;

    try {
        const logs = await invoke('get_print_logs');
        if (logs.length === 0) {
            list.innerHTML = `
                <div class="empty-state">
                    <span class="material-icons-outlined empty-icon">history</span>
                    <p>No print logs available.</p>
                </div>
            `;
            return;
        }

        // Render logs in reverse order
        list.innerHTML = logs.slice().reverse().map(log => {
            const statusClass = log.status === 'success' ? 'success' : 'error';
            const icon = log.status === 'success' ? 'check_circle' : 'error_outline';
            return `
                <div class="log-item">
                    <span class="material-icons-outlined log-status-icon ${statusClass}">${icon}</span>
                    <div class="log-info">
                        <div class="log-title">${log.order_number}</div>
                        <div class="log-meta">
                            <span>🕒 ${log.printed_at}</span>
                            <span>🖨️ ${log.printer_name}</span>
                        </div>
                        <div class="log-message">${log.message}</div>
                    </div>
                </div>
            `;
        }).join('');
    } catch (e) {
        console.error('Failed to load logs:', e);
    }
}

document.getElementById('btn-clear-logs').addEventListener('click', async () => {
    try {
        await invoke('clear_print_logs');
        showToast('Print logs cleared', 'success');
        loadLogs();
    } catch (e) {
        showToast('Failed to clear logs: ' + e, 'error');
    }
});

// ============================================================
// Activity feed helper
// ============================================================

function addActivity(type, message, time) {
    const list = document.getElementById('recent-activity-list');
    if (!list) return;

    const emptyState = list.querySelector('.empty-state');
    if (emptyState) {
        list.innerHTML = '';
    }

    const icons = {
        print: '🖨️',
        success: '✅',
        error: '❌',
        info: 'ℹ️'
    };

    const item = document.createElement('div');
    item.className = 'activity-item';
    item.innerHTML = `
        <span class="activity-icon">${icons[type] || 'ℹ️'}</span>
        <div class="activity-info">
            <p class="activity-text">${message}</p>
            <span class="activity-time">${time}</span>
        </div>
    `;

    list.insertBefore(item, list.firstChild);

    while (list.children.length > 20) {
        list.lastChild.remove();
    }
}

// ============================================================
// Init Event Listeners & Boot
// ============================================================

async function setupEventListeners() {
    // Printers Page Buttons
    document.getElementById('btn-scan-printers').addEventListener('click', scanPrinters);

    // Dashboard Quick Actions
    document.getElementById('btn-quick-printers').addEventListener('click', () => {
        navigateTo('printer');
        scanPrinters();
    });

    document.getElementById('btn-quick-settings').addEventListener('click', () => {
        navigateTo('settings');
    });

    document.getElementById('btn-open-web-dashboard').addEventListener('click', async () => {
        try {
            const settings = await invoke('get_settings');
            const clientId = settings.broker_client_id;
            const token = settings.socket_token;

            if (clientId && token) {
                const ssoUrl = `https://theautoprint.com/?yp_sso_user=${encodeURIComponent(clientId)}&yp_sso_token=${encodeURIComponent(token)}`;
                await invoke('open_url', { url: ssoUrl });
            } else {
                showToast('Please sign in first.', 'warning');
            }
        } catch (e) {
            showToast('Failed to open dashboard: ' + e, 'error');
        }
    });

    // Login Action
    document.getElementById('btn-login').addEventListener('click', async () => {
        const btn = document.getElementById('btn-login');
        const usernameInput = document.getElementById('login-username');
        const passwordInput = document.getElementById('login-password');

        const username = usernameInput.value.trim();
        const password = passwordInput.value;

        if (!username || !password) {
            showToast('Please enter both username/email and password.', 'warning');
            return;
        }

        btn.disabled = true;
        btn.innerHTML = '<span class="spinner"></span> Signing In...';

        try {
            const response = await invoke('login_user', {
                payload: { username, password }
            });

            showToast(`Welcome, ${response.user_display_name}!`, 'success');

            // Save client ID and socket token to settings
            const settings = await invoke('get_settings');
            settings.broker_client_id = response.client_id;
            settings.socket_token = response.socket_token;

            await invoke('save_settings', { settings });

            // Reload Settings to show dashboard & connect websocket
            await loadSettings();
            navigateTo('dashboard');

            // Reset input fields
            usernameInput.value = '';
            passwordInput.value = '';

        } catch (e) {
            showToast(e.toString(), 'error');
        } finally {
            btn.disabled = false;
            btn.innerHTML = 'Sign In';
        }
    });

    // Listen for SSO login success event from Rust background server
    listen('sso_login_success', async (event) => {
        const { client_id, socket_token } = event.payload;
        console.log('SSO login success event received:', client_id);

        showToast(`Welcome, ${client_id}!`, 'success');

        // Save to settings
        const settings = await invoke('get_settings');
        settings.broker_client_id = client_id;
        settings.socket_token = socket_token;
        await invoke('save_settings', { settings });

        // Reload settings to show dashboard & connect socket
        await loadSettings();
        navigateTo('dashboard');

        // Restore Google button state
        const googleBtn = document.getElementById('btn-google-login');
        if (googleBtn) {
            googleBtn.disabled = false;
            googleBtn.innerHTML = `
                <svg width="18" height="18" viewBox="0 0 24 24"><path fill="#EA4335" d="M12.24 10.285V14.4h6.887c-.648 2.41-2.519 4.114-5.136 4.114A5.99 5.99 0 0 1 7.996 12.5a5.99 5.99 0 0 1 5.995-6.014c1.642 0 3.124.664 4.21 1.74l3.184-3.183C19.347 3.03 16.85 1.874 13.99 1.874a10.618 10.618 0 0 0-10.6 10.627 10.617 10.617 0 0 0 10.6 10.626c5.848 0 9.715-4.053 9.715-9.887 0-.585-.053-1.157-.156-1.705H12.24Z"/></svg>
                Sign In with Google
            `;
        }

        // Restore Passkey button state
        const passkeyBtn = document.getElementById('btn-passkey-login');
        if (passkeyBtn) {
            passkeyBtn.disabled = false;
            passkeyBtn.innerHTML = `
                <span class="material-icons-outlined" style="font-size: 18px; color: #475569;">fingerprint</span>
                Sign In with Passkey
            `;
        }
    });

    // Google Login Action
    document.getElementById('btn-google-login').addEventListener('click', async () => {
        const btn = document.getElementById('btn-google-login');
        const originalHtml = btn.innerHTML;
        btn.innerHTML = '<span class="spinner"></span> Opening Browser...';

        // Auto-restore button text after 2 seconds so they can retry if needed
        setTimeout(() => {
            btn.innerHTML = originalHtml;
        }, 2000);

        try {
            const port = await invoke('start_sso_server');
            console.log('Local SSO server started for Google login on port:', port);

            const loginUrl = `https://theautoprint.com/wp-json/yeeprint-auth/v1/google-login?yp_port=${port}`;
            await invoke('open_url', { url: loginUrl });
            showToast('Opening default browser for Google Sign In...', 'info');
        } catch (e) {
            showToast('Failed to start Google login: ' + e, 'error');
            btn.innerHTML = originalHtml;
        }
    });

    // Passkey Login Action
    document.getElementById('btn-passkey-login').addEventListener('click', async () => {
        const btn = document.getElementById('btn-passkey-login');
        const originalHtml = btn.innerHTML;
        btn.innerHTML = '<span class="spinner"></span> Opening Browser...';

        // Auto-restore button text after 2 seconds so they can retry if needed
        setTimeout(() => {
            btn.innerHTML = originalHtml;
        }, 2000);

        try {
            const port = await invoke('start_sso_server');
            console.log('📡 Local SSO server started for Passkey login on port:', port);

            const loginUrl = `https://theautoprint.com/wp-json/yeeprint-auth/v1/passkey-login?yp_port=${port}`;
            await invoke('open_url', { url: loginUrl });
            showToast('Opening default browser for Passkey Sign In...', 'info');
        } catch (e) {
            showToast('Failed to start Passkey login: ' + e, 'error');
            btn.innerHTML = originalHtml;
        }
    });

    // Logout Action
    document.getElementById('btn-logout').addEventListener('click', async () => {
        try {
            // Clear credentials
            const settings = await invoke('get_settings');
            settings.broker_client_id = '';
            settings.socket_token = '';
            await invoke('save_settings', { settings });

            showToast('Signed out successfully.', 'info');

            // Reload settings to show login screen
            await loadSettings();
        } catch (e) {
            showToast('Failed to sign out: ' + e, 'error');
        }
    });

    // Login Enter Keydown & Footer Links Actions
    const loginUser = document.getElementById('login-username');
    const loginPass = document.getElementById('login-password');
    const loginBtn = document.getElementById('btn-login');

    if (loginUser) {
        loginUser.addEventListener('keydown', (e) => {
            if (e.key === 'Enter') {
                e.preventDefault();
                if (loginPass) loginPass.focus();
            }
        });
    }

    if (loginPass) {
        loginPass.addEventListener('keydown', (e) => {
            if (e.key === 'Enter') {
                e.preventDefault();
                if (loginBtn) loginBtn.click();
            }
        });
    }

    const forgotLink = document.getElementById('link-forgot-password');
    if (forgotLink) {
        forgotLink.addEventListener('click', async (e) => {
            e.preventDefault();
            try {
                await invoke('open_url', { url: 'https://theautoprint.com/wp-login.php?action=lostpassword' });
            } catch (err) {
                console.error('Failed to open forgot password link:', err);
            }
        });
    }

    const registerLink = document.getElementById('link-register');
    if (registerLink) {
        registerLink.addEventListener('click', async (e) => {
            e.preventDefault();
            try {
                await invoke('open_url', { url: 'https://theautoprint.com/api/' });
            } catch (err) {
                console.error('Failed to open register link:', err);
            }
        });
    }
}

async function ensureDefaultPrinterSelected() {
    try {
        const settings = await invoke('get_settings');
        if ((!settings.selected_printers || settings.selected_printers.length === 0) && printers.length > 0) {
            settings.selected_printers = [printers[0].name];
            await invoke('save_settings', { settings });
            showToast(`Default printer selected: ${printers[0].name}`, 'info');
            await renderPrinters();
            updateDashboardPrinters(settings.selected_printers);
            initWebSocket(settings);
        }
    } catch (e) {
        console.error('Failed to ensure default printer selection:', e);
    }
}

async function init() {
    await setupEventListeners();
    await loadSettings();
    await loadLogs();

    // Fetch and display computer name
    try {
        let name = await invoke('get_computer_name');
        if (name.toLowerCase().endsWith('.local')) {
            name = name.slice(0, -6);
        }
        const computerLabel = document.getElementById('computer-name-label');
        if (computerLabel) {
            computerLabel.textContent = `💻 ${name}`;
        }
    } catch (e) {
        console.error('Failed to load computer name:', e);
    }

    // Check if this is the first run to default autostart to ON
    const hasRunBefore = localStorage.getItem('has_run_before');
    if (!hasRunBefore) {
        try {
            await invoke('plugin:autostart|enable');
            document.getElementById('autostart-toggle').checked = true;
        } catch (e) {
            console.error('Failed to enable default autostart:', e);
        }
        localStorage.setItem('has_run_before', 'true');
    } else {
        try {
            const isAutostart = await invoke('plugin:autostart|is_enabled');
            document.getElementById('autostart-toggle').checked = isAutostart;
        } catch (e) {
            console.error(e);
        }
    }

    // Perform initial printer scan
    try {
        printers = await invoke('list_printers');
        await renderPrinters();
        await ensureDefaultPrinterSelected();
    } catch (e) {
        console.error('Initial printer scan failed:', e);
    }

    console.log('🖨️ YeePrint Client initialized');
}

if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
} else {
    init();
}
