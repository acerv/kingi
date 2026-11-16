#!/usr/bin/env bash
# Deterministic validation gate for kingi multi-agent workflows and local dev.
# Usage:
#   .agents/scripts/validate.sh            # Standard gate (diff-scoped fmt/clippy + full build & tests)
#   .agents/scripts/validate.sh --strict   # Full-repo fmt --check + clippy -D warnings

set -euo pipefail

STRICT=0
BASE_REF="master"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --strict)
      STRICT=1
      shift
      ;;
    --base)
      BASE_REF="${2:-master}"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 [--strict] [--base <git-ref>]"
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 2
      ;;
  esac
done

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

echo "==> [1/6] Checking changed Rust files vs ${BASE_REF} / HEAD"
CHANGED_RS="$(
  {
    git diff --name-only --diff-filter=ACMRTUXB HEAD -- '*.rs' 2>/dev/null || true
    git ls-files --others --exclude-standard -- '*.rs' 2>/dev/null || true
    if git rev-parse --verify "$BASE_REF" >/dev/null 2>&1; then
      git diff --name-only --diff-filter=ACMRTUXB "${BASE_REF}...HEAD" -- '*.rs' 2>/dev/null || true
    fi
  } | sort -u
)"

echo "==> [2/6] Running cargo build --all-targets"
cargo build --all-targets

echo "==> [3/6] Running cargo test"
cargo test

echo "==> [4/6] Checking formatting (rustfmt)"
if [[ "$STRICT" -eq 1 ]]; then
  cargo fmt --all -- --check
elif [[ -n "$CHANGED_RS" ]]; then
  while IFS= read -r file; do
    [[ -f "$file" ]] || continue
    rustfmt --edition 2024 --check "$file"
  done <<< "$CHANGED_RS"
else
  echo "No changed .rs files; skipping diff-scoped rustfmt check (use --strict for full repo)."
fi

echo "==> [5/6] Running cargo clippy"
if [[ "$STRICT" -eq 1 ]]; then
  cargo clippy --all-targets -- -D warnings
else
  CLIPPY_LOG="$(mktemp)"
  trap 'rm -f "$CLIPPY_LOG"' EXIT
  cargo clippy --all-targets --message-format=short 2>&1 | tee "$CLIPPY_LOG"
  if [[ -n "$CHANGED_RS" ]]; then
    while IFS= read -r file; do
      [[ -n "$file" ]] || continue
      if grep -qE "^${file}:[0-9]+:[0-9]+: (warning|error):" "$CLIPPY_LOG"; then
        echo "ERROR: Clippy reported warnings/errors in modified file: $file" >&2
        grep -E "^${file}:[0-9]+:[0-9]+: (warning|error):" "$CLIPPY_LOG" >&2
        exit 1
      fi
    done <<< "$CHANGED_RS"
  fi
fi

echo "==> [6/6] Running kingi repository hygiene checks"
python3 - "$BASE_REF" <<'PY'
import pathlib
import re
import subprocess
import sys

base_ref = sys.argv[1]
errors = []
warnings = []

# 1. Verify SPDX license headers on all src/**/*.rs files
for path in sorted(pathlib.Path("src").rglob("*.rs")):
    first_lines = path.read_text(encoding="utf-8", errors="replace").splitlines()[:3]
    if not any("SPDX-License-Identifier: GPL-3.0-or-later" in line for line in first_lines):
        errors.append(f"Missing SPDX-License-Identifier: GPL-3.0-or-later header in {path}")

# 2. Verify src/core/ does not import UI or terminal crates
forbidden_core = re.compile(r"\b(ratatui|crossterm|edtui|arboard|crate::ui)\b")
for path in sorted(pathlib.Path("src/core").rglob("*.rs")):
    text = path.read_text(encoding="utf-8", errors="replace")
    # Check only non-test code above #[cfg(test)]
    prod_part = text.split("#[cfg(test)]", 1)[0]
    for idx, line in enumerate(prod_part.splitlines(), 1):
        stripped = line.lstrip()
        if stripped.startswith("//"):
            continue
        if forbidden_core.search(line):
            errors.append(f"Core/UI boundary violation in {path}:{idx}: {line.strip()}")

# 3. Check diff for newly added .unwrap()/.expect( in non-test code and keybinding parity
diff_cmd = ["git", "diff", "HEAD", "--", "src/"]
diff_out = subprocess.check_output(diff_cmd, text=True, errors="replace")
if not diff_out.strip():
    try:
        subprocess.check_call(["git", "rev-parse", "--verify", base_ref], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        diff_out = subprocess.check_output(["git", "diff", f"{base_ref}...HEAD", "--", "src/"], text=True, errors="replace")
    except subprocess.CalledProcessError:
        diff_out = ""

changed_ui_keys = False
changed_help_or_readme = False
current_file = None
in_test_section = False

for line in diff_out.splitlines():
    if line.startswith("+++ b/"):
        current_file = line[6:]
        in_test_section = False
        if current_file == "src/ui/help.rs":
            changed_help_or_readme = True
        continue
    if line.startswith("+") and not line.startswith("+++"):
        content = line[1:]
        if "#[cfg(test)]" in content:
            in_test_section = True
        if current_file and current_file.startswith("src/ui/") and "KeyCode::" in content:
            changed_ui_keys = True
        if not in_test_section and not content.lstrip().startswith("//"):
            if re.search(r"\.(unwrap|expect)\s*\(", content):
                warnings.append(f"New unwrap/expect in added line ({current_file}): {content.strip()}")

if not changed_help_or_readme:
    all_changed = subprocess.check_output(["git", "diff", "--name-only", "HEAD"], text=True, errors="replace")
    if "README.md" in all_changed or "src/ui/help.rs" in all_changed:
        changed_help_or_readme = True

if changed_ui_keys and not changed_help_or_readme:
    warnings.append("KeyCode:: changed under src/ui/ without updates to src/ui/help.rs or README.md; verify keybinding parity.")

for w in warnings:
    print(f"WARNING: {w}")
for e in errors:
    print(f"ERROR: {e}", file=sys.stderr)

if errors:
    sys.exit(1)
print("Hygiene checks passed.")
PY

echo "ALL VALIDATION CHECKS PASSED."
