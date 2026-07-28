#!/usr/bin/env bash
set -euo pipefail

REPO_OWNER="joshyorko"
REPO_NAME="plugins"
REPO_URL_DEFAULT="https://github.com/${REPO_OWNER}/${REPO_NAME}.git"
REPO_PATH_DEFAULT="${HOME}/src/${REPO_NAME}"
CODEX_HOME_DEFAULT="${HOME}/.codex"
MARKETPLACE_NAME_DEFAULT="plugins"
API_BASE="https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}"

REPO_URL="${AGENT_SKILLS_REPO_URL:-$REPO_URL_DEFAULT}"
REPO_PATH="${AGENT_SKILLS_REPO_PATH:-$REPO_PATH_DEFAULT}"
CODEX_HOME="${AGENT_SKILLS_CODEX_HOME:-$CODEX_HOME_DEFAULT}"
MARKETPLACE_NAME="${AGENT_SKILLS_MARKETPLACE_NAME:-$MARKETPLACE_NAME_DEFAULT}"
SKILL_MODE="${AGENT_SKILLS_SKILL_MODE:-auto}"
INSTALL_METHOD="${AGENT_SKILLS_INSTALL_METHOD:-auto}"
REF_SPEC="${AGENT_SKILLS_REF:-}"
FORCE=0

usage() {
  cat <<'EOF'
Usage: install.sh [options]

Remote bootstrap for the Agent Skills marketplace.

Options:
  --repo-path PATH          Stable local checkout path (default: ~/src/plugins)
  --repo-url URL            Git clone URL (default: https://github.com/joshyorko/plugins.git)
  --codex-home PATH         Codex home directory (default: ~/.codex)
  --marketplace-name NAME   Marketplace name (default: plugins)
  --skill-mode MODE         auto (default), link, or copy
  --install-method MODE     auto (default), git, or archive
  --ref REF                 Release tag, branch, or commit to install
  --force                   Replace conflicting skill entries
  -h, --help                Show this help message

Examples:
  curl -fsSL https://raw.githubusercontent.com/joshyorko/plugins/main/install.sh | bash
  curl -fsSL https://raw.githubusercontent.com/joshyorko/plugins/main/install.sh | bash -s -- --ref v1.2.3
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

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required command not found: $1" >&2
    exit 1
  fi
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
  auto|git|archive) ;;
  *)
    echo "Invalid install method: ${INSTALL_METHOD}" >&2
    exit 1
    ;;
esac

REPO_PATH="$(normalize_path "$REPO_PATH")"
CODEX_HOME="$(normalize_path "$CODEX_HOME")"

TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMP_ROOT"' EXIT

latest_release_tag() {
  require_command curl

  local response
  if ! response="$(curl -fsSL -H 'Accept: application/vnd.github+json' "${API_BASE}/releases/latest" 2>/dev/null)"; then
    return 1
  fi

  python3 - <<'PY' "$response"
import json
import sys

payload = json.loads(sys.argv[1])
tag = payload.get("tag_name")
if not tag:
    raise SystemExit(1)
print(tag)
PY
}

verify_sha256() {
  local checksum_file="$1"
  local asset_file="$2"

  python3 - "$checksum_file" "$asset_file" <<'PY'
import hashlib
import sys
from pathlib import Path

checksum_path = Path(sys.argv[1])
asset_path = Path(sys.argv[2])
checksums = {}
for line in checksum_path.read_text().splitlines():
    line = line.strip()
    if not line:
        continue
    digest, name = line.split(None, 1)
    checksums[name.lstrip("* ")] = digest

expected = checksums.get(asset_path.name)
if not expected:
    raise SystemExit(f"missing checksum for {asset_path.name}")

actual = hashlib.sha256(asset_path.read_bytes()).hexdigest()
if actual != expected:
    raise SystemExit(f"checksum mismatch for {asset_path.name}: expected {expected}, got {actual}")
PY
}

extract_archive() {
  local archive_path="$1"
  local destination="$2"

  python3 - "$archive_path" "$destination" <<'PY'
import shutil
import sys
import tarfile
import zipfile
from pathlib import Path

archive = Path(sys.argv[1])
destination = Path(sys.argv[2])
staging = destination.parent / (destination.name + ".extracting")

if staging.exists():
    shutil.rmtree(staging)
staging.mkdir(parents=True)

if archive.suffix == ".zip":
    with zipfile.ZipFile(archive) as zf:
        zf.extractall(staging)
else:
    with tarfile.open(archive) as tf:
        tf.extractall(staging)

children = [child for child in staging.iterdir()]
source_root = children[0] if len(children) == 1 and children[0].is_dir() else staging

if destination.exists():
    shutil.rmtree(destination)
destination.parent.mkdir(parents=True, exist_ok=True)
if source_root == staging:
    destination.mkdir(parents=True, exist_ok=True)
    for child in staging.iterdir():
      shutil.move(str(child), destination / child.name)
else:
    shutil.move(str(source_root), destination)

shutil.rmtree(staging, ignore_errors=True)
PY
}

