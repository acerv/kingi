# Repository Guidelines for `kingi`

`kingi` is a fast, Vim-inspired Maildir TUI email client written in Rust (edition 2024). Every agent, subagent, workflow step, and swarm worker operating in this repository must follow these rules.

## Architecture & Module Boundaries

- **`src/core/`** (`address`, `attachment`, `config`, `date`, `gpg`, `maildir`, `thread`):
  Pure domain, Maildir, MIME, GPG, thread-tree (`O(n)`), and config logic.
  - **Boundary rule**: `src/core/` must remain UI-agnostic. Never import `ratatui`, `crossterm`, `edtui`, `arboard`, or `crate::ui::*` from `src/core/`.
- **`src/ui/`** (`app`, `compose`, `draw`, `editor`, `email`, `help`, `markers`, `send`, `threads`, `utils`):
  TUI state, event handling, rendering, compose editor, and SMTP dispatch.
  - `src/ui/app.rs` is already large (~3,000 lines). Place focused view/state/helper logic in the relevant `src/ui/*` submodule (or extract cohesive helpers) instead of bloating `app.rs`.
- **`src/main.rs`**: CLI entrypoint (`kingi`, `kingi init [account]`, `-h`/`--help`).

## Rust & Code Quality Standards

1. **License Header**: Every `.rs` file under `src/` must start with:
   ```rust
   // SPDX-License-Identifier: GPL-3.0-or-later
   // Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
   ```
2. **Error Handling**: Never add `.unwrap()` or `.expect(...)` in non-test production code. Propagate errors with `anyhow::Context` (`?`) or surface them cleanly to the UI (`status_error`).
3. **No Duplicated Logic**: Extract shared helpers rather than copying branches across key handlers, Maildir operations, or render functions. Keep functions small with shallow nesting and early returns.
4. **Warnings & Formatting**:
   - Do not introduce any `cargo clippy` warnings in files you touch.
   - Format modified code cleanly with `rustfmt` (avoid reformatting unrelated untouched files unless requested).
5. **Comments & Docstrings**: Document public structs, enums, and functions with concise `///` docstrings; comment non-obvious invariants (why, not what). Preserve unrelated existing comments.

## Testing Rules

- Every feature or bug fix must include deterministic `#[test]` unit tests verified with `cargo test`.
- Tests must be self-contained:
  - Use temporary directories (and clean them up) rather than `~/.config/kingi` or `~/Mail`.
  - Never perform live SMTP connections, spawn external `mbsync` processes against user data, or require an interactive `gpg-agent`.
- Never relax, skip, or delete existing assertions to make a failing test pass.

## Documentation & Keybinding Parity

- Whenever a keybinding, mode action, or user-facing config option is added or changed:
  - Update the help screen in `src/ui/help.rs`.
  - Update the keybinding/configuration tables in `README.md`.

## Multi-Agent & Git Hygiene

- **Role separation**: Validator/Reviewer agents must not edit production code directly; they run `.agents/scripts/validate.sh`, review diffs, and report concrete blockers back to the Implementer.
- **No direct pushes to `master`**: Always work on a feature branch (e.g., `feat/...`, `fix/...`) before opening a PR. Never run `git push --force`.
