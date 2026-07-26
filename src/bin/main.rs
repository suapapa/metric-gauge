#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::cell::RefCell;

use embassy_executor::Spawner;
use embassy_net::{DhcpConfig, Runner, Stack, StackResources};
use embassy_time::{Delay, Duration, Timer};
use embedded_hal_bus::spi::RefCellDevice;
use metric_gauge::{
    config::{self, GAUGE1_URL, GAUGE2_URL, PASS, SSID},
    http::fetch_prometheus,
    metrics::CpuHistory,
    render::{BandBuffer, render_gauge_bands},
};
use esp_hal::{
    clock::CpuClock,
    delay::Delay as BlockingDelay,
    gpio::{Level, Output, OutputConfig},
    interrupt::software::SoftwareInterruptControl,
    rng::Rng,
    spi::{
        Mode,
        master::{Config as SpiConfig, Spi},
    },
    time::Rate,
    timer::timg::TimerGroup,
};
use esp_println::println;
use esp_radio::wifi::{Config as WifiConfig, WifiController, sta::StationConfig};
use gc9a01::{
    Gc9a01, SPIDisplayInterface, display::DisplayResolution240x240, mode::DisplayConfiguration,
};
use static_cell::StaticCell;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {info}");
    loop {}
}

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

macro_rules! mk_static {
    ($t:ty, $val:expr) => {{
        static STATIC_CELL: StaticCell<$t> = StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

// ESP32-C3 Super Mini + dual GC9A01 (4-wire SPI).
// Round modules often label MOSI/SCK as SDA/SCL — still SPI, not I²C.
//
// Shared: SCK=GPIO6, MOSI=GPIO7, RST=GPIO0, BL=GPIO5
// Gauge1: CS=GPIO10, DC=GPIO1
// Gauge2: CS=GPIO3,  DC=GPIO4

struct RotationInterface<DI> {
    inner: DI,
    madctl: u8,
    intercept_madctl: bool,
}

impl<DI> RotationInterface<DI> {
    const fn new(inner: DI, madctl: u8) -> Self {
        Self {
            inner,
            madctl,
            intercept_madctl: false,
        }
    }
}

impl<DI: display_interface::WriteOnlyDataCommand> display_interface::WriteOnlyDataCommand
    for RotationInterface<DI>
{
    fn send_commands(
        &mut self,
        cmd: display_interface::DataFormat<'_>,
    ) -> Result<(), display_interface::DisplayError> {
        if let display_interface::DataFormat::U8(slice) = cmd {
            if !slice.is_empty() && slice[0] == 0x36 {
                if slice.len() > 1 {
                    let mut modified = [0u8; 16];
                    let len = slice.len().min(16);
                    modified[..len].copy_from_slice(&slice[..len]);
                    modified[1] = self.madctl;
                    return self
                        .inner
                        .send_commands(display_interface::DataFormat::U8(&modified[..len]));
                } else {
                    self.intercept_madctl = true;
                }
            }
            self.inner
                .send_commands(display_interface::DataFormat::U8(slice))
        } else {
            self.inner.send_commands(cmd)
        }
    }

    fn send_data(
        &mut self,
        buf: display_interface::DataFormat<'_>,
    ) -> Result<(), display_interface::DisplayError> {
        if self.intercept_madctl {
            self.intercept_madctl = false;
            if let display_interface::DataFormat::U8(slice) = buf {
                if !slice.is_empty() {
                    let mut modified = [0u8; 16];
                    let len = slice.len().min(16);
                    modified[..len].copy_from_slice(&slice[..len]);
                    modified[0] = self.madctl;
                    return self
                        .inner
                        .send_data(display_interface::DataFormat::U8(&modified[..len]));
                }
                return self
                    .inner
                    .send_data(display_interface::DataFormat::U8(slice));
            }
        }
        self.inner.send_data(buf)
    }
}

#[allow(
    clippy::large_stack_frames,
    reason = "display state and scrape loop live in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // C3 max is 160 MHz; 80 MHz cuts idle heat a lot (TLS scrape still OK).
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz);
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 56 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    esp_println::logger::init_logger_from_env();
    println!("metric-gauge boot");
    println!("SSID len={} PASS len={}", SSID.len(), PASS.len());
    println!("gauge1={GAUGE1_URL}");
    println!("gauge2={GAUGE2_URL}");
    println!("gauge1 rotation={:?}", config::GAUGE1_ROTATION);
    println!("gauge2 rotation={:?}", config::GAUGE2_ROTATION);

    // --- Displays ---
    let mut rst = Output::new(peripherals.GPIO0, Level::High, OutputConfig::default());
    let mut bl = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default());
    let _ = bl.set_high();

    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(64)) //40
            .with_mode(Mode::_0),
    )
    .expect("SPI2 init")
    .with_sck(peripherals.GPIO6)
    .with_mosi(peripherals.GPIO7);

    let spi_bus = RefCell::new(spi);
    let spi_dev1 = RefCellDevice::new(
        &spi_bus,
        Output::new(peripherals.GPIO10, Level::High, OutputConfig::default()),
        Delay,
    );
    let spi_dev2 = RefCellDevice::new(
        &spi_bus,
        Output::new(peripherals.GPIO3, Level::High, OutputConfig::default()),
        Delay,
    );

    let iface1 = RotationInterface::new(
        SPIDisplayInterface::new(
            spi_dev1,
            Output::new(peripherals.GPIO1, Level::Low, OutputConfig::default()),
        ),
        config::GAUGE1_MADCTL,
    );
    let iface2 = RotationInterface::new(
        SPIDisplayInterface::new(
            spi_dev2,
            Output::new(peripherals.GPIO4, Level::Low, OutputConfig::default()),
        ),
        config::GAUGE2_MADCTL,
    );

    let mut display1 = Gc9a01::new(iface1, DisplayResolution240x240, config::GAUGE1_ROTATION);
    let mut display2 = Gc9a01::new(iface2, DisplayResolution240x240, config::GAUGE2_ROTATION);

    let mut blocking_delay = BlockingDelay::new();
    let _ = display1.reset(&mut rst, &mut blocking_delay);
    let _ = display1.init(&mut blocking_delay);
    let _ = display2.init(&mut blocking_delay);
    println!("displays ready");

    static BAND: StaticCell<BandBuffer> = StaticCell::new();
    let band = BAND.init_with(BandBuffer::new);
    paint_gauge(&mut display1, band, Some(0.0), Some(0.0), "boot", true);
    paint_gauge(&mut display2, band, Some(0.0), Some(0.0), "boot", true);

    // --- Wi-Fi ---
    let rng = Rng::new();
    let (wifi_controller, interfaces) =
        esp_radio::wifi::new(peripherals.WIFI, Default::default()).expect("Wi-Fi init");

    let wifi_interface = interfaces.station;
    let net_seed = u64::from(rng.random()) | (u64::from(rng.random()) << 32);
    let tls_seed = u64::from(rng.random()) | (u64::from(rng.random()) << 32);

    let (stack, runner) = embassy_net::new(
        wifi_interface,
        embassy_net::Config::dhcpv4(DhcpConfig::default()),
        mk_static!(StackResources<4>, StackResources::<4>::new()),
        net_seed,
    );

    spawner.spawn(connection(wifi_controller).unwrap());
    spawner.spawn(net_task(runner).unwrap());

    wait_for_connection(stack).await;
    println!("network up");

    let host1 = config::host_label(GAUGE1_URL);
    let host2 = config::host_label(GAUGE2_URL);
    let mut hist1 = CpuHistory::default();
    let mut hist2 = CpuHistory::default();

    let mut tls_seed = tls_seed;
    loop {
        let stats1 = fetch_prometheus(stack, tls_seed, GAUGE1_URL, &mut hist1).await;
        tls_seed = tls_seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let stats2 = fetch_prometheus(stack, tls_seed, GAUGE2_URL, &mut hist2).await;
        tls_seed = tls_seed.wrapping_add(0x9E37_79B9_7F4A_7C15);

        paint_gauge(
            &mut display1,
            band,
            stats1.cpu_percent,
            stats1.mem_percent,
            host1,
            stats1.reachable,
        );
        paint_gauge(
            &mut display2,
            band,
            stats2.cpu_percent,
            stats2.mem_percent,
            host2,
            stats2.reachable,
        );

        println!(
            "g1 cpu={:?} mem={:?} | g2 cpu={:?} mem={:?}",
            stats1.cpu_percent, stats1.mem_percent, stats2.cpu_percent, stats2.mem_percent
        );

        Timer::after(Duration::from_secs(10)).await;
    }
}

