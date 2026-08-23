# lmchatty project vision

`lmchatty` is a conversational launcher for coding agents. It should feel like
a small, fast, Vim-friendly terminal application rather than an agent-folder
picker.

## Setup and configuration

- Settings live in `${XDG_CONFIG_HOME:-~/.config}/lmchatty/config.toml`: which
  coding agents are enabled and their rank (`enabled_agents`, an ordered
  list — position is launch priority), and the conversations root
  (`chats_dir`). This path is fixed and not itself configurable.
- Agents are opt-in per user, not a single global default: someone with only
  a Codex subscription enables just Codex; someone with several enables all
  of them and ranks the one they reach for first as #1. At least one agent
  must stay enabled.
- Setup is a page within the same TUI, not a separate program. On first run,
  if `config.toml` doesn't exist, it opens automatically before any other
  page and can't be left until saved (there's nothing to fall back to yet).
  Any other time, `c` opens it from Search or New, pre-filled with the
  current config; `s`/`n`/Esc leave without saving.
- The agent checklist: `j`/`k` moves the cursor, `Space`/`Enter` toggles an
  agent on/off, `J`/`K` moves the highlighted agent up/down for manual
  reordering. Enabling an agent moves it to the bottom of the enabled block
  (so it picks up the next rank); disabling one moves it to the top of the
  disabled block. Net effect: enabled agents stay grouped at the top, in the
  order you enabled them, without needing `J`/`K` just to promote something
  out of the disabled section — `J`/`K` is only for overriding that order.
  `Tab` switches to the chats-directory field, which is pre-filled with a
  suggested default (`${XDG_DATA_HOME:-~/.local/share}/lmchatty`) — accept
  it with Enter or clear and replace it.
- `lmchatty setup` also opens this page directly from the command line.

## Storage

- Conversations live under the configured `chats_dir` (see above).
- Each conversation has a short stable hash directory.
- The hash directory contains a directory named after the conversation title.
- The title directory is the working directory for the selected coding agent.
- Metadata is stored in the hash directory in `.lmchatty-meta`.
- Untitled conversations use a local timestamp such as `2026Aug21-1423`.
- Pinned conversations appear before unpinned conversations; lists are newest
  first within each group.
- Deleting a conversation always moves its hash directory to desktop trash.
- The existing `~/.config/agents` tree is legacy/project material and must not
  be silently moved or deleted during this migration.

Example (with the default `chats_dir` of `~/.local/share/lmchatty`):

```text
~/.local/share/lmchatty/
└── a1b2c3d4/
    ├── .lmchatty-meta
    └── Fix the parser/
```

## Pages and commands

- `lmchatty` and `lmchatty search` open the search page.
- `lmchatty new` opens the new-conversation page.
- `lmchatty setup` opens the setup page (see above).
- `/` jumps to the search page. `c` jumps to setup from Search or New.
- `--chat HASH` opens a conversation directly.

Every page shares one header: the app name on the left, right-aligned key
hints for the pages actually reachable from where you are, and a rule
underneath. On the mandatory first-run setup page the hints are replaced
with "first run", since nothing else is reachable yet.

The search page has a focused search field at the top by default. Search
filters conversation titles. Escape enters normal mode; `j`, `k`, `gg`, and `G`
navigate conversations. Enter opens the selected conversation. Space opens
actions such as rename, icon change, pin/unpin, and move to trash. `n` opens
the new-conversation page.

The new-conversation page asks for a coding agent — one button per *enabled*
agent, in rank order — then a title. Empty titles get the timestamp default.
Enter creates the directory and starts the chosen agent.

Text fields should support Vim-style insert/normal editing, including `i`,
`a`, `A`, `h/j/k/l`, `gg`, `G`, `0`, `$`, `x`, `D`, and undo where practical.

## Agent sessions

- Every conversation runs in its own Zellij session named `lmchatty-HASH`.
- The selected agent's command (`claude`, `codex`, or `opencode`) starts with
  the title directory as its current working directory.
- Reopening a conversation reattaches to or switches to its existing Zellij
  session.
- tmux is not part of the new lmchatty flow.

## Desktop shortcuts

Preserve the existing Claude app window rules and map the launcher keys as:

- Super+A: `lmchatty new`
- Super+Alt+A: Claude
- Super+Shift+A: `lmchatty search`

The exact terminal/app wrapper may vary with the Hyprland launcher setup, but
the three behaviors must remain distinct.

## Implementation direction

Use Ratatui with Crossterm and `tui-textarea` (or a maintained compatible
replacement) for the TUI and Vim-friendly fields. Keep storage operations
small, explicit, and testable. Prefer recoverable operations and never use a
recursive destructive delete for a conversation.
