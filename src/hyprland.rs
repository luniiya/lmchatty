//! Best-effort Hyprland integration: styles lmchatty's own window as a
//! compact floating side panel, so a fresh install looks right without the
//! user hand-writing any window rules — this used to only work via a
//! personal rule in the original author's own Hyprland config, which a new
//! user obviously wouldn't have.
//!
//! As of Hyprland 0.55+, `hyprland.lua` (not `hyprland.conf`) is the real
//! upstream config format — confirmed against a live stock Hyprland 0.56.2
//! instance, not just docs. Two consequences that shaped this module:
//!
//! - `hyprctl dispatch`'s classic `dispatch NAME ARGS` string form is gone;
//!   it now evaluates its argument as a Lua expression (`hl.dispatch(...)`).
//!   Confirmed live: `hl.dsp.window.float({action="toggle"})` works fine as
//!   a runtime dispatch. But exact pixel size/position/opacity/rounding
//!   have no such live dispatcher — `hl.dsp.window.resize()` turned out to
//!   be the *interactive mouse-drag* resize, not a settable-exact-size call
//!   (confirmed by its error message when called with size args).
//! - Those properties are only settable via a declarative `hl.window_rule`,
//!   and — also confirmed live, by registering one via `hyprctl eval` against
//!   an already-open window and watching nothing happen — window rules only
//!   apply at window *map* time, never retroactively. So there is no
//!   runtime-only way to style lmchatty's own already-open window; the rule
//!   has to be in place *before* the window maps, i.e. sourced from
//!   `hyprland.lua` itself.
//!
//! Design that follows from that: lmchatty regenerates a small rule file at
//! `~/.config/lmchatty/hyprland.lua` on every launch (cheap, idempotent,
//! silent — never requires touching the user's own config), matching the
//! *current* terminal's window class (detected via `hyprctl activewindow`,
//! so it works regardless of which terminal emulator the user runs — no
//! guessing at a "default terminal"). Wiring that file into the user's real
//! `hyprland.lua` via one `dofile(...)` line is the one part that touches a
//! file lmchatty doesn't own, so it's consent-gated through the Setup
//! screen instead of done silently (see `needs_source_line`/
//! `write_source_line`, used from `ui::App::submit_setup`).
//!
//! **One requirement this can't remove**: the rule matches on `title`, but
//! (also confirmed live) it's matched once against the window's *initial*
//! title at map time, not re-evaluated on a later title change — so
//! `ui::run_tui`'s own `SetTitle(WINDOW_TITLE)` call (which does visibly
//! rename the window) cannot make an untitled launch match retroactively;
//! it only helps window-switcher/taskbar labeling, not this rule. The
//! terminal has to be launched with its window title already set to
//! `WINDOW_TITLE` (e.g. `ghostty --title=lmchatty -e lmchatty new`) for the
//! float/size/position rule to apply on that launch — same requirement the
//! original personal `windowrulev2` always had, just now enforced by
//! Hyprland's map-time matching instead of by hand-written config. README
//! setup instructions should say so.
//!
//! Everything here is cosmetic and best-effort: no-ops entirely off
//! Hyprland, and every `hyprctl` call is swallowed on failure.

use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Fixed window title lmchatty sets on its own terminal window (see
/// `ui::run_tui`'s `SetTitle`), so the rule below can match on it reliably
/// regardless of how the terminal/launcher was invoked.
pub const WINDOW_TITLE: &str = "lmchatty";

const WIDTH_FRACTION: f64 = 0.22;
const HEIGHT_FRACTION: f64 = 0.9;
/// Fraction of monitor width from the left edge to the window's left edge —
/// `1.0 - WIDTH_FRACTION` would butt it against the right edge exactly;
/// this leaves a couple percent of margin, matching what was verified to
/// look right live.
const MOVE_X_FRACTION: f64 = 0.76;
const TOP_PX: i64 = 100;
const OPACITY: &str = "0.94";

/// Call once at startup, before entering the TUI. No-ops entirely (no
/// subprocess spawned at all) if `HYPRLAND_INSTANCE_SIGNATURE` isn't set,
/// i.e. not running under Hyprland. Otherwise regenerates
/// `~/.config/lmchatty/hyprland.lua` to match the current terminal's window
/// class, and reloads Hyprland if the content actually changed.
pub fn sync_window_rule() {
    if env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return;
    }
    let _ = try_sync();
}

fn try_sync() -> Option<()> {
    let class = active_window_class()?;
    let snippet = render_snippet(&class);
    let path = crate::config::hyprland_snippet_path().ok()?;
    if fs::read_to_string(&path).ok().as_deref() == Some(snippet.as_str()) {
        return Some(()); // already up to date, nothing to reload
    }
    fs::create_dir_all(path.parent()?).ok()?;
    fs::write(&path, &snippet).ok()?;
    // Window rules only apply at window *map* time (verified live — they
    // don't retroactively restyle an already-open window), so reloading
    // here can't style *this* launch if this is the very first run; it
    // just makes sure the rule is live in time for the next one.
    let _ = run(&["reload"]);
    Some(())
}

