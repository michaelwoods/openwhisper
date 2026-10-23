#!/usr/bin/env bash
# harness-qa.sh — deterministic QA contract for the autonomous coding harness.
#
# This is the canonical gate the Forgejo/worktree harness runs on a candidate tree
# after the final writer turn. Every tier below comes from AGENTS.md §4; nothing here
# is interactive, and nothing here touches a live desktop session.
#
# Usage: ./scripts/harness-qa.sh [tier ...]
#   Tiers: fmt clippy doc test   (default: all four, in order)
#   HARNESS_QA_SKIP=clippy,test ./scripts/harness-qa.sh   # skip specific tiers
#
# Exit status: 0 only when every requested tier passed. A skipped tier is reported
# as SKIP and is never counted as a pass (see the summary table).
set -Eeuo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Toolchain environment (AGENTS.md §1 / Makefile)
export CARGO_HOME="${CARGO_HOME:-$HOME/.cache/puccinialin/cargo}"
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"
export PKG_CONFIG_PATH="$HOME/.local/lib/pkgconfig:$HOME/.local/share/pkgconfig:${PKG_CONFIG_PATH:-}:/usr/lib64/pkgconfig:/usr/share/pkgconfig"
export CARGO_TERM_COLOR=never

# Never let a hermetic gate inherit a live desktop session.
unset WAYLAND_DISPLAY DISPLAY DBUS_SESSION_BUS_ADDRESS || true

SKIP_LIST="${HARNESS_QA_SKIP:-}"
ALL_TIERS=(fmt clippy doc test)
REQUESTED=("$@")
if [ "${#REQUESTED[@]}" -eq 0 ]; then
  REQUESTED=("${ALL_TIERS[@]}")
fi

skipped() {
  local tier="$1"
  case ",${SKIP_LIST}," in
    *",${tier},"*) return 0 ;;
  esac
  return 1
}

declare -a RESULTS=()
FAILED=0

run_tier() {
  local name="$1"
  shift
  if skipped "$name"; then
    echo "=== [${name}] SKIP (requested via HARNESS_QA_SKIP) ==="
    RESULTS+=("${name}: SKIP")
    return 0
  fi
  echo "=== [${name}] \$ $* ==="
  local rc=0
  "$@" || rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "=== [${name}] FAIL (exit ${rc}) ==="
    RESULTS+=("${name}: FAIL (exit ${rc})")
    FAILED=1
    return "$rc"
  fi
  echo "=== [${name}] PASS ==="
  RESULTS+=("${name}: PASS")
  return 0
}

# Continue through later tiers only while nothing has failed: a broken tree makes
# doc/test output noise, and the first failure is the actionable evidence.
for tier in "${REQUESTED[@]}"; do
  case "$tier" in
    fmt) run_tier fmt cargo fmt --check || break ;;
    clippy) run_tier clippy cargo clippy --all-targets -- -D warnings || break ;;
    doc) run_tier doc cargo doc --no-deps || break ;;
    test) run_tier test cargo test --all-targets || break ;;
    *) echo "Unknown tier: $tier (expected: fmt clippy doc test)" >&2; exit 2 ;;
  esac
done

echo
echo "=== QA summary ==="
for line in "${RESULTS[@]}"; do
  echo "  ${line}"
done

if [ "$FAILED" -ne 0 ]; then
  echo "=== QA RESULT: FAIL ==="
  exit 1
fi
echo "=== QA RESULT: PASS ==="