choose_install_method() {
  if [[ "$INSTALL_METHOD" != "auto" ]]; then
    printf '%s\n' "$INSTALL_METHOD"
    return 0
  fi

  if [[ -d "${REPO_PATH}/.git" ]]; then
    printf 'git\n'
    return 0
  fi

  if [[ -f "${REPO_PATH}/marketplaces/catalog.json" ]]; then
    printf 'archive\n'
    return 0
  fi

  if command -v git >/dev/null 2>&1; then
    printf 'git\n'
  else
    printf 'archive\n'
  fi
}

install_from_git() {
  require_command git

  if [[ -d "${REPO_PATH}/.git" ]]; then
    log "Updating existing git checkout at ${REPO_PATH}"
    git -C "${REPO_PATH}" fetch --tags --prune
  elif [[ -e "${REPO_PATH}" ]]; then
    echo "Target path ${REPO_PATH} exists but is not a git checkout. Use --install-method archive or remove it first." >&2
    exit 1
  else
    mkdir -p "$(dirname "${REPO_PATH}")"
    log "Cloning ${REPO_URL} into ${REPO_PATH}"
    git clone "${REPO_URL}" "${REPO_PATH}"
  fi

  if [[ -n "$REF_SPEC" ]]; then
    if git -C "${REPO_PATH}" rev-parse --verify --quiet "${REF_SPEC}^{commit}" >/dev/null; then
      git -C "${REPO_PATH}" checkout --force "${REF_SPEC}"
    elif git -C "${REPO_PATH}" fetch origin "${REF_SPEC}" --depth=1 >/dev/null 2>&1; then
      git -C "${REPO_PATH}" checkout --force FETCH_HEAD
    else
      echo "Unable to resolve ref ${REF_SPEC}" >&2
      exit 1
    fi
  else
    git -C "${REPO_PATH}" pull --ff-only
  fi

  local resolved_ref
  if [[ -n "$REF_SPEC" ]]; then
    resolved_ref="$REF_SPEC"
  else
    if resolved_ref="$(git -C "${REPO_PATH}" describe --tags --exact-match 2>/dev/null)"; then
      :
    else
      resolved_ref="$(git -C "${REPO_PATH}" symbolic-ref --short -q HEAD 2>/dev/null || git -C "${REPO_PATH}" rev-parse --short HEAD)"
    fi
  fi

  bash "${REPO_PATH}/scripts/install-codex-assets.sh" \
    --repo-path "${REPO_PATH}" \
    --codex-home "${CODEX_HOME}" \
    --marketplace-name "${MARKETPLACE_NAME}" \
    --skill-mode "${SKILL_MODE}" \
    --install-method git \
    --resolved-ref "${resolved_ref}" \
    --skip-repo-sync \
    $( [[ "$FORCE" -eq 1 ]] && printf '%s' '--force' )
}

install_from_archive() {
  require_command curl
  require_command python3

  local resolved_ref="$REF_SPEC"
  local release_tag=""
  if [[ -z "$resolved_ref" ]]; then
    if release_tag="$(latest_release_tag)"; then
      resolved_ref="$release_tag"
    else
      resolved_ref="main"
      warn "No GitHub release found; falling back to main branch archive without checksum verification."
    fi
  fi

  local archive_path checksum_path archive_url checksum_url
  if [[ "$resolved_ref" == "main" ]]; then
    archive_path="${TMP_ROOT}/plugins-main.tar.gz"
    archive_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/archive/refs/heads/main.tar.gz"
    curl -fsSL "$archive_url" -o "$archive_path"
  else
    archive_path="${TMP_ROOT}/plugins-${resolved_ref}.tar.gz"
    checksum_path="${TMP_ROOT}/SHA256SUMS"
    archive_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${resolved_ref}/plugins-${resolved_ref}.tar.gz"
    checksum_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${resolved_ref}/SHA256SUMS"
    curl -fsSL "$archive_url" -o "$archive_path" || {
      echo "Archive mode requires a published release tag. Could not download assets for ${resolved_ref}." >&2
      exit 1
    }
    curl -fsSL "$checksum_url" -o "$checksum_path"
    verify_sha256 "$checksum_path" "$archive_path"
  fi

  log "Extracting ${resolved_ref} into ${REPO_PATH}"
  extract_archive "$archive_path" "$REPO_PATH"

  bash "${REPO_PATH}/scripts/install-codex-assets.sh" \
    --repo-path "${REPO_PATH}" \
    --codex-home "${CODEX_HOME}" \
    --marketplace-name "${MARKETPLACE_NAME}" \
    --skill-mode "${SKILL_MODE}" \
    --install-method archive \
    --resolved-ref "${resolved_ref}" \
    --skip-repo-sync \
    $( [[ "$FORCE" -eq 1 ]] && printf '%s' '--force' )
}

main() {
  local method
  method="$(choose_install_method)"
  log "Using install method: ${method}"

  case "$method" in
    git)
      install_from_git
      ;;
    archive)
      install_from_archive
      ;;
  esac
}

main "$@"