/// Whether the Setup screen should offer to wire lmchatty's generated rule
/// file into the user's real Hyprland config. `None` means don't ask at
/// all (not running under Hyprland). `Some(false)` means already wired.
pub fn needs_source_line() -> Option<bool> {
    env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let entrypoint = hypr_entrypoint()?;
    let contents = fs::read_to_string(&entrypoint).ok()?;
    let marker = crate::config::hyprland_snippet_path().ok()?;
    Some(!contents.contains(&marker.to_string_lossy().into_owned()))
}

/// Appends a `dofile(...)` line sourcing lmchatty's generated rule file
/// into the user's `~/.config/hypr/hyprland.lua`, then reloads Hyprland.
/// Only call this after explicit user confirmation (Setup screen) — unlike
/// everything else in this module, it edits a file lmchatty doesn't own.
pub fn write_source_line() -> Result<()> {
    let entrypoint =
        hypr_entrypoint().context("~/.config/hypr/hyprland.lua not found")?;
    let snippet = crate::config::hyprland_snippet_path()?;
    let line = format!(
        "\n-- lmchatty window rule (added by lmchatty setup, see \
         ~/.config/lmchatty/hyprland.lua)\ndofile(\"{}\")\n",
        snippet.display()
    );
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&entrypoint)
        .with_context(|| format!("could not open {}", entrypoint.display()))?;
    file.write_all(line.as_bytes())
        .with_context(|| format!("could not write to {}", entrypoint.display()))?;
    let _ = run(&["reload"]);
    Ok(())
}

fn render_snippet(class: &str) -> String {
    format!(
        "-- Auto-generated by lmchatty (src/hyprland.rs) — do not hand-edit,\n\
         -- it's regenerated (and Hyprland reloaded) whenever the content would\n\
         -- differ. Safe to delete; lmchatty recreates it on next launch.\n\
         hl.window_rule({{\n\
         \x20\x20name = \"lmchatty\",\n\
         \x20\x20match = {{ class = \"^({class_pattern})$\", title = \"^({title})$\" }},\n\
         \x20\x20float = true,\n\
         \x20\x20decorate = false,\n\
         \x20\x20border_size = 0,\n\
         \x20\x20animation = \"slide right\",\n\
         \x20\x20opacity = \"{OPACITY}\",\n\
         \x20\x20size = \"(monitor_w*{WIDTH_FRACTION}) (monitor_h*{HEIGHT_FRACTION})\",\n\
         \x20\x20move = \"(monitor_w*{MOVE_X_FRACTION}) {TOP_PX}\",\n\
         \x20\x20rounding = 0,\n\
         }})\n",
        class_pattern = lua_regex_literal(class),
        title = WINDOW_TITLE,
    )
}

/// Escapes `raw` for embedding inside a Lua double-quoted string as a
/// Hyprland regex-match literal — e.g. `com.mitchellh.ghostty` becomes
/// `com\\.mitchellh\\.ghostty` (which Lua then un-escapes to the regex
/// `com\.mitchellh\.ghostty`, matching the class with its dots literal).
fn lua_regex_literal(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|'
            | '\\' => {
                out.push('\\');
                out.push('\\');
                out.push(ch);
            }
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out
}

/// Reads the active window's `class` from `hyprctl activewindow` — that's
/// lmchatty's own terminal window at the point this is called, so this is
/// how the generated rule targets "whatever terminal emulator is actually
/// running lmchatty" without needing to guess/look up a "default terminal".
fn active_window_class() -> Option<String> {
    let active = run_capture(&["activewindow"])?;
    plain_field(&active, "class:").map(str::to_owned)
}

/// `${XDG_CONFIG_HOME:-~/.config}/hypr/hyprland.lua`, if it exists — the
/// standard Hyprland 0.55+ Lua config entrypoint.
fn hypr_entrypoint() -> Option<PathBuf> {
    let dir = match env::var_os("XDG_CONFIG_HOME") {
        Some(config_home) => PathBuf::from(config_home).join("hypr"),
        None => PathBuf::from(env::var_os("HOME")?).join(".config/hypr"),
    };
    let path = dir.join("hyprland.lua");
    path.exists().then_some(path)
}

fn run(args: &[&str]) -> Option<()> {
    let status = Command::new("hyprctl")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    status.success().then_some(())
}

fn run_capture(args: &[&str]) -> Option<String> {
    let output = Command::new("hyprctl").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Extracts the value from a line that (after trimming) starts with `key`,
/// e.g. `plain_field(text, "class:")` on a line "\tclass: kitty".
fn plain_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix(key) {
            return Some(rest.trim());
        }
    }
    None
}
