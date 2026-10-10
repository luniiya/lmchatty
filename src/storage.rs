use crate::config::Config;
use crate::model::{Conversation, Engine};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Component, Path},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const DEFAULT_ICON: &str = "⬜";
const META_FILE: &str = ".lmchatty-meta";

/// Recursively copies `source` into `destination`, creating `destination`.
/// Used by `templates::copy_into` to seed a new conversation's working
/// directory from the picked template.
pub(crate) fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
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

pub fn load_conversations(config: &Config) -> Result<Vec<Conversation>> {
    let root = &config.chats_dir;
    fs::create_dir_all(root).context("could not create the chats directory")?;
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
    let mut directory = None;
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
            "directory" => directory = Some(value.to_owned()),
            "icon" if !value.is_empty() => icon = value.to_owned(),
            "engine" => {
                if let Some(parsed) = Engine::from_command(value) {
                    engine = parsed;
                }
            }
            "pinned" => pinned = value == "true",
            "started" => started = value == "true",
            "created" => created = value.parse().unwrap_or_default(),
            _ => {}
        }
    }
    // Older conversations used the title as their work-directory name.
    // If metadata has neither field, a single work directory is unambiguous.
    let directory = match directory.or_else(|| title.clone()) {
        Some(directory) => directory,
        None => {
            let mut directories = fs::read_dir(hash_dir)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()));
            let only = directories
                .next()
                .ok_or_else(|| anyhow!("conversation has no work directory"))?;
            if directories.next().is_some() {
                bail!("conversation has multiple work directories and no directory metadata");
            }
            only.file_name().to_string_lossy().into_owned()
        }
    };
    validate_name(&directory)?;
    let title = title.unwrap_or_else(|| directory.clone());
    validate_title(&title)?;
    let path = hash_dir.join(&directory);
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

pub fn create_conversation(
    config: &Config,
    engine: Engine,
    requested_title: &str,
    template: Option<&str>,
) -> Result<Conversation> {
    let root = &config.chats_dir;
    fs::create_dir_all(root)?;
    let title = if requested_title.trim().is_empty() {
        default_title()
    } else {
        requested_title.trim().to_owned()
    };
    validate_name(&title)?;
    let created = unix_now();
    let hash = short_hash(&title, created, root);
    let hash_dir = root.join(&hash);
    let path = hash_dir.join(&title);
    match template {
        Some(name) => crate::templates::copy_into(name, &path)?,
        None => fs::create_dir_all(&path)?,
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
    validate_title(title)?;
    if title == conversation.title {
        return Ok(());
    }
    let mut updated = conversation.clone();
    updated.title = title.to_owned();
    write_metadata(&updated)
}

pub fn rename_by_hash(config: &Config, hash: &str, title: &str) -> Result<()> {
    let matches = load_conversations(config)?
        .into_iter()
        .filter(|conversation| conversation.hash == hash || conversation.hash.starts_with(hash))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [conversation] => rename_conversation(conversation, title),
        [] => bail!("no conversation matches hash '{hash}'"),
        _ => bail!("hash '{hash}' matches multiple conversations"),
    }
}

pub fn rename_current(config: &Config, title: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    rename_for_path(config, &cwd, title)
}

fn rename_for_path(config: &Config, cwd: &Path, title: &str) -> Result<()> {
    let matches = load_conversations(config)?
        .into_iter()
        .filter(|conversation| cwd.starts_with(&conversation.path))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [conversation] => rename_conversation(conversation, title),
        [] => bail!("current directory is not inside a conversation"),
        _ => bail!("current directory matches multiple conversations"),
    }
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

pub fn open_by_hash(config: &Config, hash: &str) -> Result<()> {
    let conversation = load_conversations(config)?
        .into_iter()
        .find(|conversation| conversation.hash == hash || conversation.hash.starts_with(hash))
        .ok_or_else(|| anyhow!("no conversation matches hash '{hash}'"))?;
    crate::sessions::open_session(&conversation)
}

