//! Minimal HTTPS GET with streaming body (embassy-net + embedded-tls).
//!
//! Two scrape slots keep TCP/TLS connections alive across the scrape loop when
//! the peer speaks HTTP/1.1 keep-alive (Content-Length or chunked). Static
//! buffers; main-task only (no concurrent scrapes).

use crate::metrics::{CpuHistory, GaugeStats, MetricsParser};
use core::mem::MaybeUninit;
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
const TLS_TX: usize = 4096;
const TCP_BUF: usize = 4096;
const ALPN_HTTP11: &[&[u8]] = &[b"http/1.1"];
const SLOT_COUNT: usize = 2;

type TlsConn = TlsConnection<'static, TcpSocket<'static>, Aes128GcmSha256>;

struct SessionBuffers {
    tcp_rx: [u8; TCP_BUF],
    tcp_tx: [u8; TCP_BUF],
    tls_rx: [u8; TLS_RECORD_MAX],
    tls_tx: [u8; TLS_TX],
}

impl SessionBuffers {
    const fn new() -> Self {
        Self {
            tcp_rx: [0; TCP_BUF],
            tcp_tx: [0; TCP_BUF],
            tls_rx: [0; TLS_RECORD_MAX],
            tls_tx: [0; TLS_TX],
        }
    }
}

enum LiveConn {
    Tls(TlsConn),
    Plain(TcpSocket<'static>),
}

struct Slot {
    bufs: SessionBuffers,
    host: heapless::String<64>,
    use_tls: bool,
    conn: Option<LiveConn>,
}

impl Slot {
    const fn new() -> Self {
        Self {
            bufs: SessionBuffers::new(),
            host: heapless::String::new(),
            use_tls: false,
            conn: None,
        }
    }
}

struct Slots {
    slots: [Slot; SLOT_COUNT],
}

impl Slots {
    const fn new() -> Self {
        Self {
            slots: [Slot::new(), Slot::new()],
        }
    }
}

// SAFETY: touched only from the main Embassy task.
static mut SLOTS: MaybeUninit<Slots> = MaybeUninit::uninit();
static mut SLOTS_READY: bool = false;

fn slots_mut() -> &'static mut Slots {
    unsafe {
        let p = core::ptr::addr_of_mut!(SLOTS);
        if !*core::ptr::addr_of!(SLOTS_READY) {
            (*p).write(Slots::new());
            *core::ptr::addr_of_mut!(SLOTS_READY) = true;
        }
        (*p).assume_init_mut()
    }
}

struct HttpResult {
    stats: GaugeStats,
    /// Peer left the connection usable for another request.
    keep_alive: bool,
}

/// Fetch Prometheus metrics. `slot` is `0` or `1` (one keep-alive connection each).
pub async fn fetch_prometheus(
    stack: Stack<'static>,
    slot: usize,
    tls_seed: u64,
    url: &str,
    history: &mut CpuHistory,
) -> GaugeStats {
    if url.is_empty() {
        return GaugeStats::default();
    }
    let slot = slot.min(SLOT_COUNT - 1);

    let Some((host, path, use_tls)) = parse_url(url) else {
        println!("bad url: {url}");
        return unreachable_stats();
    };

    if let Some(stats) = try_reuse(slot, host, use_tls, path, history).await {
        return stats;
    }

    connect_and_get(stack, slot, tls_seed, host, path, use_tls, history).await
}

async fn try_reuse(
    slot: usize,
    host: &str,
    use_tls: bool,
    path: &str,
    history: &mut CpuHistory,
) -> Option<GaugeStats> {
    let can_reuse = {
        let s = &slots_mut().slots[slot];
        s.conn.is_some() && s.host.as_str() == host && s.use_tls == use_tls
    };
    if !can_reuse {
        return None;
    }

    println!("reuse slot{slot} {host}");
    let result = {
        let conn = slots_mut().slots[slot].conn.as_mut().unwrap();
        match conn {
            LiveConn::Tls(tls) => {
                with_timeout(
                    Duration::from_secs(45),
                    stream_http_get(tls, host, path, history),
                )
                .await
            }
            LiveConn::Plain(sock) => {
                with_timeout(
                    Duration::from_secs(45),
                    stream_http_get(sock, host, path, history),
                )
                .await
            }
        }
    };

    match result {
        Ok(HttpResult {
            stats,
            keep_alive: true,
        }) if stats.reachable => Some(stats),
        Ok(HttpResult { stats, keep_alive }) => {
            println!(
                "reuse drop slot{slot} reachable={} keepalive={keep_alive}",
                stats.reachable
            );
            drop_conn(slot).await;
            None
        }
        Err(_) => {
            println!("reuse http timeout slot{slot} {host}");
            drop_conn(slot).await;
            None
        }
    }
}

