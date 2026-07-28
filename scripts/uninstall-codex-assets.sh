#!/usr/bin/env bash
set -euo pipefail

REPO_PATH_DEFAULT="${HOME}/src/plugins"
CODEX_HOME_DEFAULT="${HOME}/.codex"
AGENTS_HOME_DEFAULT="${HOME}/.agents"
MARKETPLACE_NAME_DEFAULT="plugins"

REPO_PATH="$REPO_PATH_DEFAULT"
CODEX_HOME="$CODEX_HOME_DEFAULT"
AGENTS_HOME="$AGENTS_HOME_DEFAULT"
MARKETPLACE_NAME="$MARKETPLACE_NAME_DEFAULT"
FORCE=0

REPO_PATH_SET=0
CODEX_HOME_SET=0
AGENTS_HOME_SET=0
MARKETPLACE_NAME_SET=0

usage() {
  cat <<'EOF'
Usage: scripts/uninstall-codex-assets.sh [options]

Remove Codex skill links/copies and marketplace entries created by install-codex-assets.sh.

Options:
  --repo-path PATH        Location of the plugins checkout (default: ~/src/plugins)
  --codex-home PATH       Codex user directory (default: ~/.codex)
  --agents-home PATH      Agents user directory for legacy marketplace metadata (default: ~/.agents)
  --marketplace-name NAME Marketplace name to remove (default: plugins)
  --force                 Remove matching copy-mode skill directories
  -h, --help              Show this help message
EOF
}

log() {
  printf '[codex-bootstrap] %s\n' "$*"
}

warn() {
  printf '[codex-bootstrap] warning: %s\n' "$*" >&2
}

normalize_path() {
  python3 - "$1" <<'PY'
import sys
from pathlib import Path

print(Path(sys.argv[1]).expanduser().resolve())
PY
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-path)
      REPO_PATH="$2"
      REPO_PATH_SET=1
      shift 2
      ;;
    --codex-home)
      CODEX_HOME="$2"
      CODEX_HOME_SET=1
      shift 2
      ;;
    --agents-home)
      AGENTS_HOME="$2"
      AGENTS_HOME_SET=1
      shift 2
      ;;
    --marketplace-name)
      MARKETPLACE_NAME="$2"
      MARKETPLACE_NAME_SET=1
      shift 2
      ;;
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

CODEX_HOME="$(normalize_path "$CODEX_HOME")"
STATE_PATH="${CODEX_HOME}/state/plugins.json"

load_state_defaults() {
  [[ -f "$STATE_PATH" ]] || return 0

  while IFS='=' read -r key value; do
    case "$key" in
      repo_path)
        if [[ "$REPO_PATH_SET" -eq 0 ]]; then
          REPO_PATH="$value"
        fi
        ;;
      codex_home)
        if [[ "$CODEX_HOME_SET" -eq 0 ]]; then
          CODEX_HOME="$value"
        fi
        ;;
      marketplace_name)
        if [[ "$MARKETPLACE_NAME_SET" -eq 0 ]]; then
          MARKETPLACE_NAME="$value"
        fi
        ;;
    esac
  done < <(python3 - "$STATE_PATH" <<'PY'
import json
import sys
from pathlib import Path

data = json.loads(Path(sys.argv[1]).read_text())
for key in ("repo_path", "codex_home", "marketplace_name"):
    value = data.get(key)
    if value:
        print(f"{key}={value}")
PY
)
}

load_state_defaults

REPO_PATH="$(normalize_path "$REPO_PATH")"
CODEX_HOME="$(normalize_path "$CODEX_HOME")"
AGENTS_HOME="$(normalize_path "$AGENTS_HOME")"
MARKETPLACE_NAME="${MARKETPLACE_NAME:-$MARKETPLACE_NAME_DEFAULT}"

SKILLS_ROOT="${CODEX_HOME}/skills"
STATE_PATH="${CODEX_HOME}/state/plugins.json"

resolve_link_target() {
  python3 - "$1" <<'PY'
import os
import sys

target = os.path.abspath(sys.argv[1])
link = os.readlink(target)
if not os.path.isabs(link):
    link = os.path.join(os.path.dirname(target), link)
print(os.path.realpath(link))
PY
}

load_managed_skills() {
  if [[ -f "$STATE_PATH" ]]; then
    python3 - "$STATE_PATH" <<'PY'
import json
import sys
from pathlib import Path

data = json.loads(Path(sys.argv[1]).read_text())
for item in data.get("managed_skills", []):
    print(item)
PY
  fi
}

