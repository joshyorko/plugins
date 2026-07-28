#!/usr/bin/env bash
set -euo pipefail

REPO_URL_DEFAULT="https://github.com/joshyorko/plugins.git"
REPO_PATH_DEFAULT="${HOME}/src/plugins"
CODEX_HOME_DEFAULT="${HOME}/.codex"
MARKETPLACE_NAME_DEFAULT="plugins"
SKILL_MODE_DEFAULT="auto"
INSTALL_METHOD_DEFAULT="git"
LEGACY_AGENTS_HOME="${HOME}/.agents"

REPO_URL="${REPO_URL:-$REPO_URL_DEFAULT}"
REPO_PATH="$REPO_PATH_DEFAULT"
CODEX_HOME="$CODEX_HOME_DEFAULT"
MARKETPLACE_NAME="$MARKETPLACE_NAME_DEFAULT"
SKILL_MODE="${SKILL_MODE:-$SKILL_MODE_DEFAULT}"
INSTALL_METHOD="${INSTALL_METHOD:-$INSTALL_METHOD_DEFAULT}"
REF_SPEC="${REF_SPEC:-}"
RESOLVED_REF_OVERRIDE="${RESOLVED_REF:-}"
FORCE=0
SKIP_REPO_SYNC=0
MARKETPLACE_STATUS="not attempted"

MANAGED_SKILLS=()
LINKED_COUNT=0
COPIED_COUNT=0
SKIPPED_COUNT=0
ACTUAL_SKILL_MODE=""
MARKETPLACE_ADD_MODE=""

usage() {
  cat <<'EOF'
Usage: scripts/install-codex-assets.sh [options]

Bootstrap Codex plugins and skills from this repository into a user-level installation.

Options:
  --repo-path PATH          Destination for the plugins checkout (default: ~/src/plugins)
  --repo-url URL           Git clone URL to use when syncing the repo (default: https://github.com/joshyorko/plugins.git)
  --codex-home PATH        Codex user directory (default: ~/.codex)
  --marketplace-name NAME  Marketplace name to register (default: plugins)
  --skill-mode MODE        auto (default), link, or copy
  --link                   Symlink skills into Codex
  --copy                   Copy skills into Codex
  --install-method MODE    git or archive metadata for the managed install (default: git)
  --ref REF                Ref to sync when managing the repo checkout directly
  --resolved-ref REF       Resolved ref metadata to record in install state
  --skip-repo-sync         Skip clone/update logic and install from an existing checkout
  --force                  Replace conflicting skill entries
  -h, --help               Show this help message

Environment overrides:
  REPO_URL      Default clone URL
  REF_SPEC      Default ref
  RESOLVED_REF  Resolved ref metadata

Examples:
  bash ~/src/plugins/scripts/install-codex-assets.sh --repo-path ~/src/plugins
  bash ~/src/plugins/scripts/install-codex-assets.sh --repo-path ~/src/plugins --skill-mode copy --force
  bash ~/src/plugins/scripts/install-codex-assets.sh --repo-path ~/src/plugins --skip-repo-sync --install-method archive --resolved-ref v1.2.3
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

current_ref() {
  local repo="$1"

  if [[ -n "$RESOLVED_REF_OVERRIDE" ]]; then
    printf '%s\n' "$RESOLVED_REF_OVERRIDE"
    return 0
  fi

  if [[ ! -d "${repo}/.git" ]]; then
    if [[ -n "$REF_SPEC" ]]; then
      printf '%s\n' "$REF_SPEC"
    else
      printf 'unknown\n'
    fi
    return 0
  fi

  local tag=""
  if tag="$(git -C "$repo" describe --tags --exact-match 2>/dev/null)"; then
    printf '%s\n' "$tag"
    return 0
  fi

  local branch=""
  if branch="$(git -C "$repo" symbolic-ref --short -q HEAD 2>/dev/null)"; then
    printf '%s\n' "$branch"
    return 0
  fi

  git -C "$repo" rev-parse --short HEAD
}

sync_repo_ref() {
  [[ -n "$REF_SPEC" ]] || return 0

  log "Syncing repository to ref ${REF_SPEC}"
  git -C "${REPO_PATH}" fetch --tags --prune origin

  if git -C "${REPO_PATH}" rev-parse --verify --quiet "${REF_SPEC}^{commit}" >/dev/null; then
    git -C "${REPO_PATH}" checkout --force "${REF_SPEC}"
    return 0
  fi

  if git -C "${REPO_PATH}" fetch origin "${REF_SPEC}" --depth=1 >/dev/null 2>&1; then
    git -C "${REPO_PATH}" checkout --force FETCH_HEAD
    return 0
  fi

  echo "Unable to resolve ref ${REF_SPEC} in ${REPO_PATH}" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-path)
      REPO_PATH="$2"
      shift 2
      ;;
    --repo-url)
      REPO_URL="$2"
      shift 2
      ;;
    --codex-home)
      CODEX_HOME="$2"
      shift 2
      ;;
    --marketplace-name)
      MARKETPLACE_NAME="$2"
      shift 2
      ;;
    --skill-mode)
      SKILL_MODE="$2"
      shift 2
      ;;
    --install-method)
      INSTALL_METHOD="$2"
      shift 2
      ;;
    --ref)
      REF_SPEC="$2"
      shift 2
      ;;
    --resolved-ref)
      RESOLVED_REF_OVERRIDE="$2"
      shift 2
      ;;
    --skip-repo-sync)
      SKIP_REPO_SYNC=1
      shift
      ;;
    --link)
      SKILL_MODE="link"
      shift
      ;;
    --copy)
      SKILL_MODE="copy"
      shift
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

