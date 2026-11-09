//! Bounded local catalog metadata; game resources and saves are never modified.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::json_string;

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_GAMES: usize = 64;
const MAX_BUILDS: usize = 16;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    #[default]
    Unknown,
    Experimental,
    Verified,
    Unsupported,
}
impl Status {
    fn value(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Experimental => "experimental",
            Self::Verified => "verified",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Capabilities {
    video: Status,
    audio: Status,
    input: Status,
    save: Status,
    reconnect: Status,
    motion: Status,
    rumble: Status,
}
impl Capabilities {
    fn fields(&self) -> [(&'static str, Status); 7] {
        [
            ("video", self.video),
            ("audio", self.audio),
            ("input", self.input),
            ("save", self.save),
            ("reconnect", self.reconnect),
            ("motion", self.motion),
            ("rumble", self.rumble),
        ]
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Build {
    id: String,
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resource_path: Option<String>,
    #[serde(default)]
    status: Status,
    #[serde(default)]
    evidence: Option<String>,
    #[serde(default)]
    capabilities: Capabilities,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Game {
    id: String,
    name: String,
    #[serde(default)]
    builds: Vec<Build>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    schema_version: u32,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    games: Vec<Game>,
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            games: Vec::new(),
        }
    }
}
impl Catalog {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() as u64 > MAX_BYTES {
            return Err("catalog is too large".into());
        }
        let catalog: Self =
            toml::from_str(text).map_err(|_| "invalid catalog TOML or unknown field")?;
        if catalog.schema_version != 1
            || catalog.games.len() > MAX_GAMES
            || catalog.revision > 9_007_199_254_740_990
        {
            return Err("unsupported catalog version or too many games".into());
        }
        let mut game_ids = HashSet::new();
        for game in &catalog.games {
            if !valid_id(&game.id)
                || !valid_text(&game.name, 128)
                || !game_ids.insert(&game.id)
                || game.builds.len() > MAX_BUILDS
            {
                return Err("invalid or duplicate game, or too many builds".into());
            }
            let mut build_ids = HashSet::new();
            for build in &game.builds {
                if !valid_id(&build.id)
                    || !valid_text(&build.version, 64)
                    || !build_ids.insert(&build.id)
                    || build.evidence.as_ref().is_some_and(|v| !valid_id(v))
                    || build
                        .resource_path
                        .as_ref()
                        .is_some_and(|v| !valid_text(v, 4096) || !Path::new(v).is_absolute())
                {
                    return Err("invalid or duplicate build".into());
                }
                if (build.status == Status::Verified
                    || build
                        .capabilities
                        .fields()
                        .iter()
                        .any(|(_, s)| *s == Status::Verified))
                    && build.evidence.is_none()
                {
                    return Err("verified status requires an evidence identifier".into());
                }
            }
        }
        Ok(catalog)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|_| "cannot open catalog")?;
        let mut data = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|_| "cannot read catalog")?;
        if data.len() as u64 > MAX_BYTES {
            return Err("catalog is too large".into());
        }
        let text = std::str::from_utf8(&data).map_err(|_| "catalog is not UTF-8")?;
        Self::parse(text)
    }
    pub fn json(&self) -> String {
        self.json_with_access(false, None)
    }
    fn json_with_access(&self, writable: bool, cover_dir: Option<&Path>) -> String {
        let games = self.games.iter().map(|game| {
            let builds = game.builds.iter().map(|build| {
                let capabilities = build.capabilities.fields().iter()
                    .map(|(name, state)| format!("\"{name}\":\"{}\"", state.value()))
                    .collect::<Vec<_>>().join(",");
                let evidence = build.evidence.as_deref().map(json_string).unwrap_or_else(|| "null".into());
                let bound = build.resource_path.is_some();
                let accessible = build.resource_path.as_ref().is_some_and(|p| Path::new(p).is_dir());
                format!("{{\"id\":{},\"version\":{},\"status\":\"{}\",\"evidence\":{},\"directory_bound\":{},\"directory_accessible\":{},\"capabilities\":{{{}}}}}",
                    json_string(&build.id), json_string(&build.version), build.status.value(), evidence, bound, accessible, capabilities)
            }).collect::<Vec<_>>().join(",");
            let cover = cover_dir.and_then(|dir| fs::read(dir.join(format!("{}.png", game.id))).ok())
                .map(|bytes| json_string(&hex::encode(Sha256::digest(bytes))))
                .unwrap_or_else(|| "null".into());
            format!("{{\"id\":{},\"name\":{},\"cover_version\":{},\"builds\":[{}]}}", json_string(&game.id), json_string(&game.name), cover, builds)
        }).collect::<Vec<_>>().join(",");
        format!(
            "{{\"api_version\":1,\"mode\":\"local_experiment\",\"revision\":{},\"write_enabled\":{},\"games\":[{}]}}",
            self.revision, writable, games
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogCommand {
    AddBuild {
        revision: u64,
        game_id: String,
        game_name: String,
        build_id: String,
        version: String,
        directory: String,
    },
    BindBuild {
        revision: u64,
        game_id: String,
        build_id: String,
        directory: String,
    },
    RenameGame {
        revision: u64,
        game_id: String,
        game_name: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    Invalid,
    Conflict,
    ReadOnly,
    Io,
}

struct StoreState {
    catalog: Catalog,
    source: Vec<u8>,
}
pub struct CatalogStore {
    state: Mutex<StoreState>,
    path: Option<PathBuf>,
    _lock: Option<File>,
}

impl CatalogStore {
    pub fn readonly(catalog: Catalog) -> Self {
        Self {
            state: Mutex::new(StoreState {
                catalog,
                source: Vec::new(),
            }),
            path: None,
            _lock: None,
        }
    }
    pub fn writable(path: &Path) -> Result<Self, String> {
        let metadata = fs::symlink_metadata(path).map_err(|_| "cannot inspect catalog")?;
        if !metadata.file_type().is_file() {
            return Err("catalog must be a regular file".into());
        }
        let lock_path = path.with_extension("toml.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(lock_path)
            .map_err(|_| "cannot open catalog lock")?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("catalog is already managed by another process".into());
        }
        let mut source = Vec::new();
        File::open(path)
            .map_err(|_| "cannot open catalog")?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut source)
            .map_err(|_| "cannot read catalog")?;
        if source.len() as u64 > MAX_BYTES {
            return Err("catalog is too large".into());
        }
        let catalog =
            Catalog::parse(std::str::from_utf8(&source).map_err(|_| "catalog is not UTF-8")?)?;
        Ok(Self {
            state: Mutex::new(StoreState { catalog, source }),
            path: Some(path.to_path_buf()),
            _lock: Some(lock),
        })
    }
    pub fn json(&self) -> Result<String, StoreError> {
        let state = self.state.lock().map_err(|_| StoreError::Io)?;
        let cover_dir = self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("covers"));
        Ok(state
            .catalog
            .json_with_access(self.path.is_some(), cover_dir.as_deref()))
    }
    pub fn cover(&self, id: &str) -> Result<Vec<u8>, StoreError> {
        let path = self.path.as_ref().ok_or(StoreError::ReadOnly)?;
        let state = self.state.lock().map_err(|_| StoreError::Io)?;
        if !valid_id(id) || !state.catalog.games.iter().any(|g| g.id == id) {
            return Err(StoreError::Invalid);
        }
        fs::read(
            path.parent()
                .ok_or(StoreError::Io)?
                .join("covers")
                .join(format!("{id}.png")),
        )
        .map_err(|_| StoreError::Invalid)
    }
    pub fn set_cover(&self, id: &str, expected: &str, bytes: &[u8]) -> Result<String, StoreError> {
        let path = self.path.as_ref().ok_or(StoreError::ReadOnly)?;
        let state = self.state.lock().map_err(|_| StoreError::Io)?;
        if !valid_id(id)
            || !state.catalog.games.iter().any(|g| g.id == id)
            || bytes.len() >= 5 * 1024 * 1024
            || bytes.len() < 33
            || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
            || &bytes[12..16] != b"IHDR"
        {
            return Err(StoreError::Invalid);
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(StoreError::Invalid);
        }
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_limits(png::Limits {
            bytes: 80 * 1024 * 1024,
        });
        let mut reader = decoder.read_info().map_err(|_| StoreError::Invalid)?;
        let size = reader.output_buffer_size().ok_or(StoreError::Invalid)?;
        if size > 80 * 1024 * 1024 {
            return Err(StoreError::Invalid);
        }
        reader
            .next_frame(&mut vec![0; size])
            .map_err(|_| StoreError::Invalid)?;
        if fs::read(path).map_err(|_| StoreError::Io)? != state.source {
            return Err(StoreError::Conflict);
        }
        let dir = path.parent().ok_or(StoreError::Io)?.join("covers");
        fs::create_dir_all(&dir).map_err(|_| StoreError::Io)?;
        let dest = dir.join(format!("{id}.png"));
        let current = fs::read(&dest)
            .ok()
            .map(|data| hex::encode(Sha256::digest(data)))
            .unwrap_or_default();
        if current != expected {
            return Err(StoreError::Conflict);
        }
        let version = hex::encode(Sha256::digest(bytes));
        if version != current {
            atomic_write(&dest, bytes)?;
        }
        Ok(version)
    }
    pub fn apply_toml(&self, text: &str) -> Result<u64, StoreError> {
        if text.len() > 8192 {
            return Err(StoreError::Invalid);
        }
        let command: CatalogCommand = toml::from_str(text).map_err(|_| StoreError::Invalid)?;
        self.apply(command)
    }
    pub fn apply(&self, command: CatalogCommand) -> Result<u64, StoreError> {
        let path = self.path.as_ref().ok_or(StoreError::ReadOnly)?;
        let mut state = self.state.lock().map_err(|_| StoreError::Io)?;
        let mut candidate = state.catalog.clone();
        let revision = candidate.revision;
        let (expected, unchanged) = match command {
            CatalogCommand::AddBuild {
                revision: expected,
                game_id,
                game_name,
                build_id,
                version,
                directory,
            } => {
                if !valid_id(&game_id)
                    || !valid_text(&game_name, 128)
                    || !valid_id(&build_id)
                    || !valid_text(&version, 64)
                {
                    return Err(StoreError::Invalid);
                }
                let directory = valid_directory(&directory)?;
                let game = match candidate.games.iter_mut().find(|g| g.id == game_id) {
                    Some(g) if g.name == game_name => g,
                    Some(_) => return Err(StoreError::Conflict),
                    None => {
                        if candidate.games.len() >= MAX_GAMES {
                            return Err(StoreError::Invalid);
                        }
                        candidate.games.push(Game {
                            id: game_id,
                            name: game_name,
                            builds: Vec::new(),
                        });
                        candidate.games.last_mut().ok_or(StoreError::Io)?
                    }
                };
                if let Some(existing) = game.builds.iter().find(|b| b.id == build_id) {
                    if existing.version != version
                        || existing.resource_path.as_ref() != Some(&directory)
                    {
                        return Err(StoreError::Conflict);
                    }
                    (expected, true)
                } else {
                    if game.builds.len() >= MAX_BUILDS {
                        return Err(StoreError::Invalid);
                    }
                    game.builds.push(Build {
                        id: build_id,
                        version,
                        resource_path: Some(directory),
                        status: Status::Unknown,
                        evidence: None,
                        capabilities: Capabilities::default(),
                    });
                    (expected, false)
                }
            }
            CatalogCommand::BindBuild {
                revision: expected,
                game_id,
                build_id,
                directory,
            } => {
                let directory = valid_directory(&directory)?;
                let build = candidate
                    .games
                    .iter_mut()
                    .find(|g| g.id == game_id)
                    .and_then(|g| g.builds.iter_mut().find(|b| b.id == build_id))
                    .ok_or(StoreError::Invalid)?;
                match &build.resource_path {
                    Some(old) if old == &directory => (expected, true),
                    Some(_) => return Err(StoreError::Conflict),
                    None => {
                        build.resource_path = Some(directory);
                        (expected, false)
                    }
                }
            }
            CatalogCommand::RenameGame {
                revision: expected,
                game_id,
                game_name,
            } => {
                if !valid_id(&game_id) || !valid_text(&game_name, 128) {
                    return Err(StoreError::Invalid);
                }
                let game = candidate
                    .games
                    .iter_mut()
                    .find(|g| g.id == game_id)
                    .ok_or(StoreError::Invalid)?;
                if game.name == game_name {
                    (expected, true)
                } else {
                    game.name = game_name;
                    (expected, false)
                }
            }
        };
        let mut current = Vec::new();
        File::open(path)
            .map_err(|_| StoreError::Io)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut current)
            .map_err(|_| StoreError::Io)?;
        if current != state.source {
            return Err(StoreError::Conflict);
        }
        if unchanged {
            return Ok(revision);
        }
        if expected != revision {
            return Err(StoreError::Conflict);
        }
        candidate.revision = revision.checked_add(1).ok_or(StoreError::Invalid)?;
        let serialized = toml::to_string(&candidate).map_err(|_| StoreError::Io)?;
        Catalog::parse(&serialized).map_err(|_| StoreError::Invalid)?;
        atomic_write(path, serialized.as_bytes())?;
        state.source = serialized.into_bytes();
        state.catalog = candidate;
        Ok(state.catalog.revision)
    }
}

fn valid_directory(value: &str) -> Result<String, StoreError> {
    if !valid_text(value, 4096) || !Path::new(value).is_absolute() {
        return Err(StoreError::Invalid);
    }
    let path = fs::canonicalize(value).map_err(|_| StoreError::Invalid)?;
    if !path.is_dir() {
        return Err(StoreError::Invalid);
    }
    let path = path.to_str().ok_or(StoreError::Invalid)?;
    if !valid_text(path, 4096) {
        return Err(StoreError::Invalid);
    }
    Ok(path.into())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or(StoreError::Io)?;
    let mut attempt = 0u64;
    let (temporary, mut file) = loop {
        let temp = parent.join(format!(
            ".nxbridge-catalog-{}-{attempt}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 20 => attempt += 1,
            Err(_) => return Err(StoreError::Io),
        }
    };
    let result = (|| {
        file.write_all(bytes).map_err(|_| StoreError::Io)?;
        file.sync_all().map_err(|_| StoreError::Io)?;
        fs::rename(&temporary, path).map_err(|_| StoreError::Io)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
