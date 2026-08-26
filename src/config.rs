use crate::model::Engine;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    /// Which coding agents the user actually has and wants to launch —
    /// someone with only a Codex subscription enables just Codex, someone
    /// with everything enables all of them. Always non-empty once saved.
    pub enabled_agents: Vec<Engine>,
    pub chats_dir: PathBuf,
}

/// Always `${XDG_CONFIG_HOME:-~/.config}/lmchatty/config.toml`, independent
/// of `chats_dir` (which is user-chosen and lives in this file).
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

fn config_dir() -> Result<PathBuf> {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home).join("lmchatty"));
    }
    home_dir().map(|home| home.join(".config/lmchatty"))
}

/// `${XDG_CONFIG_HOME:-~/.config}/lmchatty/templates` — a directory of named
/// subdirectories, each one copied wholesale into a new conversation's
/// working directory when picked on the new-conversation page. See
/// `src/templates.rs`.
pub fn templates_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("templates"))
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

/// Suggested, pre-filled `chats_dir` for the setup screen:
/// `${XDG_DATA_HOME:-~/.local/share}/lmchatty`, the standard XDG location
/// for an app's own persistent user data (as opposed to `config_dir`'s
/// settings or `sessions::state_dir`'s logs). Just a suggestion — the setup
/// screen lets the user pick anywhere.
pub fn default_chats_dir() -> Result<PathBuf> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("lmchatty"));
    }
    home_dir().map(|home| home.join(".local/share/lmchatty"))
}

/// Loads `config.toml` if it exists. `None` means first run: the caller
/// should run interactive setup and then `save()` the result.
pub fn load() -> Result<Option<Config>> {
    let path = config_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()))
        }
    };
    parse(&contents)
        .map(Some)
        .with_context(|| format!("could not parse {}", path.display()))
}

impl Config {
    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let agents = self
            .enabled_agents
            .iter()
            .map(|agent| format!("\"{}\"", agent.command()))
            .collect::<Vec<_>>()
            .join(", ");
        let contents = format!(
            "enabled_agents = [{agents}]\nchats_dir = \"{}\"\n",
            escape(&self.chats_dir.display().to_string()),
        );
        fs::write(&path, contents).with_context(|| format!("could not write {}", path.display()))
    }
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Parses the small subset of TOML this config file uses (plain
/// `key = "value"` and `key = ["a", "b"]` lines) — hand-rolled to match the
/// rest of the codebase's plain key=value metadata files (see storage.rs)
/// instead of pulling in a TOML crate for two fields.
fn parse(contents: &str) -> Result<Config> {
    let mut enabled_agents = None;
    let mut chats_dir = None;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "enabled_agents" => {
                let agents: Vec<Engine> = parse_string_array(value)
                    .iter()
                    .filter_map(|name| Engine::from_command(name))
                    .collect();
                if !agents.is_empty() {
                    enabled_agents = Some(agents);
                }
            }
            "chats_dir" => {
                let dir = unquote(value);
                if !dir.is_empty() {
                    chats_dir = Some(PathBuf::from(dir));
                }
            }
            _ => {}
        }
    }
    Ok(Config {
        enabled_agents: enabled_agents
            .ok_or_else(|| anyhow!("config.toml is missing 'enabled_agents'"))?,
        chats_dir: chats_dir.ok_or_else(|| anyhow!("config.toml is missing 'chats_dir'"))?,
    })
}

fn parse_string_array(value: &str) -> Vec<String> {
    let inner = value.trim();
    let inner = inner
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(inner);
    inner
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(unquote)
        .collect()
}

fn unquote(value: &str) -> String {
    let trimmed = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    trimmed.replace("\\\"", "\"").replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_save_format() {
        let contents = "enabled_agents = [\"claude\", \"opencode\"]\nchats_dir = \"/home/x/li \\\"lm\\\"\"\n";
        let config = parse(contents).unwrap();
        assert_eq!(config.enabled_agents, vec![Engine::Claude, Engine::OpenCode]);
        assert_eq!(config.chats_dir, PathBuf::from("/home/x/li \"lm\""));
    }

    #[test]
    fn missing_fields_error() {
        assert!(parse("enabled_agents = [\"codex\"]\n").is_err());
        assert!(parse("chats_dir = \"/x\"\n").is_err());
    }

    #[test]
    fn save_then_parse_round_trips() {
        let config = Config {
            enabled_agents: vec![Engine::Codex, Engine::OpenCode],
            chats_dir: PathBuf::from("/home/x/lmchatty"),
        };
        let agents = config
            .enabled_agents
            .iter()
            .map(|agent| format!("\"{}\"", agent.command()))
            .collect::<Vec<_>>()
            .join(", ");
        let contents = format!(
            "enabled_agents = [{agents}]\nchats_dir = \"{}\"\n",
            config.chats_dir.display()
        );
        let parsed = parse(&contents).unwrap();
        assert_eq!(parsed.enabled_agents, config.enabled_agents);
        assert_eq!(parsed.chats_dir, config.chats_dir);
    }
}
