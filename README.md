# lmchatty
LLM chatting interface that works with any subscription with all conversations stored locally

## Install

Requires:
- [Rust/Cargo](https://rustup.rs)
- [Zellij](https://zellij.dev)
- Whichever coding agents you use, on your `PATH`: `claude`, `codex`,
  and/or `opencode`

```text
git clone https://github.com/luniiya/lmchatty.git
cd lmchatty
cargo install --path .
```

This installs `lmchatty` to cargo's bin dir (`~/.cargo/bin` by default).
Make sure it's on your `PATH`, then run:

```text
lmchatty
```

## Setup

The first run asks which coding agents you actually want (Claude, Codex,
OpenCode — enable only the ones you have, and reorder them with `J`/`K` to
rank your favorite first) and where conversations should live, pre-filled
with `~/.config/lmchattydata` — press Enter to accept it or type your own
path. This is saved to `~/.config/lmchatty/config.toml`. Press `c` from any page, or run
`lmchatty setup`, to change it later.

## Rename a conversation

On the search page, select a conversation and press `r`, click `[Rename]`,
or choose Rename from the Space actions menu. The title changes without moving
the conversation's working directory, so an active agent keeps its path.

From a shell inside a conversation, run `lmchatty rename --current "New title"`.
You can also run `lmchatty rename HASH "New title"` from elsewhere. Existing
conversations remain readable; their original directory name is used as the
display title if metadata has no explicit title.

### Hyprland

On Hyprland, setup will offer to add a window rule that opens lmchatty as a
compact floating side panel instead of a regular tiled window. It's
self-maintaining (regenerated on every launch to match whichever terminal
emulator you use), but it can only match a window by the title/class it had
when the window was first mapped — lmchatty can't rename itself into a match
after the fact. So whatever command launches lmchatty needs to open the
terminal with its window title set to `lmchatty`, e.g.:

```text
ghostty --title=lmchatty -e lmchatty new
```

Bind that (or your terminal's equivalent `--title`/`--class` flag) to a key
in your Hyprland config and the rule will apply from the next launch on.
