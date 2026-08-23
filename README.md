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
with `~/lmchatty` — press Enter to accept it or type your own path. This is
saved to `~/.config/lmchatty/config.toml`. Press `c` from any page, or run
`lmchatty setup`, to change it later.