case "$SKILL_MODE" in
  auto|link|copy) ;;
  *)
    echo "Invalid skill mode: ${SKILL_MODE}" >&2
    exit 1
    ;;
esac

case "$INSTALL_METHOD" in
  git|archive)
    ;;
  *)
    echo "Invalid install method: ${INSTALL_METHOD}" >&2
    exit 1
    ;;
esac

REPO_PATH="$(normalize_path "$REPO_PATH")"
CODEX_HOME="$(normalize_path "$CODEX_HOME")"
MARKETPLACE_NAME="${MARKETPLACE_NAME:-$MARKETPLACE_NAME_DEFAULT}"

SKILLS_ROOT="${CODEX_HOME}/skills"
STATE_ROOT="${CODEX_HOME}/state"
STATE_PATH="${STATE_ROOT}/plugins.json"
CATALOG_PATH="${REPO_PATH}/marketplaces/catalog.json"

clone_or_update_repo() {
  if [[ -d "${REPO_PATH}/.git" ]]; then
    log "Updating existing repository at ${REPO_PATH}"
    git -C "${REPO_PATH}" fetch --tags --prune
    if [[ -z "$REF_SPEC" ]]; then
      git -C "${REPO_PATH}" pull --ff-only
    fi
  elif [[ -e "${REPO_PATH}" ]]; then
    echo "Target path ${REPO_PATH} exists but is not a git repository." >&2
    exit 1
  else
    mkdir -p "$(dirname "${REPO_PATH}")"
    log "Cloning ${REPO_URL} into ${REPO_PATH}"
    git clone "${REPO_URL}" "${REPO_PATH}"
  fi

  sync_repo_ref
}

cleanup_legacy_marketplace() {
  local marketplace_file="${LEGACY_AGENTS_HOME}/plugins/marketplace.json"
  [[ -f "$marketplace_file" ]] || return 0

  python3 - "$marketplace_file" "$MARKETPLACE_NAME" <<'PY'
import json
import sys
from pathlib import Path

marketplace_file = Path(sys.argv[1])
marketplace_name = sys.argv[2]

try:
    data = json.loads(marketplace_file.read_text())
    if isinstance(data, dict) and "marketplaces" in data and isinstance(data["marketplaces"], list):
        entries = data["marketplaces"]
        style = "list"
    elif isinstance(data, dict) and "plugins" in data:
        entries = [data]
        style = "single"
    else:
        raise ValueError("Unexpected marketplace format")

    filtered = [m for m in entries if m.get("name") != marketplace_name]
    if len(filtered) == len(entries):
        sys.exit(0)

    if not filtered:
        marketplace_file.unlink()
        print(f"removed legacy marketplace file {marketplace_file}")
    elif style == "list" or len(filtered) > 1:
        marketplace_file.write_text(json.dumps({"marketplaces": filtered}, indent=2) + "\n")
        print(f"removed legacy marketplace entry \"{marketplace_name}\" from {marketplace_file}")
    else:
        marketplace_file.write_text(json.dumps(filtered[0], indent=2) + "\n")
        print(f"removed legacy marketplace entry \"{marketplace_name}\" from {marketplace_file}")
except Exception as exc:  # pragma: no cover
    print(f"skipping legacy marketplace cleanup: {exc}", file=sys.stderr)
    sys.exit(0)
PY
}

