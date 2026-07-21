#!/usr/bin/env bash
# Fail if user-facing docs or UI still advertise v3 as the canonical workflow format.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PATTERN='v3-only workflow schema|The only accepted workflow format is version `3`|Complete v3 workflow format reference|Import v3 JSON|canonical v3'

MATCHES="$(
  git ls-files -z -- \
    ':!docs/issues/**' \
    ':!docs/tasks/**' \
    ':!scripts/check-canonical-v4-docs.sh' \
    | xargs -0 -r grep -InE "$PATTERN" \
    || true
)"

if [[ -n "$MATCHES" ]]; then
  echo "Found stale v3 canonical-format references (canonical format is v4):" >&2
  echo "$MATCHES" >&2
  exit 1
fi

echo "No stale v3 canonical-format references found."
