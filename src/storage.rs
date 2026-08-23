use crate::model::{Conversation, Engine};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const DEFAULT_ICON: &str = "⬜";
const META_FILE: &str = ".lmchatty-meta";
/// A directory of files under the lmchatty root that gets copied into every
/// new conversation's working directory (e.g. a default CLAUDE.md/AGENTS.md).
/// Not itself treated as a conversation since it has no `.lmchatty-meta`.
const PRESET_DIR: &str = "preset";

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

pub fn lmchatty_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/lmchatty"))
}

pub fn load_conversations() -> Result<Vec<Conversation>> {
    let root = lmchatty_dir()?;
    fs::create_dir_all(&root).context("could not create the lmchatty directory")?;
    let mut conversations = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let hash_dir = entry.path();
        let meta = hash_dir.join(META_FILE);
        if meta.is_file() {
            if let Ok(conversation) = read_conversation(&hash_dir, &meta) {
                conversations.push(conversation);
            }
        }
    }
    conversations.sort_by(|left, right| {
        right
            .pinned
            .cmp(&left.pinned)
            .then_with(|| right.created.cmp(&left.created))
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });
    Ok(conversations)
}

fn read_conversation(hash_dir: &Path, meta: &Path) -> Result<Conversation> {
    let contents = fs::read_to_string(meta)?;
    let mut title = None;
    let mut icon = DEFAULT_ICON.to_owned();
    let mut engine = Engine::Codex;
    let mut pinned = false;
    let mut started = false;
    let mut created = 0;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "title" => title = Some(value.to_owned()),
            "icon" if !value.is_empty() => icon = value.to_owned(),
            "engine" if value.eq_ignore_ascii_case("claude") => engine = Engine::Claude,
            "pinned" => pinned = value == "true",
            "started" => started = value == "true",
            "created" => created = value.parse().unwrap_or_default(),
            _ => {}
        }
    }
    let title = title.ok_or_else(|| anyhow!("conversation metadata has no title"))?;
    let path = hash_dir.join(&title);
    if !path.is_dir() {
        bail!("conversation directory is missing: {}", path.display());
    }
    Ok(Conversation {
        hash: directory_name(hash_dir),
        title,
        icon,
        engine,
        pinned,
        started,
        created,
        hash_dir: hash_dir.to_owned(),
        path,
    })
}

pub fn create_conversation(engine: Engine, requested_title: &str) -> Result<Conversation> {
    let root = lmchatty_dir()?;
    fs::create_dir_all(&root)?;
    let title = if requested_title.trim().is_empty() {
        default_title()
    } else {
        requested_title.trim().to_owned()
    };
    validate_name(&title)?;
    let created = unix_now();
    let hash = short_hash(&title, created, &root);
    let hash_dir = root.join(&hash);
    let path = hash_dir.join(&title);
    let preset = root.join(PRESET_DIR);
    if preset.is_dir() {
        copy_dir_recursive(&preset, &path)
            .with_context(|| format!("could not copy preset from {}", preset.display()))?;
    } else {
        fs::create_dir_all(&path)?;
    }
    let conversation = Conversation {
        hash,
        title,
        icon: DEFAULT_ICON.to_owned(),
        engine,
        pinned: false,
        started: false,
        created,
        hash_dir,
        path,
    };
    write_metadata(&conversation)?;
    Ok(conversation)
}

pub fn update_pin(conversation: &Conversation, pinned: bool) -> Result<()> {
    let mut updated = conversation.clone();
    updated.pinned = pinned;
    write_metadata(&updated)
}

pub fn update_engine(conversation: &Conversation, engine: Engine) -> Result<()> {
    let mut updated = conversation.clone();
    updated.engine = engine;
    write_metadata(&updated)
}

pub fn mark_started(conversation: &Conversation) -> Result<()> {
    let mut updated = conversation.clone();
    updated.started = true;
    write_metadata(&updated)
}

pub fn update_icon(conversation: &Conversation, icon: &str) -> Result<()> {
    let mut updated = conversation.clone();
    updated.icon = if icon.trim().is_empty() {
        DEFAULT_ICON.to_owned()
    } else {
        icon.trim().chars().take(2).collect()
    };
    write_metadata(&updated)
}

pub fn rename_conversation(conversation: &Conversation, title: &str) -> Result<()> {
    let title = title.trim();
    validate_name(title)?;
    if title == conversation.title {
        return Ok(());
    }
    let destination = conversation.hash_dir.join(title);
    if destination.exists() {
        bail!("a conversation named '{title}' already exists");
    }
    fs::rename(&conversation.path, &destination)?;
    let mut updated = conversation.clone();
    updated.title = title.to_owned();
    updated.path = destination;
    write_metadata(&updated)
}

pub fn trash_conversation(conversation: &Conversation) -> Result<()> {
    trash::delete(&conversation.hash_dir).with_context(|| {
        format!(
            "could not move {} to trash",
            conversation.hash_dir.display()
        )
    })?;
    Ok(())
}

pub fn open_by_hash(hash: &str) -> Result<()> {
    let conversation = load_conversations()?
        .into_iter()
        .find(|conversation| conversation.hash == hash || conversation.hash.starts_with(hash))
        .ok_or_else(|| anyhow!("no conversation matches hash '{hash}'"))?;
    crate::sessions::open_session(&conversation)
}

fn write_metadata(conversation: &Conversation) -> Result<()> {
    let contents = format!(
        "title={}\nicon={}\nengine={}\npinned={}\nstarted={}\ncreated={}\n",
        conversation.title,
        conversation.icon.replace('\n', " "),
        conversation.engine.label().to_lowercase(),
        conversation.pinned,
        conversation.started,
        conversation.created
    );
    fs::write(conversation.hash_dir.join(META_FILE), contents)
        .with_context(|| format!("could not save metadata for {}", conversation.title))
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
        || !matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
        || Path::new(name).components().count() != 1
    {
        bail!("conversation titles must be a single safe directory name");
    }
    Ok(())
}

fn default_title() -> String {
    Command::new("date")
        .args(["+%Y%m%d-%H%M"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("chat-{}", unix_now()))
}

pub fn created_label(created: u64) -> String {
    let timestamp = format!("@{created}");
    Command::new("date")
        .args(["-d", &timestamp, "+%Y-%m-%d %H:%M"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| created.to_string())
}

fn short_hash(title: &str, created: u64, root: &Path) -> String {
    let mut counter = 0;
    loop {
        let mut hasher = DefaultHasher::new();
        title.hash(&mut hasher);
        created.hash(&mut hasher);
        std::process::id().hash(&mut hasher);
        counter.hash(&mut hasher);
        let candidate = format!("{:08x}", hasher.finish() as u32);
        if !root.join(&candidate).exists() {
            return candidate;
        }
        counter += 1;
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

fn directory_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hash_is_short_hex() {
        let hash = short_hash("test", 1, Path::new("/definitely-not-real"));
        assert_eq!(hash.len(), 8);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
    #[test]
    fn rejects_paths() {
        assert!(validate_name("../outside").is_err());
        assert!(validate_name("line\nbreak").is_err());
    }
}
