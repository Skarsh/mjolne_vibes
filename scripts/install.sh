#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

DEFAULT_MODEL="qwen2.5:3b"
DEFAULT_OLLAMA_BASE_URL="http://localhost:11434"

log() {
  printf '[install] %s\n' "$*"
}

fail() {
  printf '[install] ERROR: %s\n' "$*" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1 || fail "missing required command: ${cmd}"
}

read_env_var() {
  local file="$1"
  local key="$2"

  [[ -f "$file" ]] || return 1

  local line value
  line="$(grep -E "^[[:space:]]*${key}=" "$file" | tail -n 1 || true)"
  [[ -n "$line" ]] || return 1

  value="${line#*=}"
  value="$(printf '%s' "$value" | sed -e 's/[[:space:]]*#.*$//' -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
  value="${value%\"}"
  value="${value#\"}"
  value="${value%\'}"
  value="${value#\'}"

  [[ -n "$value" ]] || return 1
  printf '%s\n' "$value"
}

wait_for_ollama() {
  local base_url="$1"
  local attempts=60
  local i

  for ((i = 1; i <= attempts; i++)); do
    if curl -fsS --max-time 2 "${base_url}/api/tags" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  return 1
}

start_ollama() {
  local compose_cmd=()

  if docker compose version >/dev/null 2>&1; then
    compose_cmd=(docker compose)
  elif command -v docker-compose >/dev/null 2>&1; then
    compose_cmd=(docker-compose)
  fi

  if [[ -f "${REPO_ROOT}/compose.yaml" && ${#compose_cmd[@]} -gt 0 ]]; then
    log "Starting Ollama with ${compose_cmd[*]}..."
    "${compose_cmd[@]}" -f "${REPO_ROOT}/compose.yaml" up -d ollama
    return
  fi

  if docker ps -a --format '{{.Names}}' | grep -Fxq 'ollama'; then
    log "Starting existing Ollama container..."
    docker start ollama >/dev/null
    return
  fi

  log "Starting Ollama container with docker run..."
  docker run -d \
    --name ollama \
    --restart unless-stopped \
    -p 11434:11434 \
    -e OLLAMA_HOST=0.0.0.0:11434 \
    -v ollama-data:/root/.ollama \
    ollama/ollama:latest >/dev/null
}

main() {
  cd "$REPO_ROOT"

  require_cmd cargo
  require_cmd docker
  require_cmd curl

  if ! docker info >/dev/null 2>&1; then
    fail "cannot access Docker daemon. Check Docker is running and your user has socket access."
  fi

  if [[ ! -f .env ]]; then
    log "Creating .env from .env.example..."
    cp .env.example .env
  else
    log ".env already exists; leaving it unchanged."
  fi

  local model ollama_base_url model_provider
  model_provider="$(
    read_env_var .env MODEL_PROVIDER || \
      read_env_var .env.example MODEL_PROVIDER || \
      printf '%s' "ollama"
  )"
  model="$(read_env_var .env MODEL || read_env_var .env.example MODEL || printf '%s' "$DEFAULT_MODEL")"
  ollama_base_url="$(
    read_env_var .env OLLAMA_BASE_URL || \
      read_env_var .env.example OLLAMA_BASE_URL || \
      printf '%s' "$DEFAULT_OLLAMA_BASE_URL"
  )"

  if [[ "${model_provider,,}" != "ollama" ]]; then
    log "MODEL_PROVIDER=${model_provider}; using local Ollama model ${DEFAULT_MODEL} for bootstrap."
    model="$DEFAULT_MODEL"
  fi

  start_ollama

  log "Waiting for Ollama API at ${ollama_base_url}..."
  if ! wait_for_ollama "$ollama_base_url"; then
    fail "Ollama did not become ready at ${ollama_base_url} within timeout."
  fi

  log "Pulling model ${model}..."
  docker exec ollama ollama pull "$model"

  if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    log "Installing repository git hooks..."
    "${SCRIPT_DIR}/install_hooks.sh"
  else
    log "Skipping hook install because this directory is not a git work tree."
  fi

  log "Bootstrap complete."
  printf '\n'
  printf 'Next step:\n'
  printf '  cargo run -- chat "hello"\n'
}

main "$@"
