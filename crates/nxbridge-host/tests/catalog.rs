use nxbridge_host::{
    bind_local, serve, serve_store, Catalog, CatalogCommand, CatalogStore, StoreError,
};
use std::{
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, TcpStream},
    sync::{Arc, Barrier},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

const EXAMPLE: &str = include_str!("../../../config/game-catalog.example.toml");

#[test]
fn two_fixture_games_remain_independent_and_unknown_defaults_are_explicit() {
    let catalog = Catalog::parse(EXAMPLE).unwrap();
    let json = catalog.json();
    assert!(json.contains("\"id\":\"fixture-forest\""));
    assert!(json.contains("\"id\":\"fixture-puzzle\""));
    assert!(json.contains("\"video\":\"experimental\""));
    assert!(json.contains("\"input\":\"unsupported\""));
    assert!(json.contains("\"audio\":\"unknown\""));
    assert!(!json.contains("LOCAL_IPV4"));
    assert_eq!(
        Catalog::default().json(),
        "{\"api_version\":1,\"mode\":\"local_experiment\",\"revision\":0,\"write_enabled\":false,\"games\":[]}"
    );
}

fn post(port: u16, body: &str, origin: &str, token: bool) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let extra = if token {
        "X-NXBridge-Request: 1\r\n"
    } else {
        ""
    };
    write!(stream, "POST /api/v1/games HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/toml\r\nContent-Length: {}\r\n{}{}\r\n{}", body.len(), origin, extra, body).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn write_api_checks_origin_and_persists_without_exposing_paths() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nxbridge-api-{}-{unique}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("catalog.toml");
    fs::write(&path, "schema_version = 1\n").unwrap();
    let store = Arc::new(CatalogStore::writable(&path).unwrap());
    let listener = bind_local(0).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server_store = Arc::clone(&store);
    thread::spawn(move || {
        serve_store(listener, vec![], server_store).unwrap();
    });
    let body = format!("action = \"add_build\"\nrevision = 0\ngame_id = \"game\"\ngame_name = \"Game\"\nbuild_id = \"b1\"\nversion = \"1.0\"\ndirectory = {:?}", dir.to_str().unwrap());
    assert!(post(port, &body, "", false).starts_with("HTTP/1.1 403"));
    assert!(
        post(port, &body, "Origin: https://outside.example\r\n", true).starts_with("HTTP/1.1 403")
    );
    let origin = format!("Origin: http://127.0.0.1:{port}\r\n");
    assert!(post(port, &body, &origin, true).starts_with("HTTP/1.1 200"));
    assert!(post(port, &body, &origin, true).starts_with("HTTP/1.1 200"));
    let stale = "action = \"rename_game\"\nrevision = 0\ngame_id = \"game\"\ngame_name = \"Other\"";
    assert!(post(port, stale, &origin, true).starts_with("HTTP/1.1 409"));
    let json = store.json().unwrap();
    assert!(
        json.contains("\"directory_bound\":true") && json.contains("\"directory_accessible\":true")
    );
    assert!(!json.contains(dir.to_str().unwrap()));
    assert!(Catalog::load(&path)
        .unwrap()
        .json()
        .contains("\"id\":\"game\""));
    drop(store);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn simultaneous_edits_with_same_revision_cannot_overwrite_each_other() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nxbridge-race-{}-{unique}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("catalog.toml");
    fs::write(&path, EXAMPLE).unwrap();
    let store = Arc::new(CatalogStore::writable(&path).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for name in ["one", "two"] {
        let s = Arc::clone(&store);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait();
            s.apply(CatalogCommand::RenameGame {
                revision: 0,
                game_id: "fixture-forest".into(),
                game_name: name.into(),
            })
        }));
    }
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| **r == Ok(1)).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(StoreError::Conflict))
            .count(),
        1
    );
    drop(store);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn writes_are_atomic_idempotent_and_detect_conflicts() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("nxbridge-catalog-{}-{unique}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("catalog.toml");
    fs::write(&path, EXAMPLE).unwrap();
    let store = CatalogStore::writable(&path).unwrap();
    assert!(CatalogStore::writable(&path).is_err());
    let directory = dir.to_str().unwrap().to_string();
    let add = || CatalogCommand::AddBuild {
        revision: 0,
        game_id: "new-game".into(),
        game_name: "New game".into(),
        build_id: "build-1".into(),
        version: "1".into(),
        directory: directory.clone(),
    };
    assert_eq!(store.apply(add()), Ok(1));
    assert_eq!(store.apply(add()), Ok(1)); // retry after a lost response
    assert_eq!(
        store.apply(CatalogCommand::RenameGame {
            revision: 0,
            game_id: "new-game".into(),
            game_name: "Different".into()
        }),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.apply(CatalogCommand::RenameGame {
            revision: 1,
            game_id: "new-game".into(),
            game_name: "Renamed".into()
        }),
        Ok(2)
    );
    assert_eq!(
        store.apply(CatalogCommand::BindBuild {
            revision: 2,
            game_id: "fixture-forest".into(),
            build_id: "forest-test-1".into(),
            directory: directory.clone()
        }),
        Ok(3)
    );
    assert_eq!(
        store.apply(CatalogCommand::BindBuild {
            revision: 2,
            game_id: "fixture-forest".into(),
            build_id: "forest-test-1".into(),
            directory: directory.clone()
        }),
        Ok(3)
    );
    let data = Catalog::load(&path).unwrap().json();
    assert!(
        data.contains("\"revision\":3")
            && data.contains("Renamed")
            && data.contains("\"directory_bound\":true")
    );
    assert!(!data.contains(&directory)); // private resource paths are not returned
    fs::write(&path, EXAMPLE).unwrap();
    assert_eq!(
        store.apply(CatalogCommand::RenameGame {
            revision: 3,
            game_id: "new-game".into(),
            game_name: "Lost".into()
        }),
        Err(StoreError::Conflict)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), EXAMPLE);
    drop(store);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_duplicate_malformed_private_and_unsubstantiated_data() {
    assert!(Catalog::parse("schema_version = 2").is_err());
    assert!(Catalog::parse(&"x".repeat(1024 * 1024 + 1)).is_err());
    assert!(Catalog::parse(
        "schema_version = 1\n[[games]]\nid='same'\nname='a'\n[[games]]\nid='same'\nname='b'"
    )
    .is_err());
    assert!(Catalog::parse("schema_version = 1\n[[games]]\nid='../x'\nname='a'").is_err());
    assert!(Catalog::parse(
        "schema_version = 1\n[[games]]\nid='x'\nname='a'\npath='/private/save'"
    )
    .is_err());
    assert!(Catalog::parse("schema_version = 1\n[[games]]\nid='x'\nname='a'\n[[games.builds]]\nid='b'\nversion='1'\nstatus='verified'").is_err());
    let with_evidence = "schema_version = 1\n[[games]]\nid='x'\nname='a'\n[[games.builds]]\nid='b'\nversion='1'\nstatus='verified'\nevidence='NXB-001'";
    assert!(Catalog::parse(with_evidence).is_ok());
    let duplicate_build = "schema_version = 1\n[[games]]\nid='x'\nname='a'\n[[games.builds]]\nid='same'\nversion='1'\n[[games.builds]]\nid='same'\nversion='2'";
    assert!(Catalog::parse(duplicate_build).is_err());
    let unknown_capability = "schema_version = 1\n[[games]]\nid='x'\nname='a'\n[[games.builds]]\nid='b'\nversion='1'\n[games.builds.capabilities]\nvideo='yes'";
    assert!(Catalog::parse(unknown_capability).is_err());
}

