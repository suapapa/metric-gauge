빌드된 바이너리에서 SRAM 사용량을 집계해 표로 정리합니다.

현재 release 바이너리 기준입니다. (ESP32-C3 온칩 **SRAM 400 KiB**, 그중 ~16 KiB는 ICACHE)

## 칩 / 링커 창

| 구역 | 크기 |
|------|------|
| 온칩 SRAM 전체 | **400 KiB** |
| ICACHE | ~16 KiB |
| DRAM + dram2 창 | **~378 KiB** |
| IRAM용으로 예약(`.rwdata_dummy`와 동일 물리) | **~39 KiB** |

## 섹션별 (데이터 버스 DRAM)

| 섹션 | 크기 | 역할 |
|------|------|------|
| `.data` + `.data.wifi` | **~7.9 KiB** | 초기화 데이터 |
| `.bss` | **~197.6 KiB** | FB, HTTP 슬롯, Wi‑Fi BSS 등 |
| `.stack` | **~68.3 KiB** | 링커 스택 예약 |
| `.dram2_uninit` | **56.0 KiB** | `esp_alloc` heap (Wi‑Fi 동적 버퍼) |
| **합(더미 제외)** | **~330 KiB** | |

## `.bss` 안 큰 항목

| 항목 | 크기 |
|------|------|
| `FrameBuffer` 240×240 RGB565 | **~112.5 KiB** |
| HTTP keep-alive `SLOTS` ×2 (TCP+TLS 버퍼) | **~59.1 KiB** |
| embassy main task `POOL` | **~9.4 KiB** |
| Wi‑Fi `g_cnxMgr` 등 | **~수 KiB** |
| `StackResources` StaticCell | **~3.1 KiB** |

## IRAM (명령 버스, 같은 SRAM)

| 항목 | 크기 |
|------|------|
| `.rwtext.wifi` | **~33.0 KiB** |
| `.rwtext` + `.trap` | **~6.2 KiB** |
| **IRAM 합** | **~39 KiB** |

## 한눈에

| | |
|--|--|
| 앱이 크게 쓰는 곳 | FB **112** + HTTP×2 **59** + heap **56** + stack 예약 **68** |
| 여유 | dram2 heap 뒤 **~9 KiB**, 그 외는 거의 찬 편 |

물리적으로는 IRAM(~39)과 DRAM 데이터(~330)가 **같은 400 KiB SRAM**을 나눠 씁니다.
