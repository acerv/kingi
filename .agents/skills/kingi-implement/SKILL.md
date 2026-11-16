---
name: kingi-implement
description: >-
  Implement features or bug fixes in the kingi Rust Maildir TUI client following
  core/ui boundaries, unit testing rules, and keybinding/documentation parity.
---

# Kingi Feature & Bugfix Implementation

Use this skill when implementing a new feature, refactoring a module, or fixing a bug in `kingi`.

## Workflow

1. **Locate & Read First**:
   - Identify whether the change belongs in `src/core/` (domain, Maildir, MIME, GPG, thread tree, config) or `src/ui/` (TUI state, rendering, keybindings, SMTP dispatch).
   - Keep `src/core/` completely free of terminal/UI crates (`ratatui`, `crossterm`, `edtui`, `arboard`, `crate::ui`).
   - Avoid bloating `src/ui/app.rs`; extract cohesive helpers or put focused logic in the appropriate `src/ui/*` submodule.
2. **Design & Implement**:
   - Preserve existing public signatures unless the task requires changing them.
   - Propagate errors with `anyhow::Context` (`?`) or surface them cleanly to `status_error`; never introduce `.unwrap()` or `.expect()` in production code.
   - Ensure any new `.rs` file starts with the GPL-3.0-or-later SPDX header required by `AGENTS.md`.
   - If keybindings or user-visible config options change, update `src/ui/help.rs` and `README.md`.
3. **Write Deterministic Tests**:
   - Add `#[test]` unit tests covering both normal paths and edge cases using temporary directories (never `~/.config/kingi`, `~/Mail`, or live SMTP/GPG agents).
4. **Format & Pre-Verify**:
   - Format modified `.rs` files with `rustfmt --edition 2024 <file>`.
   - Run `.agents/scripts/validate.sh` and confirm all checks pass before handing off to validation or PR creation.
