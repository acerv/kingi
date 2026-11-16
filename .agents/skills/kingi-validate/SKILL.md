---
name: kingi-validate
description: >-
  Validate changes in kingi by running the deterministic validation gate and
  performing a strict read-only code review for regressions, LLM anti-patterns,
  core/ui boundary leaks, and missing tests.
---

# Kingi Validation & Code Review

Use this skill when verifying another agent's changes or reviewing a branch/PR before merge.

## Rules

- **Do not edit production code** during validation. Report concrete, file-and-line findings back to the implementer.

## Steps

1. **Run the Automated Gate**:
   ```sh
   .agents/scripts/validate.sh
   ```
   Confirm exit code `0`, zero test failures, zero clippy warnings in modified files, passing `rustfmt --check` on changed files, and zero hygiene errors.
2. **Audit the Diff (`git diff master...HEAD` or `git diff HEAD`)**:
   - **Correctness & Edge Cases**: Check empty Maildir folders, malformed headers/dates, missing `Message-ID`, UTF-8 slicing boundaries, and concurrent sync/send states.
   - **Code Quality**: Flag duplicated logic across branches, overly long functions added to `src/ui/app.rs`, silent error swallowing (`let _ = ...` on important I/O), and new `.unwrap()` / `.expect()` outside `#[cfg(test)]`.
   - **Test Coverage**: Verify new behavior and failure modes are tested deterministically without relaxed assertions or live network/user-dir dependencies.
   - **Docs & Help Parity**: Verify any new or modified keybindings/config options are updated in both `src/ui/help.rs` and `README.md`.
3. **Emit Verdict**:
   - Return `APPROVED` only when `.agents/scripts/validate.sh` passes and no blockers remain.
   - Otherwise return `CHANGES_REQUESTED` with numbered, actionable blockers citing exact file paths and line numbers.
