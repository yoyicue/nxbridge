use moonshine_core::session::application::ApplicationConfig;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    io,
    path::{Path, PathBuf},
};

/// Local experimental launch configuration. This is deliberately separate from the
/// Host library until that API owns session and save lifecycle.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Game {
    pub title: String,
    #[serde(default)]
    pub library_game_id: Option<String>,
    pub boxart: Option<PathBuf>,
    pub worker: PathBuf,
    pub host: PathBuf,
    pub game_dir: PathBuf,
    pub sandbox: PathBuf,
    pub environment: BTreeMap<String, String>,
}

pub fn library_cover(dir: &Path, id: &str) -> io::Result<PathBuf> {
    if !dir.is_absolute()
        || id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err(invalid("invalid library cover directory or game ID"));
    }
    Ok(dir.join(format!("{id}.png")))
}

impl Game {
    pub fn application(&self) -> ApplicationConfig {
        ApplicationConfig {
            title: self.title.clone(),
            library_game_id: self.library_game_id.clone(),
            boxart: self.boxart.clone(),
            ..Default::default()
        }
    }
}

pub fn load(path: &Path, cover_dir: Option<&Path>) -> io::Result<Vec<Game>> {
    let mut games: Vec<Game> = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    // A library mapping supersedes legacy boxart entirely, even if its old
    // path no longer exists. The library cover itself may be added later.
    if cover_dir.is_some() {
        for game in &mut games {
            if game.library_game_id.is_some() {
                game.boxart = None;
            }
        }
    }
    validate(&games)?;
    if let Some(dir) = cover_dir {
        for game in &mut games {
            if let Some(id) = &game.library_game_id {
                game.boxart = Some(library_cover(dir, id)?);
            }
        }
    }
    Ok(games)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn validate(games: &[Game]) -> io::Result<()> {
    if games.is_empty() {
        return Err(invalid("game catalog is empty"));
    }
    let mut ids = HashSet::new();
    let mut library_ids = HashSet::new();
    let mut writable = Vec::new();
    for g in games {
        if let Some(id) = &g.library_game_id {
            library_cover(Path::new("/"), id)?;
            if !library_ids.insert(id) {
                return Err(invalid("duplicate library game ID"));
            }
        }
        if g.title.trim().is_empty() || !ids.insert(g.application().id()) {
            return Err(invalid("empty title or conflicting application ID"));
        }
        for p in [&g.worker, &g.host, &g.sandbox] {
            if !p.is_absolute() || !p.is_file() {
                return Err(invalid(
                    "worker, host and sandbox must be existing absolute files",
                ));
            }
        }
        if !g.game_dir.is_absolute() || !g.game_dir.is_dir() {
            return Err(invalid("game_dir must be an existing absolute directory"));
        }
        if g.boxart
            .as_ref()
            .is_some_and(|p| !p.is_absolute() || !p.is_file())
        {
            return Err(invalid("boxart must be an existing absolute file"));
        }
        for key in g.environment.keys() {
            if !key.starts_with("DM_")
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                return Err(invalid("game environment only accepts DM_ variables"));
            }
        }
        for key in ["DM_SAVE", "DM_TEMP", "DM_CACHE"] {
            if key == "DM_CACHE" && !g.environment.contains_key(key) {
                continue;
            }
            let p = Path::new(
                g.environment
                    .get(key)
                    .ok_or_else(|| invalid("DM_SAVE and DM_TEMP are required"))?,
            );
            if !p.is_absolute() || !p.is_dir() {
                return Err(invalid(
                    "save, temp and configured cache must be existing absolute directories",
                ));
            }
            let p = p.canonicalize()?;
            if writable
                .iter()
                .any(|other: &PathBuf| p.starts_with(other) || other.starts_with(&p))
            {
                return Err(invalid(
                    "game save, temp and cache directories must not overlap",
                ));
            }
            writable.push(p);
        }
    }
    Ok(())
}

pub fn select(games: &[Game], id: i32) -> Option<&Game> {
    games.iter().find(|g| g.application().id() == id)
}

pub fn apply_environment(
    cmd: &mut tokio::process::Command,
    game: &Game,
    inherited_keys: impl IntoIterator<Item = std::ffi::OsString>,
) {
    for key in inherited_keys {
        if key.to_string_lossy().starts_with("DM_") {
            cmd.env_remove(key);
        }
    }
    cmd.envs(&game.environment);
    cmd.current_dir(&game.environment["DM_TEMP"]);
}