async fn connect_and_get(
    stack: Stack<'static>,
    slot: usize,
    tls_seed: u64,
    host: &str,
    path: &str,
    use_tls: bool,
    history: &mut CpuHistory,
) -> GaugeStats {
    drop_conn(slot).await;

    let port: u16 = if use_tls { 443 } else { 80 };
    let Some(ip) = resolve_host(stack, host).await else {
        println!("dns fail: {host}");
        return unreachable_stats();
    };
    println!("fetch {host} -> {ip}");

    // SAFETY: conn is None; we exclusively use this slot's buffers.
    let (tcp_rx, tcp_tx) = unsafe {
        let bufs = &mut slots_mut().slots[slot].bufs;
        (
            &mut *core::ptr::addr_of_mut!(bufs.tcp_rx),
            &mut *core::ptr::addr_of_mut!(bufs.tcp_tx),
        )
    };

    let mut socket = TcpSocket::new(stack, tcp_rx, tcp_tx);
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

    // Buffers live in static SLOTS for the program lifetime.
    let socket: TcpSocket<'static> = unsafe { core::mem::transmute(socket) };

    if use_tls {
        let (tls_rx, tls_tx) = unsafe {
            let bufs = &mut slots_mut().slots[slot].bufs;
            (
                &mut *core::ptr::addr_of_mut!(bufs.tls_rx),
                &mut *core::ptr::addr_of_mut!(bufs.tls_tx),
            )
        };
        let mut tls = TlsConnection::new(socket, tls_rx, tls_tx);
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

        let mut tls: TlsConn = unsafe { core::mem::transmute(tls) };
        let result = with_timeout(
            Duration::from_secs(45),
            stream_http_get(&mut tls, host, path, history),
        )
        .await;

        match result {
            Ok(HttpResult {
                stats,
                keep_alive: true,
            }) if stats.reachable => {
                remember(slot, host, true, LiveConn::Tls(tls));
                stats
            }
            Ok(HttpResult { stats, .. }) => {
                abort_tls(tls).await;
                Timer::after(Duration::from_millis(50)).await;
                if stats.reachable {
                    stats
                } else {
                    unreachable_stats()
                }
            }
            Err(_) => {
                println!("http timeout {host}");
                abort_tls(tls).await;
                Timer::after(Duration::from_millis(50)).await;
                unreachable_stats()
            }
        }
    } else {
        let mut socket = socket;
        let result = with_timeout(
            Duration::from_secs(45),
            stream_http_get(&mut socket, host, path, history),
        )
        .await;

        match result {
            Ok(HttpResult {
                stats,
                keep_alive: true,
            }) if stats.reachable => {
                remember(slot, host, false, LiveConn::Plain(socket));
                stats
            }
            Ok(HttpResult { stats, .. }) => {
                socket.abort();
                let _ = socket.flush().await;
                Timer::after(Duration::from_millis(50)).await;
                if stats.reachable {
                    stats
                } else {
                    unreachable_stats()
                }
            }
            Err(_) => {
                println!("http timeout {host}");
                socket.abort();
                let _ = socket.flush().await;
                Timer::after(Duration::from_millis(50)).await;
                unreachable_stats()
            }
        }
    }
}

fn remember(slot: usize, host: &str, use_tls: bool, conn: LiveConn) {
    let s = &mut slots_mut().slots[slot];
    s.host.clear();
    let _ = s.host.push_str(host);
    s.use_tls = use_tls;
    s.conn = Some(conn);
    println!("keepalive slot{slot} {host} tls={use_tls}");
}

async fn drop_conn(slot: usize) {
    let conn = slots_mut().slots[slot].conn.take();
    slots_mut().slots[slot].host.clear();
    if let Some(conn) = conn {
        match conn {
            LiveConn::Tls(tls) => abort_tls(tls).await,
            LiveConn::Plain(mut sock) => {
                sock.abort();
                let _ = sock.flush().await;
            }
        }
        Timer::after(Duration::from_millis(50)).await;
    }
}

async fn abort_tls(tls: TlsConn) {
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
) -> HttpResult
where
    S: Read + Write,
{
    let mut req: heapless::String<288> = heapless::String::new();
    let _ = req.push_str("GET ");
    let _ = req.push_str(path);
    let _ = req.push_str(" HTTP/1.1\r\nHost: ");
    let _ = req.push_str(host);
    let _ = req.push_str(
        "\r\nUser-Agent: metric-gauge\r\nAccept: text/plain\r\nConnection: keep-alive\r\n\r\n",
    );

    if let Err(e) = stream.write_all(req.as_bytes()).await {
        println!("http write: {e:?}");
        return HttpResult {
            stats: unreachable_stats(),
            keep_alive: false,
        };
    }
    if let Err(e) = stream.flush().await {
        println!("http flush: {e:?}");
        return HttpResult {
            stats: unreachable_stats(),
            keep_alive: false,
        };
    }
    println!("http req sent {host}");

    match read_http_response(stream, history).await {
        Ok((stats, keep_alive)) => HttpResult { stats, keep_alive },
        Err(()) => HttpResult {
            stats: unreachable_stats(),
            keep_alive: false,
        },
    }
}

