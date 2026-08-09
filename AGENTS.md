# AGENTS.md — metric-gauge agent guide

Orientation for automated agents (and humans) working on this firmware.

## Project summary

**metric-gauge** is a `no_std` Embassy / esp-hal firmware for **ESP32-C3 Super Mini** that:

1. Joins Wi-Fi using compile-time `SSID` / `PASS`
2. HTTPS-GETs two Prometheus endpoints (`GAUGE1_PROM_METRIC`, `GAUGE2_PROM_METRIC`)
3. Parses CPU % + MEM % (node-exporter or mon64 export)
4. Draws a 240×240 dual-arc circle gauge on each of two **GC9A01** SPI LCDs

Visual / layout reference: `_ref/` (Rust host renderer + Lottie; **not** linked into firmware) and `../mon64/internal/badge/circle240.go`. Firmware uses a lightweight full-framebuffer renderer instead of Lottie/TTF.

User-facing pin map and flash steps: `README.md`.

## Tech stack

| Layer | Choice |
|-------|--------|
| Target | `riscv32imc-unknown-none-elf`, ESP32-C3 |
| HAL / RTOS | `esp-hal` ~1.1, `esp-rtos` 0.3, `esp-radio` 0.18 |
| Async | Embassy (`embassy-executor`, `embassy-net`, `embassy-time`) |
| Display | `gc9a01-rs` 0.4 + shared SPI via `embedded-hal-bus` `RefCellDevice` |
| Graphics | Custom RGB565 full-framebuffer renderer + `embedded-graphics` mono fonts |
| HTTPS | Manual HTTP/1.1 + `embedded-tls` 0.19 (`default-features = false`, `NoVerify` / `UnsecureProvider`) |
| Logging | `esp-println` + `log` |

Do **not** pull in `reqwless` against current `embassy-net` 0.9 without aligning `embedded-nal-async` versions (0.8 vs 0.9 mismatch). Prefer the existing `src/http.rs` path.

## Directory layout

```
src/bin/main.rs   Wi-Fi, dual GC9A01 init, scrape/render loop
src/lib.rs        Module exports
src/config.rs     env!("…") constants + host_label()
src/http.rs       DNS + TCP + TLS + streaming HTTP GET
src/metrics.rs    Streaming Prometheus line parser + CPU delta history
src/render.rs     Full 240×240 circle-gauge drawing (RGB565)
build.rs          Linker helpers + rustc-env from SSID/PASS/GAUGE*
_ref/             Host-side reference (gitignored) — Lottie/fontdue; do not embed on device
assets/           SUIT TTF copies (too large for firmware; unused by device build)
.cargo/config.toml  Target + espflash runner
```

## Key design decisions

1. **SPI, not I²C**: GC9A01 is 4-wire SPI. Pin labels SDA/SCL on cheap modules mean MOSI/SCK.
2. **Shared SPI bus**: One `Spi` in a `RefCell`, two `RefCellDevice`s with separate CS (+ separate DC pins). Shared RST/BL.
3. **RAM**: One shared full 240×240×2 framebuffer (~112 KiB, `FrameBuffer` / `render_gauge`) in `.bss`; both LCDs reuse it (sequential paint). Two full buffers (~225 KiB) do not leave enough headroom with Wi-Fi heap.
4. **Large HTTPS bodies**: node-exporter `/metrics` can be ~100–200 KiB. Never buffer the whole body; stream into `MetricsParser` (line accumulator ≤256 B).
5. **CPU % (node-exporter)**: Sum all `node_cpu_seconds_total` and idle-mode samples; usage = `100 * (1 - Δidle/Δtotal)` between scrapes. First scrape → CPU `None` / UI `n/a`.
6. **mon64 export**: If `mon64_node_cpu_percent` / `mon64_node_mem_used_percent` appear, prefer those (no delta).
7. **TLS**: Certificate verification off (`UnsecureProvider`). Call `TlsConfig::enable_rsa_signatures()` so RSA server certs (e.g. Let's Encrypt) negotiate; default ClientHello is ECDSA/Ed25519-only and causes `HandshakeFailure`. TLS RX buffer must be **16_640** (max ciphertext record). Prefer ALPN `http/1.1` and HTTP/1.0 requests (avoids chunked bodies). Use `FlushPolicy::Relaxed` (Strict ACK-wait breaks full-duplex HTTPS on smoltcp). Disable TCP idle timeout (embassy-net maps it to `Io(ConnectionReset)`). Abort+flush sockets after each scrape; DNS is retried/cached. TLS/TCP static buffers are reused only from the main task (no concurrent scrapes).
8. **Configuration**: Configured via `GAUGE1_PROM_METRIC`, `GAUGE2_PROM_METRIC`, `GAUGE1_ROTATION`, `GAUGE2_ROTATION`, `GAUGE1_NAME`, and `GAUGE2_NAME`. `GAUGE1_NAME` / `GAUGE2_NAME` are used to specify the gauge names instead of extracting from URLs.
9. **Heap**: `esp_alloc` reclaimed region ~56 KiB — do not inflate casually; link may fail with `dram2_uninit` overflow.
10. **Fonts**: Device uses `FONT_6X10` / `FONT_10X20`, not SUIT TTF (flash/RAM). Glow text from `_ref` is omitted.

## Pin map (firmware source of truth)

See comments in `src/bin/main.rs` and `README.md`:

| Signal | GPIO |
|--------|------|
| SCK / MOSI | 6 / 7 |
| RST / BL | 0 / 5 |
| CS1 / DC1 | 10 / 1 |
| CS2 / DC2 | 3 / 4 |

## Commands

```bash
# Copy .env.sample to .env and edit it to set your credentials/metrics endpoints
cp .env.sample .env

# Check / build
cargo check --release

cargo build --release

# Flash + serial monitor (default runner)
cargo run --release
```

Clippy: crate denies `clippy::large_stack_frames` and `clippy::mem_forget` in `main.rs` — keep large buffers in `static` / `StaticCell`, not on task stacks.

## Safe change guidelines

- Prefer small, focused diffs; match existing module split (`http` / `metrics` / `render`).
- Changing pins: update `main.rs`, `README.md`, and this file together.
- Adding scrape fields: extend `MetricsParser` only; keep streaming.
- Display work: keep a single full framebuffer; avoid a second 240×240×2 buffer alongside Wi-Fi heap.
- Do not enable `embedded-tls` default features (`std` / `tokio`).
- Do not commit secrets; env vars are compile-time only and must not be hardcoded in source.

## Known limitations / good follow-ups

- TLS verify disabled; no client certs.
- Scrape interval fixed at 10 s in `main`.
- Mono fonts only; no SUIT Heavy / glow.
- Full-framebuffer redraw still recomputes the whole gauge each scrape; caching static layers is an optional optimization.
- Dual-display SPI is blocking; no DMA yet.
- `_ref` Lottie path is host-only; do not try to run `rasterlottie` on-device.

## Related docs

- ESP32-C3 Super Mini pins: https://lastminuteengineers.com/esp32-c3-super-mini-pinout-reference/
- `gc9a01-rs`: https://docs.rs/gc9a01-rs/latest/gc9a01/
- mon64 Prometheus shape: `../mon64/internal/export/prometheus/export.go`
