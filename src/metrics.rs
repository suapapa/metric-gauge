//! Streaming Prometheus text parser for node-exporter and mon64 exports.

/// Rolling CPU counters needed to compute usage between scrapes.
#[derive(Clone, Copy, Default)]
pub struct CpuHistory {
    pub idle: f32,
    pub total: f32,
    pub valid: bool,
}

/// Parsed gauge stats for one host.
#[derive(Clone, Copy, Default)]
pub struct GaugeStats {
    pub cpu_percent: Option<f32>,
    pub mem_percent: Option<f32>,
    pub reachable: bool,
}

/// Accumulator while streaming a Prometheus body.
pub struct MetricsParser {
    cpu_idle: f32,
    cpu_total: f32,
    cpu_samples: u32,
    mem_total: Option<f32>,
    mem_available: Option<f32>,
    /// mon64 pre-normalized percents (preferred when present).
    mon64_cpu: Option<f32>,
    mon64_mem: Option<f32>,
    mon64_reachable: Option<bool>,
    line: heapless::Vec<u8, 256>,
}

impl Default for MetricsParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsParser {
    pub fn new() -> Self {
        Self {
            cpu_idle: 0.0,
            cpu_total: 0.0,
            cpu_samples: 0,
            mem_total: None,
            mem_available: None,
            mon64_cpu: None,
            mon64_mem: None,
            mon64_reachable: None,
            line: heapless::Vec::new(),
        }
    }

    /// Feed a body chunk; completes when the HTTP body is fully consumed.
    pub fn push(&mut self, chunk: &[u8]) {
        for &b in chunk {
            if b == b'\n' {
                self.flush_line();
            } else if b != b'\r' {
                let _ = self.line.push(b);
            }
        }
    }

    pub fn finish(&mut self, history: &mut CpuHistory) -> GaugeStats {
        self.flush_line();

        if let (Some(cpu), Some(mem)) = (self.mon64_cpu, self.mon64_mem) {
            return GaugeStats {
                cpu_percent: Some(clamp_percent(cpu)),
                mem_percent: Some(clamp_percent(mem)),
                reachable: self.mon64_reachable.unwrap_or(true),
            };
        }

        let mem_percent = match (self.mem_total, self.mem_available) {
            (Some(total), Some(avail)) if total > 0.0 => {
                Some(clamp_percent((total - avail) / total * 100.0))
            }
            _ => None,
        };

        let cpu_percent = if self.cpu_samples > 0 {
            let idle = self.cpu_idle;
            let total = self.cpu_total;
            let usage = if history.valid {
                let di = idle - history.idle;
                let dt = total - history.total;
                if dt > 0.0 {
                    Some(clamp_percent(100.0 * (1.0 - di / dt)))
                } else {
                    None
                }
            } else {
                None
            };
            history.idle = idle;
            history.total = total;
            history.valid = true;
            usage
        } else {
            None
        };

        GaugeStats {
            cpu_percent,
            mem_percent,
            reachable: cpu_percent.is_some() || mem_percent.is_some(),
        }
    }

    fn flush_line(&mut self) {
        if self.line.is_empty() {
            return;
        }
        let mut buf = [0u8; 256];
        let len = self.line.len().min(buf.len());
        buf[..len].copy_from_slice(&self.line[..len]);
        self.line.clear();
        if let Ok(line) = core::str::from_utf8(&buf[..len]) {
            self.parse_line(line);
        }
    }

    fn parse_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return;
        }

        // mon64 normalized export
        if let Some(rest) = line.strip_prefix("mon64_node_cpu_percent") {
            if let Some(v) = parse_prom_value(rest) {
                self.mon64_cpu = Some(v);
            }
            return;
        }
        if let Some(rest) = line.strip_prefix("mon64_node_mem_used_percent") {
            if let Some(v) = parse_prom_value(rest) {
                self.mon64_mem = Some(v);
            }
            return;
        }
        if let Some(rest) = line.strip_prefix("mon64_node_reachable") {
            if let Some(v) = parse_prom_value(rest) {
                self.mon64_reachable = Some(v >= 0.5);
            }
            return;
        }

        // node-exporter CPU counters
        if let Some(rest) = line.strip_prefix("node_cpu_seconds_total") {
            if let Some(v) = parse_prom_value(rest) {
                self.cpu_total += v;
                self.cpu_samples += 1;
                if label_mode_is_idle(rest) {
                    self.cpu_idle += v;
                }
            }
            return;
        }

        if let Some(rest) = line.strip_prefix("node_memory_MemTotal_bytes") {
            // Avoid matching longer names that share the prefix.
            if rest.starts_with('{') || rest.starts_with(' ') || rest.starts_with('\t') {
                self.mem_total = parse_prom_value(rest);
            }
            return;
        }
        if let Some(rest) = line.strip_prefix("node_memory_MemAvailable_bytes") {
            if rest.starts_with('{') || rest.starts_with(' ') || rest.starts_with('\t') {
                self.mem_available = parse_prom_value(rest);
            }
        }
    }
}

/// Convenience: feed one chunk into an existing parser.
pub fn parse_prometheus_chunk(parser: &mut MetricsParser, chunk: &[u8]) {
    parser.push(chunk);
}

fn clamp_percent(v: f32) -> f32 {
    v.clamp(0.0, 100.0)
}

fn label_mode_is_idle(rest: &str) -> bool {
    // node_cpu_seconds_total{cpu="0",mode="idle"} 123
    rest.contains("mode=\"idle\"") || rest.contains("mode='idle'")
}

fn parse_prom_value(rest: &str) -> Option<f32> {
    // rest is after the metric name: `{labels} value` or ` value`
    let value_str = if let Some(idx) = rest.rfind(' ') {
        rest[idx + 1..].trim()
    } else {
        rest.trim()
    };
    if value_str.is_empty() {
        return None;
    }
    // Strip optional timestamp
    let value_str = value_str.split_whitespace().next()?;
    parse_float(value_str)
}

fn parse_float(s: &str) -> Option<f32> {
    // Handles `1.23`, `1.23e+09`, `NaN`, `+Inf`
    if s.eq_ignore_ascii_case("nan") || s.contains("Inf") {
        return None;
    }
    s.parse::<f32>().ok()
}
