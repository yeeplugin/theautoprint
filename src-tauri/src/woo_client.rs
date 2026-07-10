use crate::models::WooOrder;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::Client;
use std::sync::OnceLock;

/// Shared HTTP client — reuses TCP/TLS connections across requests
static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn get_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .pool_max_idle_per_host(2)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client")
    })
}

/// Build the WooCommerce API base URL
fn build_api_url(store_url: &str) -> String {
    let url = store_url.trim_end_matches('/');
    format!("{}/wp-json/wc/v3", url)
}

/// Build authorization header (Basic Auth)
fn build_auth_header(consumer_key: &str, consumer_secret: &str) -> String {
    let credentials = format!("{}:{}", consumer_key, consumer_secret);
    format!("Basic {}", BASE64.encode(credentials.as_bytes()))
}

/// Test connection to WooCommerce store
pub async fn test_connection(
    store_url: &str,
    consumer_key: &str,
    consumer_secret: &str,
) -> Result<String, String> {
    let client = get_client();
    let api_url = format!("{}/system_status", build_api_url(store_url));
    let auth = build_auth_header(consumer_key, consumer_secret);

    let response = client
        .get(&api_url)
        .header("Authorization", &auth)
        // Only fetch fields we need for connection test
        .query(&[("_fields", "environment")])
        .send()
        .await
        .map_err(|e| format!("Connection failed: {}", e))?;

    let status = response.status();
    if status.is_success() {
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let store_name = body["environment"]["site_url"]
            .as_str()
            .unwrap_or("Unknown");
        let wc_version = body["environment"]["version"].as_str().unwrap_or("Unknown");

        Ok(format!(
            "✅ Connection successful!\nStore: {}\nWooCommerce: v{}",
            store_name, wc_version
        ))
    } else if status.as_u16() == 401 {
        Err("❌ Invalid Consumer Key or Consumer Secret".to_string())
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("❌ Error {}: {}", status.as_u16(), error_text))
    }
}

/// Fetch recent orders from WooCommerce (full data, used for UI display)
pub async fn fetch_orders(
    store_url: &str,
    consumer_key: &str,
    consumer_secret: &str,
    per_page: u32,
    status_filter: &[String],
) -> Result<Vec<WooOrder>, String> {
    let client = get_client();
    let api_url = build_api_url(store_url);
    let auth = build_auth_header(consumer_key, consumer_secret);

    let mut url = format!(
        "{}/orders?per_page={}&orderby=id&order=desc",
        api_url, per_page
    );

    // Add status filter
    if !status_filter.is_empty() {
        let statuses = status_filter.join(",");
        url = format!("{}&status={}", url, statuses);
    }

    let response = client
        .get(&url)
        .header("Authorization", &auth)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch orders: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("API Error: {}", error_text));
    }

    let orders: Vec<WooOrder> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse orders: {}", e))?;

    Ok(orders)
}

/// Fetch new orders since a given order ID, using date-based filtering
/// to minimize server load. Uses `after` param so WooCommerce can
/// optimize its database query, and limits `per_page` to 10.
pub async fn fetch_new_orders(
    store_url: &str,
    consumer_key: &str,
    consumer_secret: &str,
    last_order_id: u64,
    status_filter: &[String],
    last_checked_at: Option<&str>,
) -> Result<Vec<WooOrder>, String> {
    let client = get_client();
    let api_url = build_api_url(store_url);
    let auth = build_auth_header(consumer_key, consumer_secret);

    // Use a smaller per_page for polling — we only need truly new orders
    let mut url = format!(
        "{}/orders?per_page=10&orderby=id&order=desc",
        api_url
    );

    // Add status filter
    if !status_filter.is_empty() {
        let statuses = status_filter.join(",");
        url = format!("{}&status={}", url, statuses);
    }

    // Use date-based filtering to reduce server query scope
    // This tells WooCommerce to only return orders created after this timestamp
    if let Some(after) = last_checked_at {
        // Sanitize timestamp: WooCommerce expects YYYY-MM-DDTHH:MM:SS format
        let sanitized = if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(after) {
            dt.naive_utc().format("%Y-%m-%dT%H:%M:%S").to_string()
        } else if let Ok(dt) = chrono::DateTime::parse_from_str(after, "%Y-%m-%dT%H:%M:%S%:z") {
            dt.naive_utc().format("%Y-%m-%dT%H:%M:%S").to_string()
        } else if after.len() >= 19 {
            // Simple truncation format YYYY-MM-DDTHH:MM:SS
            after[0..19].to_string()
        } else {
            after.to_string()
        };
        
        url = format!("{}&after={}", url, sanitized);
    }

    let response = client
        .get(&url)
        .header("Authorization", &auth)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch orders: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("API Error: {}", error_text));
    }

    let orders: Vec<WooOrder> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse orders: {}", e))?;

    // Double-check with order ID filter (belt and suspenders)
    let new_orders: Vec<WooOrder> = orders
        .into_iter()
        .filter(|o| o.id > last_order_id)
        .collect();

    Ok(new_orders)
}

/// Download a PDF file from a given URL using the shared HTTP client
pub async fn download_pdf(
    url: &str,
    consumer_key: &str,
    consumer_secret: &str,
) -> Result<Vec<u8>, String> {
    let client = get_client();
    let auth = build_auth_header(consumer_key, consumer_secret);
    let response = client
        .get(url)
        .header("Authorization", &auth)
        .send()
        .await
        .map_err(|e| format!("Failed to download PDF: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        return Err(format!("Download failed with status code {}", status));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read PDF bytes: {}", e))?;

    Ok(bytes.to_vec())
}

