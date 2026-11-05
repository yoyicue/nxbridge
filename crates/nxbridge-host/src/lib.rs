//! Local management prototype. GameStream reports are observations,
//! not managed sessions or proof that a game can be played.
mod catalog;
pub use catalog::{Catalog, CatalogCommand, CatalogStore, StoreError};
use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_ENDPOINTS: usize = 8;
const MAX_CONNECTIONS: usize = 16;
const MAX_REQUEST: usize = 8192;
const MAX_REPLY: u64 = 16 * 1024;
const TIMEOUT: Duration = Duration::from_millis(800);
const PAGE: &str = include_str!("../../../web/index.html");

#[derive(Clone, Debug)]
pub struct Endpoint {
    label: String,
    address: SocketAddrV4,
}

impl Endpoint {
    pub fn parse(value: &str) -> Result<Self, String> {
        let (label, address) = value
            .split_once('=')
            .ok_or("endpoint must be LABEL=IP:PORT")?;
        if label.trim().is_empty() || label.len() > 64 || label.chars().any(char::is_control) {
            return Err("endpoint label must be 1–64 bytes without control characters".into());
        }
        let address: SocketAddrV4 = address
            .parse()
            .map_err(|_| "endpoint requires an IPv4 address and port")?;
        if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_broadcast() {
            return Err(
                "endpoint address must name a local IPv4 interface and a nonzero port".into(),
            );
        }
        // Only observe a service bound to an interface owned by this Mac.
        UdpSocket::bind(SocketAddrV4::new(*address.ip(), 0))
            .map_err(|_| "endpoint IP is not a local interface")?;
        Ok(Self {
            label: label.into(),
            address,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Observation {
    Ready,
    Busy,
    Unreachable,
    Unknown,
}

pub fn observe(endpoint: &Endpoint) -> Observation {
    let addr = SocketAddr::V4(endpoint.address);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, TIMEOUT) else {
        return Observation::Unreachable;
    };
    if stream.set_read_timeout(Some(TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(TIMEOUT)).is_err()
    {
        return Observation::Unknown;
    }
    let req = format!("GET /serverinfo?uniqueid=nxbridge-local-observer HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", endpoint.address);
    if stream.write_all(req.as_bytes()).is_err() {
        return Observation::Unknown;
    }
    let mut response = Vec::new();
    let deadline = Instant::now() + TIMEOUT;
    let mut block = [0u8; 1024];
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Observation::Unknown;
        };
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return Observation::Unknown;
        }
        match stream.read(&mut block) {
            Ok(0) => break,
            Ok(n) => {
                response.extend_from_slice(&block[..n]);
                if response.len() as u64 > MAX_REPLY {
                    return Observation::Unknown;
                }
            }
            Err(_) => return Observation::Unknown,
        }
    }
    let Ok(text) = std::str::from_utf8(&response) else {
        return Observation::Unknown;
    };
    let Some((headers, body)) = text.split_once("\r\n\r\n") else {
        return Observation::Unknown;
    };
    if !(headers.starts_with("HTTP/1.1 200 ") || headers.starts_with("HTTP/1.0 200 "))
        || !body.contains("<root status_code=\"200\">")
    {
        return Observation::Unknown;
    }
    // Accept only fixed protocol values, never surface untrusted XML content.
    if body.matches("<state>").count() != 1 || body.matches("</state>").count() != 1 {
        return Observation::Unknown;
    }
    let Some((_, rest)) = body.split_once("<state>") else {
        return Observation::Unknown;
    };
    let Some((state, _)) = rest.split_once("</state>") else {
        return Observation::Unknown;
    };
    match state {
        "MOONSHINE_SERVER_FREE" => Observation::Ready,
        "MOONSHINE_SERVER_BUSY" => Observation::Busy,
        _ => Observation::Unknown,
    }
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0000}'..='\u{001f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn snapshot(endpoints: &[Endpoint]) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis().to_string())
        .unwrap_or_else(|_| "null".into());
    let mut items = Vec::new();
    for endpoint in endpoints.iter().take(MAX_ENDPOINTS) {
        let status = match observe(endpoint) {
            Observation::Ready => "ready",
            Observation::Busy => "busy",
            Observation::Unreachable => "unreachable",
            Observation::Unknown => "unknown",
        };
        items.push(format!(
            "{{\"label\":{},\"status\":\"{}\"}}",
            json_string(&endpoint.label),
            status
        ));
    }
    format!("{{\"api_version\":1,\"mode\":\"read_only_experiment\",\"sampled_at_unix_ms\":{},\"endpoints\":[{}],\"measured_session_capacity\":null,\"managed_sessions\":null,\"game_fps\":null,\"stream_fps\":null,\"input_latency_ms\":null}}", now, items.join(","))
}

