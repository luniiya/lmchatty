use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Search,
    New,
    Chat(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Claude,
    Codex,
}

impl Engine {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
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
