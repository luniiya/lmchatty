use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Search,
    New,
    Chat(String),
    Setup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Claude,
    Codex,
    OpenCode,
}

impl Engine {
    /// Every coding agent lmchatty knows how to launch, in the order they're
    /// listed on the setup screen and the new-conversation page.
    pub const ALL: [Engine; 3] = [Engine::Claude, Engine::Codex, Engine::OpenCode];

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }

    /// Parses the lowercase form written by `command()` (also used as the
    /// on-disk value in `.lmchatty-meta` and `config.toml`).
    pub fn from_command(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|engine| engine.command().eq_ignore_ascii_case(value))
    }
}

#[derive(Debug, Clone)]
pub struct Conversation {
    pub hash: String,
    pub title: String,
    pub icon: String,
    pub engine: Engine,
    pub pinned: bool,
    pub started: bool,
    pub created: u64,
    pub hash_dir: PathBuf,
    pub path: PathBuf,
}
