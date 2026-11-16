"""Multi-agent feature/bugfix -> test -> validate -> PR workflow for kingi.

Invoke via Antigravity `run_workflow` with ScriptPath pointing to this file:
  Args:
    task (str, required): Feature, bugfix, or refactor specification.
    branch (str, optional): Feature branch name override.
    create_pr (bool, optional): Push and open a GitHub PR if validation passes (default: false).
    draft_pr (bool, optional): Open PR as draft when create_pr is true (default: true).
    max_fix_rounds (int, optional): Max implementer fix rounds after validation failure (default: 2).
"""

import json

task = (args.get("task") or "").strip()
if not task:
    raise ValueError("Missing required Args field: 'task'")

branch_override = (args.get("branch") or "").strip()
create_pr = bool(args.get("create_pr", False))
draft_pr = bool(args.get("draft_pr", True))
max_fix_rounds = int(args.get("max_fix_rounds", 2))

PLAN_SCHEMA = {
    "type": "object",
    "properties": {
        "branch_name": {"type": "string"},
        "summary": {"type": "string"},
        "slices": {
            "type": "array",
            "minItems": 1,
            "items": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "files": {"type": "array", "items": {"type": "string"}},
                    "instructions": {"type": "string"},
                },
                "required": ["name", "files", "instructions"],
            },
        },
        "test_focus": {"type": "array", "items": {"type": "string"}},
        "docs_needed": {"type": "boolean"},
    },
    "required": ["branch_name", "summary", "slices", "test_focus", "docs_needed"],
}

IMPL_SCHEMA = {
    "type": "object",
    "properties": {
        "slice_name": {"type": "string"},
        "branch": {"type": "string"},
        "modified_files": {"type": "array", "items": {"type": "string"}},
        "summary": {"type": "string"},
    },
    "required": ["slice_name", "branch", "modified_files", "summary"],
}

VALIDATE_SCHEMA = {
    "type": "object",
    "properties": {
        "approved": {"type": "boolean"},
        "blockers": {"type": "array", "items": {"type": "string"}},
        "summary": {"type": "string"},
    },
    "required": ["approved", "blockers", "summary"],
}

PR_SCHEMA = {
    "type": "object",
    "properties": {
        "branch": {"type": "string"},
        "commit_sha": {"type": "string"},
        "pr_url": {"type": "string"},
        "summary": {"type": "string"},
    },
    "required": ["branch", "commit_sha", "pr_url", "summary"],
}

phase("Plan")
plan = await agent(
    prompt=f"""You are the Architect for `kingi`. Read `AGENTS.md` and inspect only the relevant files for this task:

TASK:
{task}

Produce an implementation plan split into non-overlapping file slices (use 1 slice if changes are tightly coupled in one module, or 2+ slices when `src/core/` and `src/ui/` can proceed independently).
Suggested branch override: {branch_override or "(derive feat/<slug> or fix/<slug>)"}.""",
    schema=PLAN_SCHEMA,
    role="Architect / Planner",
    workspace="inherit",
)
branch_name = branch_override or plan["branch_name"]
log(
    f"Plan ready on branch '{branch_name}' with {len(plan['slices'])} slice(s): {plan['summary']}"
)

phase("Implement")
slices = plan["slices"]
if len(slices) == 1:
    s = slices[0]
    impl_res = await agent(
        prompt=f"""Follow `AGENTS.md` and `.agents/skills/kingi-implement/SKILL.md`.
Create or switch to branch `{branch_name}` (if not already on it).
Implement slice `{s["name"]}` owning files {s["files"]}:

{s["instructions"]}

Task context: {task}
Docs/help update needed: {plan["docs_needed"]}.
Write unit tests, format modified `.rs` files with `rustfmt --edition 2024`, and run `cargo test` before finishing.""",
        schema=IMPL_SCHEMA,
        role=f"Implement {s['name']}",
        workspace="inherit",
    )
    log(f"Single-slice implementation complete: {impl_res['summary']}")
