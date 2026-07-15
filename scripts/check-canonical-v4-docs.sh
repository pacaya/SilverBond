#!/usr/bin/env bash
# Fail if user-facing docs or UI still advertise v3 as the canonical workflow format.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PATTERN='v3-only workflow schema|The only accepted workflow format is version `3`|Complete v3 workflow format reference|Import v3 JSON|canonical v3'

if command -v rg >/dev/null 2>&1; then
  MATCHES="$(rg -n --pcre2 "$PATTERN" \
    --glob '!docs/issues/**' \
    --glob '!docs/tasks/**' \
    --glob '!public/**' \
    --glob '!scripts/check-canonical-v4-docs.sh' \
    . || true)"
else
  MATCHES="$(grep -RInE "$PATTERN" \
    --exclude-dir=docs/issues \
    --exclude-dir=docs/tasks \
    --exclude-dir=public \
    --exclude=check-canonical-v4-docs.sh \
    . || true)"
fi

if [[ -n "$MATCHES" ]]; then
  echo "Found stale v3 canonical-format references (canonical format is v4):" >&2
  echo "$MATCHES" >&2
  exit 1
fi

echo "No stale v3 canonical-format references found."