/// Held until the game process has exited. This guards cooperating NXBridge
/// processes; unrelated software and older services do not take this lock.
pub fn lock_save(game: &Game) -> io::Result<std::fs::File> {
    use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
    // nxnative atomically replaces the save directory when it commits. A lock
    // inside that directory would be moved away along with the old contents.
    let save = Path::new(&game.environment["DM_SAVE"]).canonicalize()?;
    let parent = save
        .parent()
        .ok_or_else(|| invalid("save directory has no parent"))?;
    let name = save
        .file_name()
        .ok_or_else(|| invalid("save directory has no name"))?;
    let mut lock_name = std::ffi::OsString::from(".");
    lock_name.push(name);
    lock_name.push(".nxbridge-writer.lock");
    let lock = parent.join(lock_name);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(lock)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn library_cover_mapping_does_not_require_the_old_or_new_cover_to_exist() {
        let root = std::env::temp_dir().join(format!(
            "nxbridge-cover-map-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("save")).unwrap();
        std::fs::create_dir_all(root.join("temp")).unwrap();
        let executable = std::env::current_exe().unwrap();
        let config = serde_json::json!([{
            "title":"Example", "library_game_id":"example", "boxart":root.join("old.png"),
            "worker":executable, "host":executable, "sandbox":executable, "game_dir":root,
            "environment":{"DM_SAVE":root.join("save"),"DM_TEMP":root.join("temp")}
        }]);
        let path = root.join("catalog.json");
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        assert!(load(&path, None).is_err());
        let games = load(&path, Some(&root.join("covers"))).unwrap();
        assert_eq!(
            games[0].boxart.as_deref(),
            Some(root.join("covers/example.png").as_path())
        );
        assert_eq!(
            games[0].application().library_game_id.as_deref(),
            Some("example")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn routes_by_application_and_rejects_shared_writable_directories() {
        let root = std::env::temp_dir().join(format!(
            "nxbridge-catalog-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for n in ["save1", "temp1", "save2", "temp2", "cache1", "cache2"] {
            std::fs::create_dir(root.join(n)).unwrap();
        }
        let file = std::env::current_exe().unwrap();
        let game = |title: &str, save: &str, temp: &str| Game {
            library_game_id: None,
            title: title.into(),
            boxart: None,
            worker: file.clone(),
            host: file.clone(),
            sandbox: file.clone(),
            game_dir: root.clone(),
            environment: BTreeMap::from([
                (
                    "DM_SAVE".into(),
                    root.join(save).to_string_lossy().into_owned(),
                ),
                (
                    "DM_TEMP".into(),
                    root.join(temp).to_string_lossy().into_owned(),
                ),
            ]),
        };
        let a = game("BOTW", "save1", "temp1");
        let b = game("TOTK", "save2", "temp2");
        validate(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(
            select(&[a.clone(), b.clone()], b.application().id())
                .unwrap()
                .title,
            "TOTK"
        );
        assert!(select(&[a.clone(), b.clone()], 0).is_none());
        assert!(validate(&[a.clone(), game("other", "save1", "temp2")]).is_err());
        assert!(validate(&[a.clone(), game("BOTW", "save2", "temp2")]).is_err());
        let mut with_cache = a.clone();
        with_cache.environment.insert(
            "DM_CACHE".into(),
            root.join("cache1").to_string_lossy().into_owned(),
        );
        validate(&[with_cache.clone(), b.clone()]).unwrap();
        let mut conflict = b.clone();
        conflict.environment.insert(
            "DM_CACHE".into(),
            root.join("cache1").to_string_lossy().into_owned(),
        );
        assert!(validate(&[with_cache.clone(), conflict]).is_err());
        with_cache.environment.insert(
            "DM_CACHE".into(),
            root.join("temp1").to_string_lossy().into_owned(),
        );
        assert!(validate(&[with_cache]).is_err());
        let mut cmd = tokio::process::Command::new(&b.worker);
        apply_environment(
            &mut cmd,
            &b,
            ["DM_SAVE".into(), "DM_CHEATS".into(), "PATH".into()],
        );
        let env = cmd.as_std().get_envs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            env.get(std::ffi::OsStr::new("DM_SAVE")).unwrap().unwrap(),
            b.environment["DM_SAVE"].as_str()
        );
        assert!(env
            .get(std::ffi::OsStr::new("DM_CHEATS"))
            .unwrap()
            .is_none());
        assert!(!env.contains_key(std::ffi::OsStr::new("PATH")));
        let lock = lock_save(&a).unwrap();
        assert!(lock_save(&a).is_err());
        std::fs::rename(root.join("save1"), root.join("save1-old")).unwrap();
        std::fs::create_dir(root.join("save1")).unwrap();
        assert!(lock_save(&a).is_err());
        drop(lock);
        assert!(lock_save(&a).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