register_marketplace() {
  select_marketplace_add_command

  if ! command -v codex >/dev/null 2>&1; then
    MARKETPLACE_STATUS="not registered automatically (codex CLI not found)"
    warn "codex CLI not found; skipping marketplace registration. Run manually: $(manual_marketplace_add_command)"
    return
  fi

  if run_marketplace_add_command; then
    MARKETPLACE_STATUS="registered via \`$(marketplace_add_command) \"${REPO_PATH}\"\`"
    log "Registered marketplace \"${MARKETPLACE_NAME}\" via $(marketplace_add_command) ${REPO_PATH}"
  else
    MARKETPLACE_STATUS="not registered automatically ($(marketplace_add_command) failed)"
    warn "failed to register marketplace via codex; run manually: $(manual_marketplace_add_command)"
  fi
}

select_marketplace_add_command() {
  if [[ -n "$MARKETPLACE_ADD_MODE" ]]; then
    return 0
  fi

  if ! command -v codex >/dev/null 2>&1; then
    MARKETPLACE_ADD_MODE="plugin"
  elif CODEX_HOME="${CODEX_HOME}" codex plugin marketplace add --help >/dev/null 2>&1; then
    MARKETPLACE_ADD_MODE="plugin"
  elif CODEX_HOME="${CODEX_HOME}" codex marketplace add --help >/dev/null 2>&1; then
    MARKETPLACE_ADD_MODE="legacy"
  else
    MARKETPLACE_ADD_MODE="plugin"
  fi
}

marketplace_add_command() {
  select_marketplace_add_command

  case "$MARKETPLACE_ADD_MODE" in
    legacy)
      printf 'codex marketplace add'
      ;;
    *)
      printf 'codex plugin marketplace add'
      ;;
  esac
}

manual_marketplace_add_command() {
  printf 'CODEX_HOME="%s" %s "%s"' "${CODEX_HOME}" "$(marketplace_add_command)" "${REPO_PATH}"
}

run_marketplace_add_command() {
  select_marketplace_add_command

  case "$MARKETPLACE_ADD_MODE" in
    legacy)
      CODEX_HOME="${CODEX_HOME}" codex marketplace add "${REPO_PATH}"
      ;;
    *)
      CODEX_HOME="${CODEX_HOME}" codex plugin marketplace add "${REPO_PATH}"
      ;;
  esac
}

link_skill() {
  local source="$1"
  local target="$2"

  if [[ -L "$target" ]]; then
    local current
    current="$(readlink -f "$target")"
    if [[ "$current" == "$source" ]]; then
      return 0
    fi
    if [[ "$FORCE" -eq 0 ]]; then
      warn "skill ${target} already points to ${current}; use --force to replace"
      return 1
    fi
    rm -f "$target"
  elif [[ -e "$target" ]]; then
    if [[ "$FORCE" -eq 0 ]]; then
      warn "skill ${target} exists; use --force to replace"
      return 1
    fi
    rm -rf "$target"
  fi

  ln -s "$source" "$target"
  return 0
}

copy_skill() {
  local source="$1"
  local target="$2"

  if [[ -e "$target" || -L "$target" ]]; then
    if [[ "$FORCE" -eq 0 ]]; then
      warn "skill ${target} exists; use --force to replace"
      return 1
    fi
    rm -rf "$target"
  fi

  cp -R "$source" "$target"
  return 0
}

