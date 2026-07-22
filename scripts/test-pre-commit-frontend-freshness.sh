#!/usr/bin/env bash
# Exercise the frontend freshness hook against staged and unstaged UI changes.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ ! -d "$ROOT/node_modules" ]]; then
  echo "Frontend dependencies are missing. Run 'npm install' before this test." >&2
  exit 1
fi

TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/silverbond-hook-test.XXXXXX")"
cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

create_fixture() {
  local fixture="$1"

  mkdir -p "$fixture"
  git -C "$ROOT" archive --format=tar HEAD | tar -xf - -C "$fixture"
  install -m 755 "$ROOT/.githooks/pre-commit" "$fixture/.githooks/pre-commit"
  ln -s "$ROOT/node_modules" "$fixture/node_modules"

  git -C "$fixture" init -q
  git -C "$fixture" config user.email "hook-test@silverbond.invalid"
  git -C "$fixture" config user.name "SilverBond Hook Test"
  git -C "$fixture" add .
  git -C "$fixture" commit -qm "fixture baseline"
}

append_staged_change() {
  local fixture="$1"

  printf '\n/* staged hook regression */\n:root { --hook-staged: #123456; }\n' \
    >> "$fixture/ui/src/styles.css"
}

append_unstaged_change() {
  local fixture="$1"

  printf '\n/* unstaged hook regression */\n:root { --hook-unstaged: #abcdef; }\n' \
    >> "$fixture/ui/src/styles.css"
}

run_build() {
  local fixture="$1"

  (cd "$fixture" && npm run build >/dev/null)
}

assert_public_unchanged() {
  local before="$1"
  local fixture="$2"

  diff -qr "$before" "$fixture/public" >/dev/null \
    || fail "the hook modified the working-tree public/ directory"
}

PASS_FIXTURE="$TEST_ROOT/pass"
create_fixture "$PASS_FIXTURE"
append_staged_change "$PASS_FIXTURE"
run_build "$PASS_FIXTURE"
git -C "$PASS_FIXTURE" add ui/src/styles.css public/
append_unstaged_change "$PASS_FIXTURE"
cp -R "$PASS_FIXTURE/public" "$TEST_ROOT/pass-public-before"

if ! (cd "$PASS_FIXTURE" && .githooks/pre-commit) >"$TEST_ROOT/pass-hook.log" 2>&1; then
  sed -n '1,120p' "$TEST_ROOT/pass-hook.log" >&2
  fail "a bundle built from the staged UI tree should pass despite unstaged UI changes"
fi
assert_public_unchanged "$TEST_ROOT/pass-public-before" "$PASS_FIXTURE"

FAIL_FIXTURE="$TEST_ROOT/fail"
create_fixture "$FAIL_FIXTURE"
append_staged_change "$FAIL_FIXTURE"
git -C "$FAIL_FIXTURE" add ui/src/styles.css
append_unstaged_change "$FAIL_FIXTURE"
run_build "$FAIL_FIXTURE"
git -C "$FAIL_FIXTURE" add public/
cp -R "$FAIL_FIXTURE/public" "$TEST_ROOT/fail-public-before"

if (cd "$FAIL_FIXTURE" && .githooks/pre-commit) >"$TEST_ROOT/fail-hook.log" 2>&1; then
  fail "a staged bundle built from unstaged UI sources should be rejected"
fi
assert_public_unchanged "$TEST_ROOT/fail-public-before" "$FAIL_FIXTURE"

echo "Frontend freshness hook regressions passed."