fn response(stream: &mut TcpStream, code: &str, content_type: &str, body: &str) -> io::Result<()> {
    let header = format!("HTTP/1.1 {code}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'\r\n\r\n", body.len());
    stream.write_all(header.as_bytes())?;
    stream.write_all(body.as_bytes())
}

fn handle(
    mut stream: TcpStream,
    port: u16,
    endpoints: &[Endpoint],
    catalog: &CatalogStore,
) -> io::Result<()> {
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut request = Vec::new();
    let mut block = [0u8; 1024];
    let deadline = Instant::now() + TIMEOUT;
    while !request.windows(4).any(|v| v == b"\r\n\r\n") {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request deadline"))?;
        stream.set_read_timeout(Some(remaining))?;
        let n = stream.read(&mut block)?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&block[..n]);
        if request.len() > MAX_REQUEST {
            return response(
                &mut stream,
                "431 Request Header Fields Too Large",
                "text/plain; charset=utf-8",
                "Request too large",
            );
        }
    }
    let Ok(text) = std::str::from_utf8(&request) else {
        return response(
            &mut stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            "Invalid request",
        );
    };
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let (Some(method), Some(path), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return response(
            &mut stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            "Invalid request",
        );
    };
    if version != "HTTP/1.1" {
        return response(
            &mut stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            "HTTP/1.1 required",
        );
    }
    let hosts: Vec<_> = lines
        .take_while(|s| !s.is_empty())
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("host").then(|| value.trim())
        })
        .collect();
    let valid_hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if hosts.len() != 1 || !valid_hosts.iter().any(|h| hosts[0].eq_ignore_ascii_case(h)) {
        return response(
            &mut stream,
            "403 Forbidden",
            "text/plain; charset=utf-8",
            "Local host required",
        );
    }
    if method == "POST" && path == "/api/v1/games" {
        let header_end = request.windows(4).position(|v| v == b"\r\n\r\n").unwrap() + 4;
        let headers = text
            .split("\r\n")
            .skip(1)
            .take_while(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let get = |name: &str| -> Vec<&str> {
            headers
                .iter()
                .filter_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    k.eq_ignore_ascii_case(name).then(|| v.trim())
                })
                .collect()
        };
        let origins = get("origin");
        let valid_origin = origins.is_empty()
            || (origins.len() == 1
                && (origins[0] == format!("http://127.0.0.1:{port}")
                    || origins[0] == format!("http://localhost:{port}")));
        let token = get("x-nxbridge-request");
        let content_types = get("content-type");
        let lengths = get("content-length");
        if !valid_origin || token.len() != 1 || token[0] != "1" {
            return response(
                &mut stream,
                "403 Forbidden",
                "application/json; charset=utf-8",
                "{\"error\":\"origin_or_header\"}",
            );
        }
        if content_types.len() != 1
            || content_types[0] != "application/toml"
            || lengths.len() != 1
            || !get("transfer-encoding").is_empty()
        {
            return response(
                &mut stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                "{\"error\":\"invalid_request\"}",
            );
        }
        let Ok(length) = lengths[0].parse::<usize>() else {
            return response(
                &mut stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                "{\"error\":\"invalid_request\"}",
            );
        };
        if length == 0 || length > 8192 || request.len() > header_end + length {
            return response(
                &mut stream,
                "400 Bad Request",
                "application/json; charset=utf-8",
                "{\"error\":\"invalid_request\"}",
            );
        }
        while request.len() < header_end + length {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request deadline"))?;
            stream.set_read_timeout(Some(remaining))?;
            let n = stream.read(&mut block)?;
            if n == 0 {
                return Ok(());
            }
            request.extend_from_slice(&block[..n]);
            if request.len() > header_end + length {
                return response(
                    &mut stream,
                    "400 Bad Request",
                    "application/json; charset=utf-8",
                    "{\"error\":\"invalid_request\"}",
                );
            }
        }
        let body = match std::str::from_utf8(&request[header_end..]) {
            Ok(v) => v,
            Err(_) => {
                return response(
                    &mut stream,
                    "400 Bad Request",
                    "application/json; charset=utf-8",
                    "{\"error\":\"invalid_request\"}",
                )
            }
        };
        return match catalog.apply_toml(body) {
            Ok(revision) => response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                &format!("{{\"revision\":{revision}}}"),
            ),
            Err(StoreError::Invalid) => response(
                &mut stream,
                "422 Unprocessable Entity",
                "application/json; charset=utf-8",
                "{\"error\":\"invalid_catalog_entry\"}",
            ),
            Err(StoreError::Conflict) => response(
                &mut stream,
                "409 Conflict",
                "application/json; charset=utf-8",
                "{\"error\":\"catalog_conflict\"}",
            ),
            Err(StoreError::ReadOnly) => response(
                &mut stream,
                "403 Forbidden",
                "application/json; charset=utf-8",
                "{\"error\":\"read_only\"}",
            ),
            Err(StoreError::Io) => response(
                &mut stream,
                "500 Internal Server Error",
                "application/json; charset=utf-8",
                "{\"error\":\"catalog_io\"}",
            ),
        };
    }
    if method != "GET" {
        return response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            "Read only",
        );
    }
    match path {
        "/" => response(&mut stream, "200 OK", "text/html; charset=utf-8", PAGE),
        "/api/v1/host" => response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            &snapshot(endpoints),
        ),
        "/api/v1/games" => response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            &catalog
                .json()
                .unwrap_or_else(|_| "{\"error\":\"catalog_unavailable\"}".into()),
        ),
        _ => response(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "Not found",
        ),
    }
}