fn paint_gauge<I>(
    display: &mut Gc9a01<I, DisplayResolution240x240, gc9a01::mode::BasicMode>,
    band: &mut BandBuffer,
    cpu: Option<f32>,
    mem: Option<f32>,
    hostname: &str,
    reachable: bool,
) where
    I: display_interface::WriteOnlyDataCommand,
{
    render_gauge_bands(band, cpu, mem, hostname, reachable, |b| {
        let y0 = b.y0 as u16;
        let y1 = (b.y0 + b.height as i32 - 1) as u16;
        // MemoryWrite (0x2C) must precede pixels; draw_buffer skips it.
        let mut colors = b.row_slice().iter().copied();
        let _ = display.set_pixels((0, y0), (239, y1), &mut colors);
    });
}

async fn wait_for_connection(stack: Stack<'_>) {
    println!("waiting for link");
    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }
    println!("waiting for DHCP");
    loop {
        if let Some(config) = stack.config_v4() {
            println!("IP {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }
}

#[allow(clippy::large_stack_frames)]
#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    println!("wifi connection task");
    // ESP32-C3 Super Mini: default ~20 dBm often yields AuthenticationExpired
    // during WPA (poor onboard antenna match). 8.5 dBm is the usual fix.
    // Unit is 0.25 dBm → 34 ≈ 8.5 dBm. Must be set after start, before connect.
    const TX_POWER: i8 = 34;
    loop {
        if controller.is_connected() {
            let _ = controller.wait_for_disconnect_async().await;
            println!("wifi disconnected");
            // Full radio while reconnecting.
            let _ = controller.set_power_saving(esp_radio::wifi::PowerSaveMode::None);
            Timer::after(Duration::from_secs(3)).await;
        }

        let station_config = WifiConfig::Station(
            StationConfig::default()
                .with_ssid(SSID)
                .with_password(PASS.into()),
        );
        if let Err(e) = controller.set_config(&station_config) {
            println!("wifi set_config: {e:?}");
            Timer::after(Duration::from_secs(3)).await;
            continue;
        }

        // set_config starts the radio; TX must be lowered before auth.
        let _ = controller.set_power_saving(esp_radio::wifi::PowerSaveMode::None);
        if let Err(e) = controller.set_max_tx_power(TX_POWER) {
            println!("wifi set_max_tx_power: {e:?}");
        } else {
            println!("wifi tx power {TX_POWER}/0.25dBm (~8.5dBm)");
        }

        println!("wifi connecting…");
        match controller.connect_async().await {
            Ok(_) => {
                println!("wifi connected");
                // Keep modem awake: Minimum sleep regularly stalls TCP/TLS
                // handshakes to remote HTTPS (hangs after "fetch host -> ip").
                let _ = controller.set_power_saving(esp_radio::wifi::PowerSaveMode::None);
            }
            Err(e) => {
                println!("wifi connect failed: {e:?}");
                Timer::after(Duration::from_secs(5)).await;
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, esp_radio::wifi::Interface<'static>>) -> ! {
    runner.run().await
}
