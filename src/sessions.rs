use crate::model::{Conversation, Engine};
use anyhow::{bail, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn open_session(conversation: &Conversation) -> Result<()> {
    let name = format!("lmchatty-{}", conversation.hash);
    let inside_zellij = std::env::var_os("ZELLIJ_SESSION_NAME").is_some();

    log_event(format!(
        "open hash={} title={:?} engine={} session={} path={}",
        conversation.hash,
        conversation.title,
        conversation.engine.command(),
        name,
        conversation.path.display()
    ));

    if !zellij_has_session(&name)? {
        create_session(&name, &conversation.path, conversation.engine, conversation.started)?;
        if !conversation.started {
            if let Err(error) = crate::storage::mark_started(conversation) {
                log_event(format!("mark-started-failed session={name} error={error:#}"));
            }
        }
    }

    let result = if inside_zellij {
        run_zellij(["action", "switch-session", &name].as_slice())
    } else {
        run_zellij(["attach", &name].as_slice())
    };

    match &result {
        Ok(()) => log_event(format!("session-ready session={name}")),
        Err(error) => log_event(format!("session-failed session={name} error={error:#}")),
    }
    result
}

fn create_session(name: &str, path: &Path, engine: Engine, resume: bool) -> Result<()> {
    let cwd = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("conversation path is not valid UTF-8"))?;
    let layout = write_layout_file(name, engine, resume, cwd)?;
    let layout_string = layout
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("lmchatty layout path is not valid UTF-8"))?;
    let mut command = zellij_command();
    command
        .args([
            "attach",
            "--create-background",
            name,
            "options",
            "--default-layout",
        ])
        .arg(layout_string)
        .args(["--default-cwd", cwd, "--session-serialization", "false"]);

    log_event(format!(
        "create-background session={name} cwd={cwd} layout={layout_string} engine={} resume={resume}",
        engine.command()
    ));
    let status = command
        .status()
        .context("zellij is required to open a conversation")?;
    log_event(format!("create-status session={name} status={status}"));
    if !status.success() {
        bail!("zellij failed to create the conversation session: {status}");
    }
    Ok(())
}

fn zellij_has_session(name: &str) -> Result<bool> {
    let output = zellij_command()
        .args(["list-sessions", "--no-formatting"])
        .stderr(Stdio::piped())
        .output()
        .context("zellij is required to open a conversation")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    log_event(format!(
        "list-sessions status={} stdout={stdout:?} stderr={stderr:?}",
        output.status
    ));

    if !output.status.success() {
        bail!("zellij could not list sessions: {}", stderr.trim());
    }

    let prefix = format!("{name} ");
    Ok(stdout
        .lines()
        .map(str::trim)
        .any(|line| !line.contains("(EXITED)") && (line == name || line.starts_with(&prefix))))
}

fn run_zellij(arguments: &[&str]) -> Result<()> {
    log_event(format!("exec zellij {}", arguments.join(" ")));
    let status = zellij_command()
        .args(arguments)
        .status()
        .context("failed to run zellij")?;
    log_event(format!("exec-status status={status}"));
    if !status.success() {
        bail!("zellij exited with status {status}");
    }
    Ok(())
}

fn zellij_command() -> Command {
    let mut command = Command::new("zellij");
    command.args(["--config", config_path_string()]);
    command
}

fn config_path_string() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/zellij-config.kdl")
}

/// Resume flags per engine, used once a conversation has been opened
/// before. Claude's `--resume` and Codex's `resume` (no `--last`) both open
/// an interactive picker over past sessions in the current directory,
/// rather than silently continuing whichever was most recent. OpenCode has
/// no such single-command picker (confirmed against its own docs — see
/// CLAUDE.md), so it keeps `--continue` (auto-continues the last session)
/// as the best available option.
fn resume_args(engine: Engine) -> &'static [&'static str] {
    match engine {
        Engine::Claude => &["--resume"],
        Engine::Codex => &["resume"],
        Engine::OpenCode => &["--continue"],
    }
}

/// `zellij attach --create-background ... options --default-cwd` does not
/// reliably apply to a custom `--default-layout`'s panes (they were seen
/// spawning in $HOME instead), so the cwd is embedded directly in the
/// generated layout instead of relied on via CLI options.
fn layout_kdl(engine: Engine, resume: bool, cwd: &str) -> String {
    let command = engine.command();
    let cwd = kdl_escape(cwd);
    let args_block = if resume {
        let quoted = resume_args(engine)
            .iter()
            .map(|arg| format!("\"{arg}\""))
            .collect::<Vec<_>>()
            .join(" ");
        format!(" {{\n        args {quoted}\n    }}")
    } else {
        String::new()
    };
    format!(
        "layout {{\n    cwd \"{cwd}\"\n    pane command=\"{command}\"{args_block}\n    pane size=2 borderless=true {{\n        plugin location=\"zellij:status-bar\"\n    }}\n}}\n"
    )
}

fn kdl_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_layout_file(session_name: &str, engine: Engine, resume: bool, cwd: &str) -> Result<PathBuf> {
    let dir = state_dir()?.join("layouts");
    fs::create_dir_all(&dir).context("could not create the lmchatty layouts directory")?;
    // Keyed by session name (unique per conversation) rather than just
    // engine+resume, so two conversations opened around the same time can't
    // race each other overwriting a shared layout file.
    let file_name = format!(
        "{session_name}-{}-{}.kdl",
        engine.command(),
        if resume { "resume" } else { "new" }
    );
    let path = dir.join(file_name);
    fs::write(&path, layout_kdl(engine, resume, cwd))
        .with_context(|| format!("could not write layout to {}", path.display()))?;
    Ok(path)
}

pub fn log_event(message: impl AsRef<str>) {
    let Some(path) = log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let _ = writeln!(file, "{timestamp} {}", message.as_ref());
}

fn log_path() -> Option<PathBuf> {
    state_dir().ok().map(|dir| dir.join("lmchatty.log"))
}

fn state_dir() -> Result<PathBuf> {
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state_home).join("lmchatty"));
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/state/lmchatty"))
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}
