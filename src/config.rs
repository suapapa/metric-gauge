//! Compile-time configuration from environment variables.

pub const SSID: &str = env!("SSID");
pub const PASS: &str = env!("PASS");

/// Prometheus scrape URL for display 1 (typo `GUAGE` kept for ENV compatibility).
pub const GAUGE1_URL: &str = env!("GUAGE1_PROM_METRIC");
/// Prometheus scrape URL for display 2.
pub const GAUGE2_URL: &str = env!("GUAGE2_PROM_METRIC");

/// Short host label derived from a metrics URL (e.g. `node1` from `https://node1.homin.dev/metrics`).
pub fn host_label(url: &str) -> &str {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    host.split('.').next().unwrap_or(host)
}
