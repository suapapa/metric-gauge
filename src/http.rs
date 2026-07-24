//! Minimal HTTPS GET with streaming body (embassy-net + embedded-tls).

use crate::metrics::{CpuHistory, GaugeStats, MetricsParser};
use embassy_net::{
    dns::DnsQueryType,
    tcp::TcpSocket,
    IpAddress, Stack,
};
use embassy_time::{with_timeout, Duration, Timer};
use embedded_io_async::{Read, Write};
use embedded_tls::{
    Aes128GcmSha256, FlushPolicy, TlsConfig, TlsConnection, TlsContext, UnsecureProvider,
};
use esp_println::println;
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;

/// Max TLS ciphertext record is 16_640 bytes (embedded-tls requirement).
const TLS_RECORD_MAX: usize = 16_640;
const TCP_BUF: usize = 4096;
const ALPN_HTTP11: &[&[u8]] = &[b"http/1.1"];

/// Fetch Prometheus text from `url` and parse CPU/MEM stats.
pub async fn fetch_prometheus(
    stack: Stack<'_>,
    tls_seed: u64,
    url: &str,
    history: &mut CpuHistory,
) -> GaugeStats {
    if url.is_empty() {
        return GaugeStats::default();
    }

    let Some((host, path, use_tls)) = parse_url(url) else {
        println!("bad url: {url}");
        return unreachable_stats();
    };

    let port: u16 = if use_tls { 443 } else { 80 };

    let ip = match resolve_host(stack, host).await {
        Some(ip) => ip,
        None => {
            println!("dns fail: {host}");
            return unreachable_stats();
        }
    };
    println!("fetch {host} -> {ip}");

    // SAFETY: only awaited from the main task, never concurrently.
    static mut TCP_RX: [u8; TCP_BUF] = [0; TCP_BUF];
    static mut TCP_TX: [u8; TCP_BUF] = [0; TCP_BUF];
    let tcp_rx = unsafe { &mut *core::ptr::addr_of_mut!(TCP_RX) };
    let tcp_tx = unsafe { &mut *core::ptr::addr_of_mut!(TCP_TX) };

    let mut socket = TcpSocket::new(stack, tcp_rx, tcp_tx);
    // Idle timeout is reported as Io(ConnectionReset) by embassy-net — disable it.
    // Overall connect/TLS/HTTP deadlines use with_timeout instead.
    socket.set_timeout(None);
    socket.set_keep_alive(None);

    println!("tcp connecting {host}:{port}");
    match with_timeout(Duration::from_secs(15), socket.connect((ip, port))).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            println!("tcp connect {host}:{port}: {e:?}");
            return unreachable_stats();
        }
        Err(_) => {
            println!("tcp connect timeout {host}:{port}");
            socket.abort();
            let _ = socket.flush().await;
            return unreachable_stats();
        }
    }
    println!("tcp ok {host}:{port}");

    let stats = if use_tls {
        static mut TLS_RX: [u8; TLS_RECORD_MAX] = [0; TLS_RECORD_MAX];
        static mut TLS_TX: [u8; 4096] = [0; 4096];
        let tls_rx = unsafe { &mut *core::ptr::addr_of_mut!(TLS_RX) };
        let tls_tx = unsafe { &mut *core::ptr::addr_of_mut!(TLS_TX) };

        let mut tls = TlsConnection::new(socket, tls_rx, tls_tx);
        // Don't block waiting for ACKs between records — needed for full-duplex HTTPS.
        tls.set_flush_policy(FlushPolicy::Relaxed);

        let config = TlsConfig::new()
            .enable_rsa_signatures()
            .with_server_name(host)
            .with_alpn(ALPN_HTTP11);
        let rng = ChaCha8Rng::seed_from_u64(tls_seed);
        println!("tls opening {host}");
        match with_timeout(
            Duration::from_secs(20),
            tls.open(TlsContext::new(
                &config,
                UnsecureProvider::new::<Aes128GcmSha256>(rng),
            )),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                println!("tls open: {e:?}");
                abort_tls(tls).await;
                return unreachable_stats();
            }
            Err(_) => {
                println!("tls open timeout {host}");
                abort_tls(tls).await;
                return unreachable_stats();
            }
        }
        println!("tls ok {host}");

        let stats = match with_timeout(
            Duration::from_secs(45),
            stream_http_get(&mut tls, host, path, history),
        )
        .await
        {
            Ok(s) => s,
            Err(_) => {
                println!("http timeout {host}");
                unreachable_stats()
            }
        };
        abort_tls(tls).await;
        // Brief pause so FIN/RST can leave before the next scrape reuses buffers.
        Timer::after(Duration::from_millis(50)).await;
        stats
    } else {
        let stats = match with_timeout(
            Duration::from_secs(45),
            stream_http_get(&mut socket, host, path, history),
        )
        .await
        {
            Ok(s) => s,
            Err(_) => {
                println!("http timeout {host}");
                unreachable_stats()
            }
        };
        socket.abort();
        let _ = socket.flush().await;
        Timer::after(Duration::from_millis(50)).await;
        stats
    };

    stats
}

