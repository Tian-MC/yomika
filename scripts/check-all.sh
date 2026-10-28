#!/usr/bin/env bash
# Full local validation suite (mirrors .github/workflows/ci.yml).
#
# Usage: scripts/check-all.sh [--fast] [--keep-going]
#   --fast        skip the release `tauri build --no-bundle` smoke build
#   --keep-going  run every step even if one fails, then report all failures
#
# Optional tools are used when present: cargo-deny, shellcheck, actionlint,
# appstreamcli, desktop-file-validate, makepkg.

set -uo pipefail

FAST=0
KEEP_GOING=0
for arg in "$@"; do
  case "$arg" in
    --fast) FAST=1 ;;
    --keep-going | -k) KEEP_GOING=1 ;;
    -h | --help)
      sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
[[ -d $CARGO_BIN && :$PATH: != *":$CARGO_BIN:"* ]] && PATH="$CARGO_BIN:$PATH"
export PATH

if [[ -t 1 ]]; then B=$'\e[1m' G=$'\e[32m' R=$'\e[31m' Y=$'\e[33m' N=$'\e[0m'; else B='' G='' R='' Y='' N=''; fi

FAILED=()
SKIPPED=()
step() { # step "name" cmd args...
  local name="$1"
  shift
  printf '\n%s==> %s%s\n' "$B" "$name" "$N"
  local start=$SECONDS
  if "$@"; then
    printf '%s[pass]%s %s (%ss)\n' "$G" "$N" "$name" "$((SECONDS - start))"
  else
    printf '%s[FAIL]%s %s\n' "$R" "$N" "$name"
    FAILED+=("$name")
    if [[ $KEEP_GOING == 0 ]]; then summary; fi
  fi
}
skip() {
  printf '%s[skip]%s %s\n' "$Y" "$N" "$*"
  SKIPPED+=("$*")
}
summary() {
  echo
  ((${#SKIPPED[@]})) && printf '%sSkipped:%s\n%s\n' "$Y" "$N" "$(printf '  - %s\n' "${SKIPPED[@]}")"
  if ((${#FAILED[@]})); then
    printf '%sFailed:%s\n%s\n' "$R" "$N" "$(printf '  - %s\n' "${FAILED[@]}")"
    exit 1
  fi
  printf '%sAll checks passed.%s\n' "$G" "$N"
  exit 0
}
have() { command -v "$1" >/dev/null 2>&1; }

have cargo || {
  echo "cargo not found (install rustup; see docs/DEVELOPMENT.md)" >&2
  exit 1
}
[[ -d node_modules ]] || step "npm ci" npm ci

# --- Rust
step "cargo fmt --check" cargo fmt --all -- --check
step "cargo clippy -D warnings" cargo clippy --workspace --all-targets --locked -- -D warnings
step "cargo test" cargo test --workspace --locked

# --- Frontend
step "tsc (typecheck)" npm run typecheck
step "eslint" npm run lint
step "vitest" npm test

# --- Supply chain
if cargo deny --version >/dev/null 2>&1; then
  step "cargo deny" cargo deny --log-level error check
else
  skip "cargo deny (cargo install --locked cargo-deny)"
fi

# --- Repo hygiene: scripts, workflows, packaging metadata
shopt -s nullglob
sh_files=(scripts/*.sh)
if have shellcheck; then step "shellcheck" shellcheck "${sh_files[@]}"; else
  # shellcheck disable=SC2016 # expanded by the inner bash
  step "bash -n scripts" bash -c 'for f in "$@"; do bash -n "$f" || exit 1; done' _ "${sh_files[@]}"
  skip "shellcheck (not installed)"
fi
if have actionlint; then step "actionlint" actionlint; else skip "actionlint (not installed)"; fi
DESKTOP=packaging/linux/io.github.vnlauncher.VNLauncher.desktop
METAINFO=packaging/linux/io.github.vnlauncher.VNLauncher.metainfo.xml
if have desktop-file-validate; then step "desktop-file-validate" desktop-file-validate "$DESKTOP"; else skip "desktop-file-validate"; fi
if have appstreamcli; then
  step "appstreamcli validate" appstreamcli validate --no-net --explain "$METAINFO"
else skip "appstreamcli"; fi
if have makepkg; then
  step "PKGBUILD/.SRCINFO in sync" bash -c 'cd packaging/arch && diff -u .SRCINFO <(makepkg --printsrcinfo)'
fi

# --- Release smoke build (slow)
if [[ $FAST == 0 ]]; then
  step "tauri build --no-bundle" npm run tauri -- build --no-bundle
else
  skip "tauri build (--fast)"
fi

summary
