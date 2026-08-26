use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_NAME: &str = "default";
const DEFAULT_AGENTS_MD: &str = r#"# Agent instructions

- Whenever searching online, if the search tool fails, just use curl from
  this computer.

## Persistent Memory Protocol

Your working directory is not just a workspace — it is your long-term memory.
Everything you learn must survive context compaction and session restarts by
living on disk, not in the conversation.

### Layout

- `memory/INDEX.md` — one line per note: `- [title](file.md) — one-line hook`
- `memory/<topic>/<slug>.md` — one atomic note per file. kebab-case filenames.
  Use subdirs to group by domain once a topic has 3+ notes (e.g. `memory/user/`,
  `memory/project-x/`, `memory/reference/`).

### Note format

Every note starts with frontmatter:

```markdown
---
name: <slug>
description: <one line, used to judge relevance without opening the file>
type: fact | decision | project | reference
---
```

Body: the fact itself, dated. Cross-link related notes with `[[slug]]`.

### The two hard rules

1. **Search before you answer.** If you're not certain, check `INDEX.md`
   first, then read the matching note(s), before answering. Don't guess or
   re-derive something that's already written down. Only fall back to an
   external source (web, filesystem, asking the user) if the workspace has
   nothing.
2. **Write immediately after you learn.** The moment new info surfaces —
   user states a fact, you figure something out, an external lookup
   resolves something — write or update the note *right then*, not
   "later." If it isn't on disk, it doesn't count as known.

### Non-negotiables

- **Date every fact.** `*(as of YYYY-MM-DD)*` on every claim that could go
  stale. No exceptions.
- **Never overwrite, always append.** If something changes, add the new
  fact below the old one with its own date — don't delete or edit the old
  line. History must stay reconstructable.
- **Keep notes atomic and small.** One fact/decision per file. If a note is
  covering two unrelated things, split it.
- **Keep INDEX.md in sync.** Every new note gets a line the same turn it's
  created. INDEX.md is what gets skimmed each session — it must always be
  a true map of what's in `memory/`.
"#;

/// `${XDG_CONFIG_HOME:-~/.config}/lmchatty/templates`.
pub fn root() -> Result<PathBuf> {
    crate::config::templates_dir()
}

/// Same as `root()`, but degrades to a literal fallback string instead of
/// erroring — this is only ever used to build a human-readable hint, never
/// to actually touch the filesystem.
pub fn root_display() -> String {
    root()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "~/.config/lmchatty/templates".to_owned())
}

/// Lists available template names: `default` first (if present), the rest
/// alphabetically, `(none)` is *not* included here — callers add that
/// themselves since it's a UI concept, not a real directory.
///
/// Auto-seeds a starter `default` template (with an example `agents.md`) the
/// very first time this runs, i.e. when the templates directory doesn't
/// exist yet at all. If that seeding fails, or the directory exists but
/// turns out empty (e.g. a user deleted everything), this still returns an
/// empty list rather than failing — a missing template is never fatal to
/// creating a conversation — but logs a warning and fires a desktop
/// notification so the gap doesn't go unnoticed silently.
pub fn list() -> Vec<String> {
    match list_inner() {
        Ok(names) if names.is_empty() => {
            warn(&format!(
                "no templates found in {} (not even 'default') — new conversations \
                 will start empty until you add one",
                root_display()
            ));
            names
        }
        Ok(names) => names,
        Err(error) => {
            warn(&format!("could not read templates directory: {error:#}"));
            Vec::new()
        }
    }
}

fn list_inner() -> Result<Vec<String>> {
    let dir = root()?;
    if !dir.exists() {
        seed_default(&dir)?;
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    sort_default_first(&mut names);
    Ok(names)
}

/// Sorts alphabetically, then — if present — moves `default` to the front
/// regardless of where it fell alphabetically.
fn sort_default_first(names: &mut Vec<String>) {
    names.sort();
    if let Some(position) = names.iter().position(|name| name == DEFAULT_NAME) {
        let default = names.remove(position);
        names.insert(0, default);
    }
}

fn seed_default(root: &Path) -> Result<()> {
    let default_dir = root.join(DEFAULT_NAME);
    fs::create_dir_all(&default_dir)
        .with_context(|| format!("could not create {}", default_dir.display()))?;
    fs::write(default_dir.join("agents.md"), DEFAULT_AGENTS_MD)
        .with_context(|| format!("could not seed {}/agents.md", default_dir.display()))
}

fn warn(message: &str) {
    crate::sessions::log_event(format!("template-warning {message}"));
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "lmchatty", "-u", "normal", "Templates"])
        .arg(message)
        .status();
}

/// Recursively copies the named template into `destination`. `destination`
/// must not already exist (mirrors `create_conversation`'s expectations).
pub fn copy_into(name: &str, destination: &Path) -> Result<()> {
    let source = root()?.join(name);
    crate::storage::copy_dir_recursive(&source, destination)
        .with_context(|| format!("could not copy template '{name}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_leads_rest_alphabetical() {
        let mut names: Vec<String> = ["zeta", "default", "alpha", "mid"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        sort_default_first(&mut names);
        assert_eq!(names, vec!["default", "alpha", "mid", "zeta"]);
    }

    #[test]
    fn alphabetical_when_no_default() {
        let mut names: Vec<String> = ["zeta", "alpha", "mid"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        sort_default_first(&mut names);
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
    }
}
