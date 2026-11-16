# Multi-Agent Workflows for `kingi`

This directory contains Antigravity orchestration workflows and playbooks tailored for `kingi`.

## 1. Automated Pipeline (`feature_pr.py` via `run_workflow`)

`feature_pr.py` runs a structured **Plan -> Implement -> Test -> Validate/Fix Loop -> PR** pipeline using the project rules in [AGENTS.md](../../AGENTS.md) and the validation gate in [.agents/scripts/validate.sh](../scripts/validate.sh).

### Arguments (`Args`)
- `task` *(string, required)*: What to implement or fix.
- `branch` *(string, optional)*: Target feature branch name (defaults to `feat/...` or `fix/...` chosen by the Architect).
- `create_pr` *(bool, optional, default `false`)*: Commit, push, and create a PR via `gh pr create` once validation passes.
- `draft_pr` *(bool, optional, default `true`)*: Open the PR as a draft when `create_pr` is `true`.
- `max_fix_rounds` *(int, optional, default `2`)*: Maximum automated fix/re-validate iterations.

## 2. Interactive Swarm (`/teamwork`)

For larger features spanning multiple modules (for example, `src/core/` + `src/ui/` + `README.md`), start a swarm with `/teamwork` using these non-overlapping roles:
- `core-impl` (`workspace:"share"`): owns `src/core/*`
- `ui-impl` (`workspace:"share"`): owns `src/ui/*` and `src/ui/help.rs`
- `test-eng` (`workspace:"inherit"` after merge, or owns dedicated test cases)
- `verifier` (`workspace:"inherit"`, read-only): runs `.agents/scripts/validate.sh` and audits diffs using `.agents/skills/kingi-validate/SKILL.md`
- `pr-shepherd` (`workspace:"inherit"`): prepares commits and draft PR via `.agents/skills/kingi-pr/SKILL.md`
