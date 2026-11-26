#!/usr/bin/env bash
# Claude Code Stop hook: before Claude finishes, run `make lint` (rustfmt +
# clippy -D warnings). On failure, hand the errors back so Claude fixes them.
set -uo pipefail

input="$(cat)"
# Claude is already continuing because of this hook: let it stop rather
# than loop on something it can't fix.
if [[ "$(jq -r '.stop_hook_active // false' <<<"$input")" == "true" ]]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}" || exit 0
if output="$(make lint 2>&1)"; then
  exit 0
fi

jq -n --arg reason "make lint failed. Fix these before finishing:
$(grep -E -A12 '^(error|warning)|^Diff in' <<<"$output" | head -n 80)" \
  '{decision: "block", reason: $reason}'
