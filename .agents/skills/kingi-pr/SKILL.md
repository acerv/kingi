---
name: kingi-pr
description: >-
  Prepare a clean feature branch, commit validated changes, and open a GitHub
  pull request for kingi using gh CLI.
---

# Kingi PR Creation

Use this skill after validation passes and the user or workflow requests opening a PR.

## Steps

1. **Verify Clean Validation**:
   - Confirm `.agents/scripts/validate.sh` passed on the final state of the branch.
   - Check `git status` and `git diff` to ensure no scratch files, temporary fixtures, or unrelated edits are staged.
2. **Branch Hygiene**:
   - Never commit directly to `master` and never force-push.
   - If currently on `master`, create a descriptive branch (`feat/<slug>` or `fix/<slug>`).
3. **Commit**:
   - Group changes into focused, readable commits with imperative subject lines (matching repo history, e.g., `Add ...`, `Fix ...`, `Document ...`).
4. **Push & Open Draft PR (Default)**:
   - Push the feature branch: `git push -u origin <branch>`.
   - Create a draft PR (unless explicitly asked for a ready PR) using `.github/PULL_REQUEST_TEMPLATE.md` structure:
     ```sh
     gh pr create --draft --title "<concise title>" --body "..."
     ```
   - Include affected modules, tests added, and the exact `.agents/scripts/validate.sh` results observed.
5. **Report**:
   - Return the branch name, commit SHA(s), and PR URL.