struct Connection(Arc<AtomicUsize>);
impl Drop for Connection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn serve(listener: TcpListener, endpoints: Vec<Endpoint>, catalog: Catalog) -> io::Result<()> {
    serve_store(
        listener,
        endpoints,
        Arc::new(CatalogStore::readonly(catalog)),
    )
}

pub fn serve_store(
    listener: TcpListener,
    endpoints: Vec<Endpoint>,
    catalog: Arc<CatalogStore>,
) -> io::Result<()> {
    if listener.local_addr()?.ip() != std::net::IpAddr::V4(Ipv4Addr::LOCALHOST)
        || endpoints.len() > MAX_ENDPOINTS
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "loopback and at most eight endpoints required",
        ));
    }
    let port = listener.local_addr()?.port();
    let endpoints = Arc::new(endpoints);
    let count = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let mut stream = stream?;
        if count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_CONNECTIONS).then_some(n + 1)
            })
            .is_err()
        {
            let _ = stream.set_write_timeout(Some(TIMEOUT));
            let _ = response(
                &mut stream,
                "503 Service Unavailable",
                "text/plain; charset=utf-8",
                "Busy",
            );
            continue;
        }
        let endpoints = Arc::clone(&endpoints);
        let catalog = Arc::clone(&catalog);
        let guard = Connection(Arc::clone(&count));
        std::thread::spawn(move || {
            let _guard = guard;
            let _ = handle(stream, port, &endpoints, &catalog);
        });
    }
    Ok(())
}

pub fn bind_local(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
}
