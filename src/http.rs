//! Minimal HTTPS GET with streaming body (embassy-net + embedded-tls).

use embassy_net::{
    dns::DnsQueryType,
    tcp::TcpSocket,
    IpAddress, Stack,
};
use embedded_io_async::{Read, Write};
use embedded_tls::{
    Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext, UnsecureProvider,
};
use crate::metrics::{CpuHistory, GaugeStats, MetricsParser};
use esp_println::println;
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;

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
        return GaugeStats {
            reachable: false,
            ..Default::default()
        };
    };

    let port: u16 = if use_tls { 443 } else { 80 };

    let ip = match resolve_host(stack, host).await {
        Some(ip) => ip,
        None => {
            println!("dns fail: {host}");
            return GaugeStats {
                reachable: false,
                ..Default::default()
            };
        }
    };

    let mut rx_buf = [0u8; 4096];
    let mut tx_buf = [0u8; 4096];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    socket.set_timeout(Some(embassy_time::Duration::from_secs(15)));

    if let Err(e) = socket.connect((ip, port)).await {
        println!("tcp connect {host}:{port}: {e:?}");
        return GaugeStats {
            reachable: false,
            ..Default::default()
        };
    }

    if use_tls {
        // SAFETY: fetch_prometheus is only awaited from the main task, never concurrently.
        static mut TLS_RX: [u8; 8192] = [0; 8192];
        static mut TLS_TX: [u8; 4096] = [0; 4096];
        let tls_rx = unsafe { &mut *core::ptr::addr_of_mut!(TLS_RX) };
        let tls_tx = unsafe { &mut *core::ptr::addr_of_mut!(TLS_TX) };

        let mut tls = TlsConnection::new(socket, tls_rx, tls_tx);
        let config = TlsConfig::new().with_server_name(host);
        let rng = ChaCha8Rng::seed_from_u64(tls_seed);
        if let Err(e) = tls
            .open(TlsContext::new(
                &config,
                UnsecureProvider::new::<Aes128GcmSha256>(rng),
            ))
            .await
        {
            println!("tls open: {e:?}");
            return GaugeStats {
                reachable: false,
                ..Default::default()
            };
        }

        stream_http_get(&mut tls, host, path, history).await
    } else {
        stream_http_get(&mut socket, host, path, history).await
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
    let mut req: heapless::String<256> = heapless::String::new();
    let _ = req.push_str("GET ");
    let _ = req.push_str(path);
    let _ = req.push_str(" HTTP/1.1\r\nHost: ");
    let _ = req.push_str(host);
    let _ = req.push_str("\r\nUser-Agent: esp-dual-gauge\r\nConnection: close\r\n\r\n");

    if let Err(e) = stream.write_all(req.as_bytes()).await {
        println!("http write: {e:?}");
        return GaugeStats {
            reachable: false,
            ..Default::default()
        };
    }
    let _ = stream.flush().await;

    let mut buf = [0u8; 512];
    let mut header = heapless::Vec::<u8, 1024>::new();
    let mut header_done = false;
    let mut parser = MetricsParser::new();

    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
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
                            return GaugeStats {
                                reachable: false,
                                ..Default::default()
                            };
                        }
                        parser.push(body);
                    }
                } else {
                    parser.push(chunk);
                }
            }
            Err(e) => {
                println!("http read: {e:?}");
                break;
            }
        }
    }

    if !header_done {
        println!("http truncated headers");
        return GaugeStats {
            reachable: false,
            ..Default::default()
        };
    }

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
    match stack.dns_query(host, DnsQueryType::A).await {
        Ok(addrs) if !addrs.is_empty() => Some(addrs[0]),
        Ok(_) => None,
        Err(e) => {
            println!("dns_query: {e:?}");
            None
        }
    }
}
