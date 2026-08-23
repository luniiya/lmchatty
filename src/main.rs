mod args;
mod model;
mod sessions;
mod storage;
mod ui;
mod vim;

use anyhow::Result;
use model::Mode;
use sessions::open_session;
use storage::{load_conversations, open_by_hash};

fn main() {
    if let Err(error) = run() {
        report_error(&error.to_string());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match args::parse()? {
        Mode::Chat(hash) => open_by_hash(&hash),
        page @ (Mode::Search | Mode::New) => {
            let conversations = load_conversations()?;
            if let Some(conversation) = ui::run_tui(page, conversations)? {
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