remove_marketplace() {
  local removed_cli=0
  if command -v codex >/dev/null 2>&1; then
    if CODEX_HOME="${CODEX_HOME}" codex marketplace remove "${MARKETPLACE_NAME}" >/dev/null 2>&1; then
      log "Removed marketplace \"${MARKETPLACE_NAME}\" via codex marketplace remove"
      removed_cli=1
    elif CODEX_HOME="${CODEX_HOME}" codex marketplace remove "${REPO_PATH}" >/dev/null 2>&1; then
      log "Removed marketplace registered at ${REPO_PATH} via codex marketplace remove"
      removed_cli=1
    else
      warn "codex marketplace remove failed; remove manually: CODEX_HOME=\"${CODEX_HOME}\" codex marketplace remove \"${MARKETPLACE_NAME}\""
    fi
  else
    warn "codex CLI not found; skipping Codex marketplace removal. Remove manually: CODEX_HOME=\"${CODEX_HOME}\" codex marketplace remove \"${MARKETPLACE_NAME}\""
  fi

  remove_legacy_marketplace

  return $removed_cli
}

remove_legacy_marketplace() {
  local marketplace_file="${AGENTS_HOME}/plugins/marketplace.json"
  [[ -f "$marketplace_file" ]] || { log "Legacy marketplace file not found, skipping removal."; return; }

  python3 - "$marketplace_file" "$MARKETPLACE_NAME" <<'PY'
import json
import sys
from pathlib import Path

marketplace_file = Path(sys.argv[1])
marketplace_name = sys.argv[2]

data = json.loads(marketplace_file.read_text())
if isinstance(data, dict) and "marketplaces" in data and isinstance(data["marketplaces"], list):
    entries = data["marketplaces"]
    style = "list"
elif isinstance(data, dict) and "plugins" in data:
    entries = [data]
    style = "single"
else:
    raise SystemExit(f"Unexpected marketplace format in {marketplace_file}")

filtered = [m for m in entries if m.get("name") != marketplace_name]

if not filtered:
    marketplace_file.unlink()
    print(f"removed marketplace file {marketplace_file}")
    sys.exit(0)

if style == "list" or len(filtered) > 1:
    output = {"marketplaces": filtered}
else:
    output = filtered[0]

marketplace_file.write_text(json.dumps(output, indent=2) + "\n")
print(f"removed marketplace entry \"{marketplace_name}\" from {marketplace_file}")
PY
}

remove_skill_target() {
  local target="$1"

  if [[ -L "$target" ]]; then
    local current
    current="$(resolve_link_target "$target")"
    case "$current" in
      "${REPO_PATH}"/plugins/*/skills/*)
        rm -f "$target"
        return 0
        ;;
    esac
    warn "skipping ${target}; symlink points to ${current}"
    return 1
  fi

  if [[ -d "$target" && "$FORCE" -eq 1 ]]; then
    local name source
    name="$(basename "$target")"
    for source in "${REPO_PATH}"/plugins/*/skills/"${name}"; do
      [[ -d "$source" ]] || continue
      if diff -qr "$source" "$target" >/dev/null 2>&1; then
        rm -rf "$target"
        return 0
      fi
    done
  fi

  return 1
}

remove_skills() {
  [[ -d "$SKILLS_ROOT" ]] || { log "Skills directory not found, skipping skill removal."; return; }

  local removed=0
  local skipped=0
  local skill_name target
  local managed_any=0

  while IFS= read -r skill_name; do
    [[ -n "$skill_name" ]] || continue
    managed_any=1
    target="${SKILLS_ROOT}/${skill_name}"
    if [[ ! -e "$target" && ! -L "$target" ]]; then
      continue
    fi

    if remove_skill_target "$target"; then
      ((++removed))
    else
      ((++skipped))
    fi
  done < <(load_managed_skills)

  if [[ "$managed_any" -eq 0 ]]; then
    for target in "${SKILLS_ROOT}"/*; do
      [[ -e "$target" || -L "$target" ]] || continue

      if remove_skill_target "$target"; then
        ((++removed))
      else
        ((++skipped))
      fi
    done
  fi

  log "Skills removed: ${removed}; skipped: ${skipped}"
}

remove_state() {
  if [[ -f "$STATE_PATH" ]]; then
    rm -f "$STATE_PATH"
    log "Removed install state ${STATE_PATH}"
  fi
}

main() {
  remove_marketplace
  remove_skills
  remove_state

  cat <<EOF

Codex assets removed where possible.
- Repository path checked: ${REPO_PATH}
- Marketplace removed (if present): ${MARKETPLACE_NAME}
- Skills directory scanned: ${SKILLS_ROOT}
EOF
}

main "$@"
