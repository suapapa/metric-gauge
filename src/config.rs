//! Compile-time configuration from environment variables.

use gc9a01::rotation::DisplayRotation;

pub const SSID: &str = env!("SSID");
pub const PASS: &str = env!("PASS");

/// Prometheus scrape URL for display 1 (typo `GUAGE` kept for ENV compatibility).
pub const GAUGE1_URL: &str = env!("GUAGE1_PROM_METRIC");
/// Prometheus scrape URL for display 2.
pub const GAUGE2_URL: &str = env!("GUAGE2_PROM_METRIC");

/// Display 1 rotation parsed at compile-time.
pub const GAUGE1_ROTATION: DisplayRotation = parse_rotation_config(env!("GUAGE1_ROTATION")).0;
/// Display 1 MADCTL register value parsed at compile-time.
pub const GAUGE1_MADCTL: u8 = parse_rotation_config(env!("GUAGE1_ROTATION")).1;

/// Display 2 rotation parsed at compile-time.
pub const GAUGE2_ROTATION: DisplayRotation = parse_rotation_config(env!("GUAGE2_ROTATION")).0;
/// Display 2 MADCTL register value parsed at compile-time.
pub const GAUGE2_MADCTL: u8 = parse_rotation_config(env!("GUAGE2_ROTATION")).1;

const fn parse_hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

const fn parse_rotation_config(val: &str) -> (DisplayRotation, u8) {
    let bytes = val.as_bytes();
    
    // Check if it's a hex override like 0x18 or 0x68
    if bytes.len() >= 3 && bytes[0] == b'0' && (bytes[1] == b'x' || bytes[1] == b'X') {
        let mut result = 0u8;
        let mut idx = 2;
        while idx < bytes.len() {
            if let Some(digit) = parse_hex_digit(bytes[idx]) {
                result = (result << 4) | digit;
            } else {
                break;
            }
            idx += 1;
        }
        let rot = match result {
            0x78 | 0x68 | 0x28 | 0x38 => DisplayRotation::Rotate90,
            0xD8 | 0xC8 => DisplayRotation::Rotate180,
            0xB8 | 0xA8 | 0x88 | 0x98 => DisplayRotation::Rotate270,
            _ => DisplayRotation::Rotate0,
        };
        return (rot, result);
    }
    
    match bytes {
        b"90" | b"Rotate90" => (DisplayRotation::Rotate90, 0x78),
        b"180" | b"Rotate180" => (DisplayRotation::Rotate180, 0xD8),
        b"270" | b"Rotate270" => (DisplayRotation::Rotate270, 0xB8),
        _ => (DisplayRotation::Rotate0, 0x18),
    }
}

/// Short host label derived from a metrics URL (e.g. `node1` from `https://node1.homin.dev/metrics`).
pub fn host_label(url: &str) -> &str {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    host.split('.').next().unwrap_or(host)
}