#[test]
fn catalog_is_served_by_shared_read_only_http_api() {
    let listener = bind_local(0).unwrap();
    let port = listener.local_addr().unwrap().port();
    let catalog = Catalog::parse(EXAMPLE).unwrap();
    thread::spawn(move || {
        serve(listener, vec![], catalog).unwrap();
    });
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        stream,
        "GET /api/v1/games HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .unwrap();
    let mut result = String::new();
    stream.read_to_string(&mut result).unwrap();
    assert!(result.starts_with("HTTP/1.1 200 OK"));
    assert!(result.contains("fixture-forest") && result.contains("fixture-puzzle"));
    assert!(result.contains("\"input\":\"unsupported\""));
}

#[test]
fn covers_are_validated_versioned_atomic_and_do_not_change_game_identity() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nxbridge-cover-{}-{unique}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("catalog.toml");
    fs::write(&path, EXAMPLE).unwrap();
    let store = CatalogStore::writable(&path).unwrap();
    let png = |color: [u8; 3]| {
        let mut bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut bytes, 1, 1);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(&color)
                .unwrap();
        }
        bytes
    };
    let original = fs::read(&path).unwrap();
    assert!(store.json().unwrap().contains("\"cover_version\":null"));
    assert_eq!(store.cover("fixture-forest"), Err(StoreError::Invalid));
    assert_eq!(
        store.set_cover("fixture-forest", "", &vec![0; 5 * 1024 * 1024]),
        Err(StoreError::Invalid)
    );
    assert_eq!(
        store.set_cover("../bad", "", &png([1, 2, 3])),
        Err(StoreError::Invalid)
    );
    assert_eq!(
        store.set_cover("fixture-forest", "", b"not a png"),
        Err(StoreError::Invalid)
    );
    let a = png([1, 2, 3]);
    let mut broken = a.clone();
    broken.truncate(broken.len() - 16);
    assert_eq!(
        store.set_cover("fixture-forest", "", &broken),
        Err(StoreError::Invalid)
    );
    let va = store.set_cover("fixture-forest", "", &a).unwrap();
    assert_eq!(va.len(), 64);
    assert_eq!(store.cover("fixture-forest").unwrap(), a);
    assert_eq!(fs::read(&path).unwrap(), original);
    let b = png([4, 5, 6]);
    assert_eq!(
        store.set_cover("fixture-forest", "", &b),
        Err(StoreError::Conflict)
    );
    let vb = store.set_cover("fixture-forest", &va, &b).unwrap();
    assert_ne!(va, vb);
    assert_eq!(store.cover("fixture-forest").unwrap(), b);
    assert!(store.json().unwrap().contains(&vb));
    drop(store);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cover_http_requires_token_and_origin_and_serves_binary() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "nxbridge-cover-http-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("catalog.toml");
    fs::write(&path, EXAMPLE).unwrap();
    let store = Arc::new(CatalogStore::writable(&path).unwrap());
    let listener = bind_local(0).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || serve_store(listener, vec![], store).unwrap());
    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, 1, 1);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&[1, 2, 3])
            .unwrap();
    }
    let upload = |extra: &str| {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(stream, "POST /api/v1/covers/fixture-forest HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: image/png\r\nContent-Length: {}\r\nX-NXBridge-Cover-Version: \r\n{}\r\n", png.len(), extra).unwrap();
        stream.write_all(&png).unwrap();
        let mut result = String::new();
        stream.read_to_string(&mut result).unwrap();
        result
    };
    assert!(upload("").starts_with("HTTP/1.1 403"));
    assert!(
        upload("X-NXBridge-Request: 1\r\nOrigin: https://outside.example\r\n")
            .starts_with("HTTP/1.1 403")
    );
    assert!(upload("X-NXBridge-Request: 1\r\n").starts_with("HTTP/1.1 200"));
    assert!(upload("X-NXBridge-Request: 1\r\n").starts_with("HTTP/1.1 409"));
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        stream,
        "GET /api/v1/covers/fixture-forest HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .unwrap();
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).unwrap();
    assert_eq!(
        &reply[reply.windows(4).position(|x| x == b"\r\n\r\n").unwrap() + 4..],
        &png
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn competing_cover_replacements_do_not_lose_an_update() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "nxbridge-cover-race-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("catalog.toml");
    fs::write(&path, EXAMPLE).unwrap();
    let store = Arc::new(CatalogStore::writable(&path).unwrap());
    let png = |value: u8| {
        let mut bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut bytes, 1, 1);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(&[value, 2, 3])
                .unwrap();
        }
        bytes
    };
    let version = store.set_cover("fixture-forest", "", &png(1)).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = [2, 3]
        .into_iter()
        .map(|v| {
            let (s, b, version, data) = (
                Arc::clone(&store),
                Arc::clone(&barrier),
                version.clone(),
                png(v),
            );
            thread::spawn(move || {
                b.wait();
                s.set_cover("fixture-forest", &version, &data)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(StoreError::Conflict))
            .count(),
        1
    );
    drop(store);
    fs::remove_dir_all(dir).unwrap();
}