fn write_metadata(conversation: &Conversation) -> Result<()> {
    let contents = format!(
        "title={}\ndirectory={}\nicon={}\nengine={}\npinned={}\nstarted={}\ncreated={}\n",
        conversation.title,
        conversation.path.file_name().ok_or_else(|| anyhow!("conversation path has no directory name"))?.to_string_lossy(),
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

fn validate_title(title: &str) -> Result<()> {
    if title.is_empty() || title.chars().any(char::is_control) {
        bail!("conversation title must not be empty or contain control characters");
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn test_config() -> Config {
        let unique = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let chats_dir = std::env::temp_dir().join(format!(
            "lmchatty-storage-test-{}-{unique}",
            std::process::id()
        ));
        Config {
            enabled_agents: vec![Engine::Codex],
            chats_dir,
        }
    }
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

    #[test]
    fn rename_changes_only_title_and_preserves_work_directory() {
        let config = test_config();
        let conversation = create_conversation(&config, Engine::Codex, "Original", None).unwrap();
        let marker = conversation.path.join("keep.txt");
        fs::write(&marker, "untouched").unwrap();

        rename_conversation(&conversation, "Software Reimplementation").unwrap();
        let reloaded = load_conversations(&config).unwrap().remove(0);
        assert_eq!(reloaded.title, "Software Reimplementation");
        assert_eq!(reloaded.path, conversation.path);
        assert_eq!(fs::read_to_string(marker).unwrap(), "untouched");
        assert!(!conversation.hash_dir.join("Software Reimplementation").exists());
        assert!(fs::read_to_string(conversation.hash_dir.join(META_FILE))
            .unwrap()
            .contains("directory=Original\n"));
        fs::remove_dir_all(config.chats_dir).unwrap();
    }

    #[test]
    fn legacy_metadata_keeps_original_directory_after_rename() {
        let config = test_config();
        let hash_dir = config.chats_dir.join("legacy123");
        fs::create_dir_all(hash_dir.join("Old title")).unwrap();
        fs::write(hash_dir.join(META_FILE), "title=Old title\nengine=codex\n").unwrap();
        let old = load_conversations(&config).unwrap().remove(0);
        assert_eq!(old.path, hash_dir.join("Old title"));

        rename_conversation(&old, "New / display title").unwrap();
        let reloaded = load_conversations(&config).unwrap().remove(0);
        assert_eq!(reloaded.title, "New / display title");
        assert_eq!(reloaded.path, old.path);
        fs::remove_dir_all(config.chats_dir).unwrap();
    }

    #[test]
    fn missing_title_uses_directory_name_as_display_fallback() {
        let config = test_config();
        let hash_dir = config.chats_dir.join("legacy123");
        fs::create_dir_all(hash_dir.join("Workspace name")).unwrap();
        fs::write(hash_dir.join(META_FILE), "engine=codex\n").unwrap();
        let conversation = load_conversations(&config).unwrap().remove(0);
        assert_eq!(conversation.title, "Workspace name");
        assert_eq!(conversation.path, hash_dir.join("Workspace name"));

        rename_by_hash(&config, "legacy", "Programmatic title").unwrap();
        let updated = load_conversations(&config).unwrap().remove(0);
        assert_eq!(updated.title, "Programmatic title");
        assert_eq!(updated.path, conversation.path);
        fs::remove_dir_all(config.chats_dir).unwrap();
    }

    #[test]
    fn explicit_directory_works_without_title() {
        let config = test_config();
        let hash_dir = config.chats_dir.join("legacy123");
        fs::create_dir_all(hash_dir.join("Original")).unwrap();
        fs::write(hash_dir.join(META_FILE), "directory=Original\n").unwrap();
        let conversation = load_conversations(&config).unwrap().remove(0);
        assert_eq!(conversation.title, "Original");
        fs::remove_dir_all(config.chats_dir).unwrap();
    }

    #[test]
    fn current_path_rename_selects_containing_conversation() {
        let config = test_config();
        let first = create_conversation(&config, Engine::Codex, "First", None).unwrap();
        let second = create_conversation(&config, Engine::Codex, "Second", None).unwrap();
        let nested = first.path.join("nested");
        fs::create_dir_all(&nested).unwrap();
        rename_for_path(&config, &nested, "Selected by path").unwrap();
        let conversations = load_conversations(&config).unwrap();
        assert_eq!(
            conversations.iter().find(|item| item.hash == first.hash).unwrap().title,
            "Selected by path"
        );
        assert_eq!(
            conversations.iter().find(|item| item.hash == second.hash).unwrap().title,
            "Second"
        );
        fs::remove_dir_all(config.chats_dir).unwrap();
    }

    #[test]
    fn invalid_display_title_cannot_change_metadata() {
        let config = test_config();
        let conversation = create_conversation(&config, Engine::Codex, "Original", None).unwrap();
        for title in ["", "   ", "bad\nname"] {
            assert!(rename_conversation(&conversation, title).is_err());
        }
        assert_eq!(load_conversations(&config).unwrap()[0].title, "Original");
        fs::remove_dir_all(config.chats_dir).unwrap();
    }
}
