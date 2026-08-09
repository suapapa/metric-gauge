#![no_std]

pub mod config;
pub mod http;
pub mod metrics;
pub mod render;

pub use config::{GAUGE1_URL, GAUGE2_URL, PASS, SSID};
pub use metrics::{CpuHistory, GaugeStats, parse_prometheus_chunk};
pub use render::{FrameBuffer, SIZE, render_gauge};