else:
    impl_results = await parallel(
        slices,
        lambda s: agent(
            prompt=f"""Follow `AGENTS.md` and `.agents/skills/kingi-implement/SKILL.md`.
You are working in an isolated shared workspace. Create a temporary branch `slice/{s["name"]}` from HEAD, implement ONLY your owned files {s["files"]}, and do not touch files owned by other slices.

Instructions:
{s["instructions"]}

Task context: {task}
Write unit tests for your changes, format your `.rs` files with `rustfmt --edition 2024`, run `cargo test`, and commit your slice to `slice/{s["name"]}`.""",
            schema=IMPL_SCHEMA,
            role=f"Implement {s['name']}",
            workspace="share",
        ),
    )
    branches_info = json.dumps(impl_results, indent=2)
    integ = await agent(
        prompt=f"""Create/checkout `{branch_name}` in the main workspace and merge/cherry-pick the slice branches cleanly:
{branches_info}

Resolve any minor integration glue (such as `src/ui/help.rs` or `README.md` if docs_needed={plan["docs_needed"]}), format touched `.rs` files, and ensure `cargo test` passes.""",
        schema={"type": "string"},
        role="Integrate slices",
        workspace="inherit",
    )
    log(f"Integrated {len(impl_results)} slices: {integ}")

phase("Test & Validate")
test_summary = await agent(
    prompt=f"""Follow `AGENTS.md`. Inspect the current diff against `master` (or `HEAD`) for task:
{task}

Focus areas from Architect: {plan["test_focus"]}

Add any missing deterministic `#[test]` unit tests (edge cases, error paths, Maildir/thread/UI state transitions) without altering production behavior unless fixing a clear testability hook. Format modified `.rs` files and confirm `cargo test` passes.""",
    schema={"type": "string"},
    role="Test Engineer",
    workspace="inherit",
)
log(f"Test pass: {test_summary}")

validator = agent(
    prompt=f"""You are the read-only Validator for `kingi`. Follow `AGENTS.md` and `.agents/skills/kingi-validate/SKILL.md`.
NEVER edit source files. Run `.agents/scripts/validate.sh` and inspect the full diff for task:
{task}""",
    role="Validator / Reviewer",
    workspace="inherit",
)

verdict = await validator(
    "Run `.agents/scripts/validate.sh`, inspect the diff, and return approved=true only if all gates and code-quality checks pass.",
    schema=VALIDATE_SCHEMA,
)
log(f"Validation round 1: approved={verdict['approved']} | {verdict['summary']}")

for round_idx in range(1, max_fix_rounds + 1):
    if verdict["approved"]:
        break
    blockers_text = "\n".join(f"- {b}" for b in verdict["blockers"])
    log(f"Fix round {round_idx} for {len(verdict['blockers'])} blocker(s)")
    fix_summary = await agent(
        prompt=f"""Follow `AGENTS.md` and `.agents/skills/kingi-implement/SKILL.md`.
Address ALL validation blockers reported by the Validator:
{blockers_text}

Format modified `.rs` files and verify `.agents/scripts/validate.sh` passes.""",
        schema={"type": "string"},
        role=f"Fix blockers (round {round_idx})",
        workspace="inherit",
    )
    verdict = await validator(
        f"The implementer applied fixes:\n{fix_summary}\n\nRe-run `.agents/scripts/validate.sh`, re-inspect the diff, and return your updated verdict.",
        schema=VALIDATE_SCHEMA,
    )
    log(
        f"Validation round {round_idx + 1}: approved={verdict['approved']} | {verdict['summary']}"
    )

del validator

if not verdict["approved"]:
    raise RuntimeError(
        f"Validation failed after {max_fix_rounds} fix round(s): {verdict['blockers']}"
    )

if create_pr:
    phase("Pull Request")
    pr_res = await agent(
        prompt=f"""Follow `.agents/skills/kingi-pr/SKILL.md`.
Ensure we are on branch `{branch_name}`, commit all validated changes cleanly, push to `origin`, and open a {"draft " if draft_pr else ""}PR using `gh pr create {"--draft " if draft_pr else ""}`.
Task summary: {plan["summary"]}
Validation summary: {verdict["summary"]}""",
        schema=PR_SCHEMA,
        role="PR Shepherd",
        workspace="inherit",
    )
    log(f"PR created: {pr_res['pr_url']} ({pr_res['branch']} @ {pr_res['commit_sha']})")
    result = pr_res
else:
    log(
        f"Validated changes ready on branch '{branch_name}' (create_pr=false; skipped push/PR)."
    )
    result = {
        "branch": branch_name,
        "approved": True,
        "summary": verdict["summary"],
    }

result
