use nxbridge_host::{bind_local, observe, serve, snapshot, Catalog, Endpoint, Observation};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

fn fake_reply(body: String) -> Endpoint {
    let listener = bind_local(0).unwrap();
    let address = match listener.local_addr().unwrap() {
        std::net::SocketAddr::V4(v4) => v4,
        _ => unreachable!(),
    };
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        let reply = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{body}");
        let _ = stream.write_all(reply.as_bytes());
    });
    Endpoint::parse(&format!("测试={address}")).unwrap()
}

#[test]
fn probes_known_states_and_rejects_malformed_or_oversized_replies() {
    for (state, expected) in [
        ("MOONSHINE_SERVER_FREE", Observation::Ready),
        ("MOONSHINE_SERVER_BUSY", Observation::Busy),
    ] {
        let endpoint = fake_reply(format!(
            "<root status_code=\"200\"><state>{state}</state></root>"
        ));
        assert_eq!(observe(&endpoint), expected);
    }
    let malformed = fake_reply("<root status_code=\"200\"><state>WRONG</state></root>".into());
    assert_eq!(observe(&malformed), Observation::Unknown);
    let oversized = fake_reply("x".repeat(17 * 1024));
    assert_eq!(observe(&oversized), Observation::Unknown);
    let listener = bind_local(0).unwrap();
    let address = match listener.local_addr().unwrap() {
        std::net::SocketAddr::V4(v4) => v4,
        _ => unreachable!(),
    };
    drop(listener);
    assert_eq!(
        observe(&Endpoint::parse(&format!("离线={address}")).unwrap()),
        Observation::Unreachable
    );
}

#[test]
fn local_only_endpoint_validation_and_unknown_metrics() {
    assert!(Endpoint::parse("pad=0.0.0.0:3000").is_err());
    assert!(Endpoint::parse("pad=203.0.113.1:3000").is_err());
    assert!(Endpoint::parse("bad\nlabel=127.0.0.1:3000").is_err());
    assert!(Endpoint::parse("pad=127.0.0.1:3000").is_ok());
    let wildcard = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    assert_eq!(
        serve(wildcard, vec![], Catalog::default())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    let listener = bind_local(0).unwrap();
    let address = match listener.local_addr().unwrap() {
        std::net::SocketAddr::V4(v4) => v4,
        _ => unreachable!(),
    };
    drop(listener);
    let json = snapshot(&[Endpoint::parse(&format!("Pad \"甲\" \\={address}")).unwrap()]);
    assert!(json.contains("Pad \\\"甲\\\" \\\\"));
    assert!(json.contains("\"status\":\"unreachable\""));
    assert!(json.contains("\"measured_session_capacity\":null"));
    assert!(json.contains("\"game_fps\":null"));
}

fn request(port: u16, text: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(text.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply
}

#[test]
fn actual_http_service_enforces_local_host_and_read_only_routes() {
    let listener = bind_local(0).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        serve(listener, vec![], Catalog::default()).unwrap();
    });
    let get = request(
        port,
        &format!("GET /api/v1/host HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"),
    );
    assert!(get.starts_with("HTTP/1.1 200 OK"));
    assert!(get.contains("\"measured_session_capacity\":null"));
    let page = request(
        port,
        &format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"),
    );
    assert!(page.contains("主机概览") && page.contains("/api/v1/host"));
    assert!(request(
        port,
        &format!("POST /api/v1/host HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n")
    )
    .starts_with("HTTP/1.1 405"));
    assert!(request(
        port,
        "GET /api/v1/host HTTP/1.1\r\nHost: attacker.example\r\n\r\n"
    )
    .starts_with("HTTP/1.1 403"));
    assert!(request(
        port,
        &format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\nHost: localhost:{port}\r\n\r\n")
    )
    .starts_with("HTTP/1.1 403"));
    assert!(request(
        port,
        &format!("GET /missing HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n")
    )
    .starts_with("HTTP/1.1 404"));
}

#[test]
fn slow_backend_has_a_total_read_deadline() {
    let listener = bind_local(0).unwrap();
    let address = match listener.local_addr().unwrap() {
        std::net::SocketAddr::V4(v4) => v4,
        _ => unreachable!(),
    };
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        for _ in 0..12 {
            if stream.write_all(b"x").is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(150));
        }
    });
    let start = Instant::now();
    assert_eq!(
        observe(&Endpoint::parse(&format!("slow={address}")).unwrap()),
        Observation::Unknown
    );
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn slow_client_cannot_hold_an_incomplete_request_indefinitely() {
    let listener = bind_local(0).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        serve(listener, vec![], Catalog::default()).unwrap();
    });
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\nHost: ").unwrap();
    let start = Instant::now();
    let mut out = [0; 1];
    assert_eq!(stream.read(&mut out).unwrap(), 0);
    assert!(start.elapsed() < Duration::from_secs(2));
}
