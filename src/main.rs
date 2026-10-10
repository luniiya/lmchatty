mod args;
mod config;
mod hyprland;
mod model;
mod sessions;
mod storage;
mod templates;
mod ui;
mod vim;

use anyhow::{bail, Result};
use config::Config;
use model::Mode;
use sessions::open_session;
use storage::{load_conversations, open_by_hash, rename_by_hash, rename_current};

fn main() {
    if let Err(error) = run() {
        report_error(&error.to_string());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    hyprland::sync_window_rule();

    let mode = args::parse()?;

    // No saved config yet: build a placeholder with no enabled agents.
    // `enabled_agents.is_empty()` is exactly the signal `ui::App` uses to
    // force the setup screen open (a real saved config always has at least
    // one agent) — everywhere except `--chat`, which has no interactive
    // fallback and just errors instead.
    let config = match config::load()? {
        Some(config) => config,
        None => Config {
            enabled_agents: Vec::new(),
            chats_dir: config::default_chats_dir()?,
        },
    };

    match mode {
        Mode::Rename { hash, title } => match hash {
            Some(hash) => rename_by_hash(&config, &hash, &title),
            None => rename_current(&config, &title),
        },
        Mode::Chat(hash) => {
            if config.enabled_agents.is_empty() {
                bail!("no coding agents are set up yet — run `lmchatty setup` first");
            }
            open_by_hash(&config, &hash)
        }
        page => {
            let conversations = if config.enabled_agents.is_empty() {
                Vec::new()
            } else {
                load_conversations(&config)?
            };
            if let Some(conversation) = ui::run_tui(page, conversations, config)? {
                open_session(&conversation)?;
            }
            Ok(())
        }
    }
}

fn report_error(message: &str) {
    sessions::log_event(format!("fatal-error {message}"));
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "lmchatty", "-u", "critical", "Error"])
        .arg(message)
        .status();
    eprintln!("✗ {message}");
}