async fn abort_tls(
    tls: TlsConnection<'_, TcpSocket<'_>, Aes128GcmSha256>,
) {
    match tls.close().await {
        Ok(mut socket) => {
            socket.abort();
            let _ = socket.flush().await;
        }
        Err((mut socket, e)) => {
            println!("tls close: {e:?}");
            socket.abort();
            let _ = socket.flush().await;
        }
    }
}

fn unreachable_stats() -> GaugeStats {
    GaugeStats {
        reachable: false,
        ..Default::default()
    }
}

async fn stream_http_get<S>(
    stream: &mut S,
    host: &str,
    path: &str,
    history: &mut CpuHistory,
) -> GaugeStats
where
    S: Read + Write,
{
    // HTTP/1.0 avoids chunked encoding on many reverse proxies; body ends on close.
    let mut req: heapless::String<256> = heapless::String::new();
    let _ = req.push_str("GET ");
    let _ = req.push_str(path);
    let _ = req.push_str(" HTTP/1.0\r\nHost: ");
    let _ = req.push_str(host);
    let _ = req.push_str("\r\nUser-Agent: esp-dual-gauge\r\nConnection: close\r\n\r\n");

    if let Err(e) = stream.write_all(req.as_bytes()).await {
        println!("http write: {e:?}");
        return unreachable_stats();
    }
    if let Err(e) = stream.flush().await {
        println!("http flush: {e:?}");
        return unreachable_stats();
    }
    println!("http req sent {host}");

    let mut buf = [0u8; 512];
    let mut header = heapless::Vec::<u8, 1024>::new();
    let mut header_done = false;
    let mut got = 0usize;
    let mut parser = MetricsParser::new();

    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                got += n;
                let chunk = &buf[..n];
                if !header_done {
                    for &b in chunk {
                        let _ = header.push(b);
                    }
                    if let Some(pos) = find_header_end(&header) {
                        header_done = true;
                        let (head, body) = header.split_at(pos);
                        if !parse_status_ok(head) {
                            println!("http bad status");
                            return unreachable_stats();
                        }
                        parser.push(body);
                    }
                } else {
                    parser.push(chunk);
                }
            }
            Err(e) => {
                println!("http read: {e:?} (got {got}B, hdr={header_done})");
                break;
            }
        }
    }

    if !header_done {
        println!("http truncated headers ({got}B)");
        return unreachable_stats();
    }

    println!("http ok {host} ({got}B)");
    parser.finish(history)
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
}

fn parse_status_ok(headers: &[u8]) -> bool {
    let Ok(s) = core::str::from_utf8(headers) else {
        return false;
    };
    let line = s.lines().next().unwrap_or("");
    line.contains("HTTP/1.1 200") || line.contains("HTTP/1.0 200")
}

fn parse_url(url: &str) -> Option<(&str, &str, bool)> {
    let (use_tls, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return None;
    };
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        return None;
    }
    Some((host, path, use_tls))
}

async fn resolve_host(stack: Stack<'_>, host: &str) -> Option<IpAddress> {
    if let Ok(v4) = host.parse::<core::net::Ipv4Addr>() {
        return Some(IpAddress::Ipv4(embassy_net::Ipv4Address::from(v4.octets())));
    }

    // Cache last few resolutions — node1/node2 often share an A record, and DNS
    // can fail after a messy TCP teardown.
    struct Entry {
        host: heapless::String<64>,
        ip: IpAddress,
    }
    static mut CACHE: [Option<Entry>; 2] = [None, None];
    // SAFETY: only called from the main task.
    let cache = unsafe { &mut *core::ptr::addr_of_mut!(CACHE) };
    for e in cache.iter() {
        if let Some(e) = e {
            if e.host.as_str() == host {
                return Some(e.ip);
            }
        }
    }

    for attempt in 0..3u8 {
        match stack.dns_query(host, DnsQueryType::A).await {
            Ok(addrs) if !addrs.is_empty() => {
                let ip = addrs[0];
                let mut name = heapless::String::new();
                let _ = name.push_str(host);
                // Insert / replace in a free or oldest slot.
                if let Some(slot) = cache.iter_mut().find(|s| s.is_none()) {
                    *slot = Some(Entry { host: name, ip });
                } else {
                    cache[0] = Some(Entry { host: name, ip });
                }
                return Some(ip);
            }
            Ok(_) => println!("dns empty: {host} (try {attempt})"),
            Err(e) => println!("dns_query: {e:?} (try {attempt})"),
        }
        Timer::after(Duration::from_millis(200)).await;
    }

    // Last resort: reuse any cached IP (same LB for sibling hostnames).
    for e in cache.iter() {
        if let Some(e) = e {
            println!("dns fallback cache {} -> {}", e.host, e.ip);
            return Some(e.ip);
        }
    }
    None
}
