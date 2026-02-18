#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

log() {
  printf '[hooks] %s\n' "$*"
}

fail() {
  printf '[hooks] ERROR: %s\n' "$*" >&2
  exit 1
}

main() {
  cd "${REPO_ROOT}"

  git rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail "not inside a git repository"
  [[ -d .githooks ]] || fail "missing .githooks directory"

  chmod +x .githooks/pre-commit .githooks/pre-push
  git config core.hooksPath .githooks

  log "Installed repository hooks via core.hooksPath=.githooks"
  log "pre-commit and pre-push checks are now enforced locally."
}

main "$@"