enum BodyKind {
    Length(usize),
    Chunked,
    UntilEof,
}

async fn read_http_response<S: Read>(
    stream: &mut S,
    history: &mut CpuHistory,
) -> Result<(GaugeStats, bool), ()> {
    let mut buf = [0u8; 512];
    let mut header: heapless::Vec<u8, 1536> = heapless::Vec::new();
    let mut got = 0usize;

    let header_end = loop {
        match stream.read(&mut buf).await {
            Ok(0) => {
                println!("http eof during headers ({got}B)");
                return Err(());
            }
            Ok(n) => {
                got += n;
                for &b in &buf[..n] {
                    if header.push(b).is_err() {
                        println!("http headers too long");
                        return Err(());
                    }
                }
                if let Some(pos) = find_header_end(&header) {
                    break pos;
                }
            }
            Err(e) => {
                println!("http read hdr: {e:?} (got {got}B)");
                return Err(());
            }
        }
    };

    let (head, preamble) = header.split_at(header_end);
    if !parse_status_ok(head) {
        println!("http bad status");
        return Err(());
    }
    let peer_close = header_has_connection_close(head);
    let body_kind = parse_body_kind(head, peer_close);

    let mut parser = MetricsParser::new();
    let mut keep_alive = !peer_close;

    match body_kind {
        BodyKind::Length(len) => {
            let mut remaining = len;
            let take = preamble.len().min(remaining);
            parser.push(&preamble[..take]);
            remaining -= take;
            while remaining > 0 {
                match stream.read(&mut buf).await {
                    Ok(0) => {
                        println!("http eof mid-body ({got}B, left {remaining})");
                        return Err(());
                    }
                    Ok(n) => {
                        got += n;
                        let use_n = n.min(remaining);
                        parser.push(&buf[..use_n]);
                        remaining -= use_n;
                        if use_n < n {
                            // Extra bytes would be pipelined next response — not expected.
                            println!("http trailing {}B after body", n - use_n);
                            keep_alive = false;
                            break;
                        }
                    }
                    Err(e) => {
                        println!("http read body: {e:?}");
                        return Err(());
                    }
                }
            }
        }
        BodyKind::Chunked => {
            let mut stash: heapless::Vec<u8, 1024> = heapless::Vec::new();
            for &b in preamble {
                let _ = stash.push(b);
            }
            if !read_chunked(stream, &mut stash, &mut buf, &mut parser, &mut got).await {
                return Err(());
            }
        }
        BodyKind::UntilEof => {
            parser.push(preamble);
            keep_alive = false;
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        got += n;
                        parser.push(&buf[..n]);
                    }
                    Err(e) => {
                        println!("http read eof-body: {e:?} (got {got}B)");
                        break;
                    }
                }
            }
        }
    }

    println!("http ok ({got}B) keepalive={keep_alive}");
    Ok((parser.finish(history), keep_alive && !peer_close))
}

fn stash_pop_front(stash: &mut heapless::Vec<u8, 1024>) -> Option<u8> {
    if stash.is_empty() {
        return None;
    }
    let b = stash[0];
    let len = stash.len();
    for i in 1..len {
        stash[i - 1] = stash[i];
    }
    let _ = stash.pop();
    Some(b)
}

fn stash_pop_front_n(stash: &mut heapless::Vec<u8, 1024>, n: usize) {
    let n = n.min(stash.len());
    let len = stash.len();
    for i in n..len {
        stash[i - n] = stash[i];
    }
    for _ in 0..n {
        let _ = stash.pop();
    }
}

