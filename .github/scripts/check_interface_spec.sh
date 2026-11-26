#!/usr/bin/env bash
# Keeps docs/0002-interface-spec.md in step with the contract's real interface.
#
# Fails if:
#   - a `pub fn` in the `#[contractimpl]` block of contracts/escrow/src/lib.rs
#     has no `### N. \`name\`` section in the spec (or the spec documents a
#     method that no longer exists);
#   - a section's JSON block has a different "name" than its heading, or the
#     section numbers aren't 1..N in order;
#   - a JSON block lists an `Error` variant that isn't in `pub enum Error`;
#   - the Summary table's counts (total, auth-gated, permissionless, state
#     transitions, error variants) don't match what the JSON blocks say.
#
# Sibling of check_auth_docs.sh; same "run from repo root" convention.
set -euo pipefail

LIB="contracts/escrow/src/lib.rs"
SPEC="docs/0002-interface-spec.md"

for f in "$LIB" "$SPEC"; do
  if [[ ! -f "$f" ]]; then
    echo "error: $f not found (run from repo root)" >&2
    exit 1
  fi
done

python3 - "$LIB" "$SPEC" <<'PY'
import json
import re
import sys

lib_path, spec_path = sys.argv[1], sys.argv[2]
lib_lines = open(lib_path).read().splitlines()
lib = "\n".join(lib_lines)
spec = open(spec_path).read()
failures = []

# --- pub fns inside the #[contractimpl] block --------------------------------
try:
    start = next(i for i, l in enumerate(lib_lines) if l.strip() == "#[contractimpl]")
except StopIteration:
    print(f"error: no #[contractimpl] block in {lib_path}")
    sys.exit(1)
# The impl block closes at the first column-0 `}` after the attribute.
end = next(i for i in range(start + 1, len(lib_lines)) if lib_lines[i] == "}")
code_fns = []
for line in lib_lines[start:end]:
    m = re.match(r"^    pub fn ([A-Za-z0-9_]+)\s*\(", line)
    if m:
        code_fns.append(m.group(1))

# --- method sections in the spec ---------------------------------------------
sections = list(re.finditer(r"^### (\d+)\. `([A-Za-z0-9_]+)`\s*$", spec, re.M))
spec_fns = [m.group(2) for m in sections]

for name in sorted(set(code_fns) - set(spec_fns)):
    failures.append(f"`{name}` is a pub fn in #[contractimpl] but has no section in {spec_path}")
for name in sorted(set(spec_fns) - set(code_fns)):
    failures.append(f"`{name}` has a section in {spec_path} but is not a pub fn in #[contractimpl]")
for name in sorted({n for n in spec_fns if spec_fns.count(n) > 1}):
    failures.append(f"`{name}` has more than one section in {spec_path}")

for expected, m in enumerate(sections, start=1):
    if int(m.group(1)) != expected:
        failures.append(f"section `{m.group(2)}` is numbered {m.group(1)}, expected {expected}")

# --- JSON blocks --------------------------------------------------------------
enum = re.search(r"pub enum Error \{(.*?)\n\}", lib, re.S)
error_variants = set(re.findall(r"^\s*([A-Z]\w*)\s*=", enum.group(1), re.M)) if enum else set()

blocks = []
for idx, m in enumerate(sections):
    body_end = sections[idx + 1].start() if idx + 1 < len(sections) else len(spec)
    body = spec[m.end():body_end]
    jm = re.search(r"```json\n(.*?)\n```", body, re.S)
    if not jm:
        failures.append(f"section `{m.group(2)}` has no ```json block")
        continue
    try:
        block = json.loads(jm.group(1))
    except json.JSONDecodeError as e:
        failures.append(f"section `{m.group(2)}`: JSON does not parse ({e})")
        continue
    if block.get("name") != m.group(2):
        failures.append(f"section `{m.group(2)}`: JSON \"name\" is {block.get('name')!r}")
    for err in block.get("errors", []):
        if err not in error_variants:
            failures.append(f"section `{m.group(2)}`: error `{err}` is not a variant of `Error`")
    blocks.append(block)

# --- Summary table ------------------------------------------------------------
def summary_number(label):
    m = re.search(r"^\| " + re.escape(label) + r" \| (\d+)", spec, re.M)
    if not m:
        failures.append(f"Summary row `{label}` missing or has no leading number")
        return None
    return int(m.group(1))

permissionless = sum(1 for b in blocks if b.get("auth_requirement") == "permissionless")
expected_counts = {
    "Total Methods": len(code_fns),
    "Auth-gated": len(blocks) - permissionless,
    "Permissionless": permissionless,
    "State transitions": sum(1 for b in blocks if b.get("state_transition")),
    "Error variants": len({e for b in blocks for e in b.get("errors", [])}),
}
for label, want in expected_counts.items():
    got = summary_number(label)
    if got is not None and got != want:
        failures.append(f"Summary `{label}` says {got}, but the spec/contract give {want}")

if failures:
    print(f"error: {spec_path} is out of sync with {lib_path}:")
    for f in failures:
        print(f"  - {f}")
    print("hint: add a `### N. `name`` section with a ```json block in the existing")
    print("      format for every new pub fn, renumber, and update the Summary table.")
    sys.exit(1)

print(f"ok: {spec_path} documents all {len(code_fns)} public methods; summary counts match")
PY
