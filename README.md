# metric-gauge

ESP32-C3 Super Mini + dual **GC9A01** round LCDs (240×240). Each display shows a circle gauge with **CPU** and **MEM** from a Prometheus metrics URL (node-exporter or mon64 export).

Metric source sibling project: [mon64](https://github.com/suapapa/mon64).

## Hardware

| Part | Notes |
|------|--------|
| MCU | ESP32-C3 Super Mini |
| Displays | 2× GC9A01 1.28″ round TFT, **4-wire SPI** |

> GC9A01 modules often label MOSI/SCK as **SDA/SCL**. That is still SPI, not I²C.

### Pin map

| Signal | GPIO | Notes |
|--------|------|--------|
| SCK (SCL) | **6** | Shared SPI clock |
| MOSI (SDA) | **7** | Shared SPI data |
| RST | **0** | Shared reset |
| BL | **5** | Shared backlight (driven high) |
| CS1 | **10** | Gauge 1 chip select |
| DC1 | **1** | Gauge 1 data/command |
| CS2 | **3** | Gauge 2 chip select |
| DC2 | **4** | Gauge 2 data/command |

Avoid strapping pins **2 / 8 / 9** for CS/DC if possible. UART0 (**20/21**) is left free for USB serial.

Power both modules from **3V3** (logic is 3.3 V). Do not feed 5 V into GPIO.

## Build / flash

Requires a Rust nightly toolchain with `riscv32imc-unknown-none-elf` (see `rust-toolchain.toml`) and [`espflash`](https://github.com/esp-rs/espflash).

Compile-time env vars (required for a useful image):

| Env | Alias | Purpose |
|-----|--------|---------|
| `SSID` | — | Wi-Fi SSID |
| `PASS` | `PASSWORD` | Wi-Fi password |
| `GAUGE1_PROM_METRIC` | — | Prometheus URL for display 1 |
| `GAUGE2_PROM_METRIC` | — | Prometheus URL for display 2 |
| `GAUGE1_ROTATION` | — | Rotation for display 1 (`0`, `90`, `180`, `270` or `RotateX`) |
| `GAUGE2_ROTATION` | — | Rotation for display 2 (`0`, `90`, `180`, `270` or `RotateX`) |
| `GAUGE1_NAME` | — | Name of the gauge 1 (drawn on the display 1) |
| `GAUGE2_NAME` | — | Name of the gauge 2 (drawn on the display 2) |

Copy `.env.sample` to `.env` and edit it to fit your configuration:

```bash
cp .env.sample .env
# Edit the .env file to set your SSID, PASS, and metrics endpoints
```

Run and flash directly (uses the runner in `.cargo/config.toml` to call `espflash flash --monitor --chip esp32c3`):

```bash
cargo run --release
```

Build only:

```bash
cargo build --release
```

Flash an already-built binary:

```bash
espflash flash --monitor --chip esp32c3 \
  target/riscv32imc-unknown-none-elf/release/metric-gauge
```

### Metrics notes

- Scrapes every **10 s** over HTTPS (TLS cert verification disabled).
- Supports **node-exporter** (`node_cpu_seconds_total`, `node_memory_MemTotal_bytes` / `MemAvailable_bytes`) and **mon64** gauges (`mon64_node_cpu_percent`, `mon64_node_mem_used_percent`, …).
- Node-exporter CPU needs two samples: first scrape shows `n/a` for CPU, then deltas work.
- Hostname label on the gauge is configured via `GAUGE1_NAME` and `GAUGE2_NAME` (e.g. `node1`, `node2`).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