async fn read_chunked<S: Read>(
    stream: &mut S,
    stash: &mut heapless::Vec<u8, 1024>,
    buf: &mut [u8; 512],
    parser: &mut MetricsParser,
    got: &mut usize,
) -> bool {
    loop {
        let size = match read_chunk_size(stream, stash, buf, got).await {
            Some(s) => s,
            None => return false,
        };
        if size == 0 {
            return consume_until_blank(stream, stash, buf, got).await;
        }
        let mut left = size;
        while left > 0 {
            if !stash.is_empty() {
                let n = left.min(stash.len());
                parser.push(&stash[..n]);
                stash_pop_front_n(stash, n);
                left -= n;
                continue;
            }
            match stream.read(buf).await {
                Ok(0) => {
                    println!("http chunk eof");
                    return false;
                }
                Ok(n) => {
                    *got += n;
                    let use_n = n.min(left);
                    parser.push(&buf[..use_n]);
                    left -= use_n;
                    if use_n < n {
                        for &b in &buf[use_n..n] {
                            if stash.push(b).is_err() {
                                println!("http chunk stash overflow");
                                return false;
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("http chunk read: {e:?}");
                    return false;
                }
            }
        }
        if !expect_crlf(stream, stash, buf, got).await {
            return false;
        }
    }
}

async fn read_chunk_size<S: Read>(
    stream: &mut S,
    stash: &mut heapless::Vec<u8, 1024>,
    buf: &mut [u8; 512],
    got: &mut usize,
) -> Option<usize> {
    let mut line: heapless::Vec<u8, 64> = heapless::Vec::new();
    loop {
        while let Some(b) = stash_pop_front(stash) {
            if b == b'\n' {
                if line.last() == Some(&b'\r') {
                    let _ = line.pop();
                }
                return parse_hex_size(&line);
            }
            if line.push(b).is_err() {
                println!("http chunk size line long");
                return None;
            }
        }
        match stream.read(buf).await {
            Ok(0) => return None,
            Ok(n) => {
                *got += n;
                for &b in &buf[..n] {
                    if stash.push(b).is_err() {
                        return None;
                    }
                }
            }
            Err(_) => return None,
        }
    }
}

async fn expect_crlf<S: Read>(
    stream: &mut S,
    stash: &mut heapless::Vec<u8, 1024>,
    buf: &mut [u8; 512],
    got: &mut usize,
) -> bool {
    let mut seen = 0u8;
    loop {
        while let Some(b) = stash_pop_front(stash) {
            match (seen, b) {
                (0, b'\r') => seen = 1,
                (1, b'\n') => return true,
                _ => {
                    println!("http chunk crlf mismatch");
                    return false;
                }
            }
        }
        match stream.read(buf).await {
            Ok(0) => return false,
            Ok(n) => {
                *got += n;
                for &b in &buf[..n] {
                    if stash.push(b).is_err() {
                        return false;
                    }
                }
            }
            Err(_) => return false,
        }
    }
}

async fn consume_until_blank<S: Read>(
    stream: &mut S,
    stash: &mut heapless::Vec<u8, 1024>,
    buf: &mut [u8; 512],
    got: &mut usize,
) -> bool {
    // After final chunk: optional trailers ending with CRLF CRLF. Often just CRLF.
    let mut prev = 0u8;
    let mut count = 0u8;
    loop {
        let b = loop {
            if let Some(b) = stash_pop_front(stash) {
                break b;
            }
            match stream.read(buf).await {
                Ok(0) => return count >= 1,
                Ok(n) => {
                    *got += n;
                    for &x in &buf[..n] {
                        if stash.push(x).is_err() {
                            return false;
                        }
                    }
                }
                Err(_) => return false,
            }
        };
        if prev == b'\r' && b == b'\n' {
            count += 1;
            if count >= 2 {
                return true;
            }
            // Single CRLF after last chunk (no trailers) is enough.
            if count == 1 {
                return true;
            }
        } else if b != b'\r' {
            count = 0;
        }
        prev = b;
    }
}

fn parse_hex_size(line: &[u8]) -> Option<usize> {
    let hex = line.split(|&b| b == b';').next().unwrap_or(line);
    let Ok(s) = core::str::from_utf8(hex) else {
        return None;
    };
    let s = s.trim();
    usize::from_str_radix(s, 16).ok()
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

fn ascii_contains_ci(hay: &str, needle: &[u8]) -> bool {
    hay.as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

fn header_has_connection_close(headers: &[u8]) -> bool {
    let Ok(s) = core::str::from_utf8(headers) else {
        return false;
    };
    for line in s.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("connection") {
                return ascii_contains_ci(v, b"close");
            }
        }
    }
    false
}

fn parse_body_kind(headers: &[u8], peer_close: bool) -> BodyKind {
    let Ok(s) = core::str::from_utf8(headers) else {
        return BodyKind::UntilEof;
    };
    let mut length = None;
    let mut chunked = false;
    for line in s.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                if let Ok(n) = v.trim().parse::<usize>() {
                    length = Some(n);
                }
            } else if k.eq_ignore_ascii_case("transfer-encoding")
                && ascii_contains_ci(v, b"chunked")
            {
                chunked = true;
            }
        }
    }
    if chunked {
        BodyKind::Chunked
    } else if let Some(n) = length {
        BodyKind::Length(n)
    } else if peer_close {
        BodyKind::UntilEof
    } else {
        BodyKind::UntilEof
    }
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

    struct Entry {
        host: heapless::String<64>,
        ip: IpAddress,
    }
    static mut CACHE: [Option<Entry>; 2] = [None, None];
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

    for e in cache.iter() {
        if let Some(e) = e {
            println!("dns fallback cache {} -> {}", e.host, e.ip);
            return Some(e.ip);
        }
    }
    None
}
