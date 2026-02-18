#!/usr/bin/env bash
set -euo pipefail

CONTAINER_NAME="ollama"
OLLAMA_MOUNT_DEST="/root/.ollama"
DEFAULT_VOLUME_NAME="ollama-data"

ASSUME_YES=0
DRY_RUN=0

log() {
  printf '[cleanup] %s\n' "$*"
}

fail() {
  printf '[cleanup] ERROR: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'USAGE'
Usage: ./scripts/cleanup_ollama_data.sh [--yes] [--dry-run]

Delete local Ollama Docker model data volume after discovering it.

Options:
  --yes       Execute deletion without prompt.
  --dry-run   Print actions without making changes.
  -h, --help  Show this help.
USAGE
}

run_cmd() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '[cleanup] DRY RUN:'
    printf ' %q' "$@"
    printf '\n'
    return 0
  fi

  "$@"
}

container_exists() {
  docker ps -a --format '{{.Names}}' | grep -Fxq "$CONTAINER_NAME"
}

container_running() {
  docker inspect -f '{{.State.Running}}' "$CONTAINER_NAME" 2>/dev/null | grep -Fxq 'true'
}

find_target_volume() {
  local volume_name

  if container_exists; then
    volume_name="$(
      docker inspect \
        --format '{{range .Mounts}}{{if and (eq .Type "volume") (eq .Destination "'"$OLLAMA_MOUNT_DEST"'")}}{{.Name}}{{end}}{{end}}' \
        "$CONTAINER_NAME"
    )"
    volume_name="$(printf '%s' "$volume_name" | tr -d '[:space:]')"
    if [[ -n "$volume_name" ]]; then
      printf '%s\n' "$volume_name"
      return 0
    fi
  fi

  if docker volume ls --format '{{.Name}}' | grep -Fxq "$DEFAULT_VOLUME_NAME"; then
    printf '%s\n' "$DEFAULT_VOLUME_NAME"
    return 0
  fi

  return 1
}

main() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --yes)
        ASSUME_YES=1
        shift
        ;;
      --dry-run)
        DRY_RUN=1
        shift
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        fail "unknown argument: $1"
        ;;
    esac
  done

  command -v docker >/dev/null 2>&1 || fail "docker command not found"
  docker info >/dev/null 2>&1 || fail "cannot access Docker daemon"

  local volume_name mountpoint
  volume_name="$(find_target_volume)" || fail "could not find Ollama volume to delete"
  mountpoint="$(docker volume inspect --format '{{.Mountpoint}}' "$volume_name")"

  log "Discovered Ollama data volume:"
  log "  volume: ${volume_name}"
  log "  host path: ${mountpoint}"
  log "  expected model data path: ${mountpoint}/models"

  if [[ "$ASSUME_YES" -ne 1 && "$DRY_RUN" -ne 1 ]]; then
    printf '[cleanup] This will delete all local Ollama models in the volume above.\n'
    printf '[cleanup] Re-run with --yes to continue.\n'
    exit 0
  fi

  if container_exists; then
    if container_running; then
      log "Stopping container ${CONTAINER_NAME}..."
      run_cmd docker stop "$CONTAINER_NAME"
    fi

    log "Removing container ${CONTAINER_NAME}..."
    run_cmd docker rm "$CONTAINER_NAME"
  fi

  log "Removing volume ${volume_name}..."
  run_cmd docker volume rm "$volume_name"

  log "Done. Ollama model data has been deleted."
  log "Recreate with: docker compose up -d ollama && docker exec ollama ollama pull qwen2.5:3b"
}

main "$@"