install_one_skill() {
  local source="$1"
  local target="$2"
  local name="$3"

  case "$SKILL_MODE" in
    link)
      if link_skill "$source" "$target"; then
        MANAGED_SKILLS+=("$name")
        ((++LINKED_COUNT))
      else
        ((++SKIPPED_COUNT))
      fi
      ;;
    copy)
      if copy_skill "$source" "$target"; then
        MANAGED_SKILLS+=("$name")
        ((++COPIED_COUNT))
      else
        ((++SKIPPED_COUNT))
      fi
      ;;
    auto)
      if link_skill "$source" "$target"; then
        MANAGED_SKILLS+=("$name")
        ((++LINKED_COUNT))
      else
        warn "falling back to copy mode for ${name}"
        if copy_skill "$source" "$target"; then
          MANAGED_SKILLS+=("$name")
          ((++COPIED_COUNT))
        else
          ((++SKIPPED_COUNT))
        fi
      fi
      ;;
  esac
}

install_skills() {
  mkdir -p "$SKILLS_ROOT"

  for skill_dir in "${REPO_PATH}"/plugins/*/skills/*; do
    [[ -d "$skill_dir" ]] || continue
    local name target
    name="$(basename "$skill_dir")"
    target="${SKILLS_ROOT}/${name}"
    install_one_skill "$skill_dir" "$target" "$name"
  done

  if [[ "$LINKED_COUNT" -gt 0 && "$COPIED_COUNT" -gt 0 ]]; then
    ACTUAL_SKILL_MODE="mixed"
  elif [[ "$COPIED_COUNT" -gt 0 ]]; then
    ACTUAL_SKILL_MODE="copy"
  elif [[ "$LINKED_COUNT" -gt 0 ]]; then
    ACTUAL_SKILL_MODE="link"
  else
    ACTUAL_SKILL_MODE="$SKILL_MODE"
  fi

  log "Skills installed: linked=${LINKED_COUNT} copied=${COPIED_COUNT} skipped=${SKIPPED_COUNT}"
}

write_state() {
  mkdir -p "$STATE_ROOT"
  python3 - "$STATE_PATH" "$REPO_PATH" "$CODEX_HOME" "$MARKETPLACE_NAME" "$INSTALL_METHOD" "$ACTUAL_SKILL_MODE" "$(current_ref "$REPO_PATH")" -- "${MANAGED_SKILLS[@]}" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

state_path = Path(sys.argv[1])
repo_path = sys.argv[2]
codex_home = sys.argv[3]
marketplace_name = sys.argv[4]
install_method = sys.argv[5]
skill_mode = sys.argv[6]
resolved_ref = sys.argv[7]
skills = sys.argv[9:]

payload = {
    "schema_version": 1,
    "repo_path": repo_path,
    "codex_home": codex_home,
    "marketplace_name": marketplace_name,
    "install_method": install_method,
    "skill_mode": skill_mode,
    "resolved_ref": resolved_ref,
    "managed_skills": skills,
    "installed_at": datetime.now(timezone.utc).isoformat(),
}

state_path.write_text(json.dumps(payload, indent=2) + "\n")
PY
}

main() {
  if [[ "$SKIP_REPO_SYNC" -eq 0 ]]; then
    clone_or_update_repo
  else
    log "Skipping repo sync for existing checkout at ${REPO_PATH}"
  fi

  if [[ ! -f "$CATALOG_PATH" ]]; then
    echo "Catalog not found at ${CATALOG_PATH}" >&2
    exit 1
  fi

  mkdir -p "$CODEX_HOME"
  cleanup_legacy_marketplace
  register_marketplace
  install_skills
  write_state

  cat <<EOF

Codex assets installed.
- Repository path: ${REPO_PATH}
- Managed ref: $(current_ref "$REPO_PATH")
- Install method: ${INSTALL_METHOD}
- Marketplace: ${MARKETPLACE_STATUS}
- Skills directory: ${SKILLS_ROOT}
- State file: ${STATE_PATH}

Next steps:
- Restart Codex if marketplace registration succeeded.
- Run "/plugins" or inspect available skills in your client.
- If marketplace registration failed or Codex is not installed, run manually: $(manual_marketplace_add_command)
EOF
}

main "$@"
