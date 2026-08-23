# lmchatty project vision

`lmchatty` is a conversational launcher for coding agents. It should feel like
a small, fast, Vim-friendly terminal application rather than an agent-folder
picker.

## Storage

- Conversations live under `~/.config/lmchatty`.
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

Example:

```text
~/.config/lmchatty/
└── a1b2c3d4/
    ├── .lmchatty-meta
    └── Fix the parser/
```

## Pages and commands

- `lmchatty` and `lmchatty search` open the search page.
- `lmchatty new` opens the new-conversation page.
- `/` jumps to the search page.
- `--chat HASH` opens a conversation directly.

The search page has a focused search field at the top by default. Search
filters conversation titles. Escape enters normal mode; `j`, `k`, `gg`, and `G`
navigate conversations. Enter opens the selected conversation. Space opens
actions such as rename, icon change, pin/unpin, and move to trash. `n` opens
the new-conversation page.

The new-conversation page asks for Claude or Codex, then a title. Empty titles
get the timestamp default. Enter creates the directory and starts the chosen
agent.

Text fields should support Vim-style insert/normal editing, including `i`,
`a`, `A`, `h/j/k/l`, `gg`, `G`, `0`, `$`, `x`, `D`, and undo where practical.

## Agent sessions

- Every conversation runs in its own Zellij session named `lmchatty-HASH`.
- The selected `claude` or `codex` command starts with the title directory as
  its current working directory.
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
