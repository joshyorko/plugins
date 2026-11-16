"""Approved-root path resolution, binary verification, and secret redaction policies."""

from __future__ import annotations

import hashlib
import os
import re
import shutil
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

PINNED_SHA256_BY_ARTIFACT: dict[str, str] = {
    "actions-runtime-1.1.1-linux64": "8c27fbed198aef16c98cdc7a45230fbc4708b06dff1c3523aeddfcdf7840efd0",
    "actions-runtime-1.1.1-windows64.exe": "0388d7f2e4080ba5c3a800bb556f8f01a65aa4834f3698eceb240888288fd8d2",
    "actions-runtime-1.1.1-macos-arm64": "6710985b3abf1b87b8719af8dad66d5b63f434a41de0f3eb355cd576fc570e91",
    "actions-runtime-1.1.0-linux64": "e163684dc296f632a177e95d4f355ac6d4f2299c62aab53789f43a642f41c0f1",
    "actions-runtime-1.1.0-windows64.exe": "44507e8dcd82fb367c616c699957611096bc05ff2ae5e0f41a11b98a98332098",
    "rcc-18.19.8-linux64": "ea8dc96e66d7a81536b03a344b4eaf44bbd94122585954e70290bd466f5de4b6",
    "rcc-18.19.8-windows64.exe": "f5e868af05382731f6675d98ed6ead7c7e95b250466bc5fee054b41817280a89",
    "rcc-18.19.8-macosarm64": "bf11dcee5a1e009a3f5b456bc8a24eb8f3f5d064eb7962c5e90956f13d4dfcba",
    "rcc-18.19.11-linux64": "4d68609044d3a39a1728d571d54c77a6174215adb40eb9acd69ce5ad34a6db1f",
    "rcc-18.19.11-windows64.exe": "58f43a3b3c54032740f224cb4a8bbf68c47c865cf2652084fb139b8ecc01cfd0",
}

KNOWN_VALID_RUNTIME_SHA256 = {
    PINNED_SHA256_BY_ARTIFACT["actions-runtime-1.1.1-linux64"],
    PINNED_SHA256_BY_ARTIFACT["actions-runtime-1.1.1-windows64.exe"],
    PINNED_SHA256_BY_ARTIFACT["actions-runtime-1.1.0-linux64"],
    PINNED_SHA256_BY_ARTIFACT["actions-runtime-1.1.0-windows64.exe"],
}

KNOWN_VALID_RCC_SHA256 = {
    PINNED_SHA256_BY_ARTIFACT["rcc-18.19.8-linux64"],
    PINNED_SHA256_BY_ARTIFACT["rcc-18.19.8-windows64.exe"],
    PINNED_SHA256_BY_ARTIFACT["rcc-18.19.11-linux64"],
    PINNED_SHA256_BY_ARTIFACT["rcc-18.19.11-windows64.exe"],
}

_SAFE_IDENTIFIER_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
_SECRET_ENV_KEY_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]{0,127}$")
_GENERIC_TOKEN_RE = re.compile(
    r"(?i)\b(?:bearer\s+[A-Za-z0-9._~+/=-]{8,}|(?:api[_-]?key|secret|token|password)\s*[:=]\s*['\"]?[^\s,'\";]{4,})"
)


class PolicyViolationError(ValueError):
    """Raised when an input violates path, identifier, or trust boundaries."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


try:
    from .models import BinaryHealth
except ModuleNotFoundError as exc:
    if exc.name != "pydantic":
        raise

    @dataclass
    class BinaryHealth:
        """Small stdlib-only binary report used by the extracted-package launcher."""

        name: str
        available: bool
        executable_path: str | None = None
        version: str | None = None
        sha256: str | None = None
        pinned_version: str | None = None
        digest_verified: bool = False

        def model_dump(self) -> dict[str, object]:
            return asdict(self)


def _default_home() -> Path:
    configured = os.environ.get("RCC_BOOTSTRAPPER_HOME")
    if configured:
        return Path(configured).expanduser().absolute()
    if sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA") or (Path.home() / "AppData" / "Local"))
    else:
        base = Path(os.environ.get("XDG_DATA_HOME") or (Path.home() / ".local" / "share"))
    return (base / "rcc-bootstrapper").absolute()


def default_datadir() -> Path:
    """Return the per-user external state directory for the bootstrapper."""
    configured = (
        os.environ.get("RCC_BOOTSTRAPPER_DATADIR")
        or os.environ.get("ACTION_SERVER_DATADIR")
        or os.environ.get("ACTIONS_RUNTIME_DATADIR")
    )
    if configured:
        return Path(configured).expanduser().absolute()
    return (_default_home() / "state" / "runtime").absolute()


def default_robocorp_home() -> Path:
    """Return isolated, persistent RCC cache and Holotree state for this user."""
    configured = os.environ.get("RCC_BOOTSTRAPPER_ROBOCORP_HOME")
    if configured:
        return Path(configured).expanduser().absolute()
    return (_default_home() / "state" / "robocorp").absolute()


def get_approved_roots(explicit_roots: list[str | Path] | None = None) -> list[Path]:
    """Return canonical approved root directories for project operations."""
    raw_items: list[str | Path] = []
    if explicit_roots:
        raw_items.extend(explicit_roots)
    else:
        env_val = os.environ.get("RCC_BOOTSTRAPPER_APPROVED_ROOTS", "").strip()
        if env_val:
            sep = ";" if (";" in env_val and os.pathsep == ";") else os.pathsep
            for part in env_val.split(sep):
                part = part.strip()
                if part:
                    raw_items.append(part)
        else:
            raw_items.append(Path.cwd())

    resolved: list[Path] = []
    seen: set[Path] = set()
    for item in raw_items:
        p = Path(item).expanduser().resolve()
        if p not in seen:
            seen.add(p)
            resolved.append(p)
    return resolved


def get_trusted_project_roots() -> list[Path]:
    """Return operator-configured roots whose code may be executed by the bootstrapper."""
    raw = os.environ.get("RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS", "").strip()
    if not raw:
        return []
    sep = ";" if (";" in raw and os.pathsep == ";") else os.pathsep
    roots: list[Path] = []
    seen: set[Path] = set()
    for item in raw.split(sep):
        item = item.strip()
        if item:
            root = Path(item).expanduser().resolve()
            if root not in seen:
                seen.add(root)
                roots.append(root)
    return roots


def is_trusted_project(project_dir: str | Path, trusted_roots: list[Path] | None = None) -> bool:
    """Check project containment in operator-owned executable trust roots."""
    target = Path(project_dir).expanduser().resolve()
    roots = get_trusted_project_roots() if trusted_roots is None else trusted_roots
    return any(target.is_relative_to(root) for root in roots)


def _check_no_symlink_escape(candidate: Path, root: Path) -> None:
    """Verify no existing path component between root and candidate is a symlink escaping root."""
    current = root
    try:
        rel_parts = candidate.relative_to(root).parts
    except ValueError as exc:
        raise PolicyViolationError(
            "path_outside_approved_root",
            "Target path resolves outside the approved workspace roots.",
        ) from exc

    for part in rel_parts:
        current = current / part
        if current.is_symlink():
            resolved_target = current.resolve()
            if not resolved_target.is_relative_to(root):
                raise PolicyViolationError(
                    "symlink_escape_rejected",
                    "Symlink inside target path points outside the approved root.",
                )


def resolve_approved_target(
    target_path: str | Path,
    *,
    approved_roots: list[str | Path] | None = None,
    allow_root_itself: bool = False,
) -> tuple[Path, Path]:
    """Resolve target_path canonically and verify it resides inside an approved root.

    Returns (canonical_target, matching_approved_root).
    """
    raw = str(target_path or "").strip()
    if not raw or "\x00" in raw:
        raise PolicyViolationError("invalid_path", "Target path must be a non-empty valid path.")

    # Reject explicit parent traversal tokens before resolution as defense-in-depth
    raw_parts = Path(raw).parts
    if ".." in raw_parts:
        raise PolicyViolationError(
            "path_traversal_rejected",
            "Parent directory traversal ('..') is not permitted in target paths.",
        )

    roots = get_approved_roots(approved_roots)
    if not roots:
        raise PolicyViolationError(
            "no_approved_roots",
            "No approved project roots are configured.",
        )

    expanded = Path(raw).expanduser()
    if not expanded.is_absolute():
        expanded = roots[0] / expanded

    # Check symlink components on lexical path relative to any matching root before full resolve
    for root in roots:
        try:
            rel = expanded.relative_to(root)
        except ValueError:
            continue
        probe = root
        for part in rel.parts:
            probe = probe / part
            if probe.exists() or probe.is_symlink():
                if probe.is_symlink():
                    resolved_link = probe.resolve()
                    if not resolved_link.is_relative_to(root):
                        raise PolicyViolationError(
                            "symlink_escape_rejected",
                            "Symlink in target path escapes approved root.",
                        )

    canonical = expanded.resolve()
    for root in roots:
        if canonical.is_relative_to(root):
            if not allow_root_itself and canonical == root:
                raise PolicyViolationError(
                    "target_is_approved_root",
                    "Target directory must be a project subdirectory inside an approved root, not the root itself.",
                )
            _check_no_symlink_escape(canonical, root)
            return canonical, root

    raise PolicyViolationError(
        "path_outside_approved_root",
        "Target path is outside all approved project roots.",
    )


def validate_safe_identifier(value: str, *, field_name: str = "identifier") -> str:
    """Validate a template, task, or action identifier against shell/path metacharacters."""
    cleaned = (value or "").strip()
    if not _SAFE_IDENTIFIER_RE.match(cleaned):
        raise PolicyViolationError(
            f"invalid_{field_name}",
            f"Invalid {field_name}: must contain only letters, digits, '.', '_', or '-'.",
        )
    return cleaned


def validate_secret_env_map(secrets: dict[str, str] | None) -> dict[str, str]:
    """Validate secret environment variable names and ensure non-null string values."""
    if not secrets:
        return {}
    validated: dict[str, str] = {}
    for key, val in secrets.items():
        if not _SECRET_ENV_KEY_RE.match(str(key)):
            raise PolicyViolationError(
                "invalid_secret_key",
                "Secret environment key must be a valid environment variable name.",
            )
        if "\x00" in str(val):
            raise PolicyViolationError(
                "invalid_secret_value",
                "Secret value must not contain NUL bytes.",
            )
        validated[str(key)] = str(val)
    return validated


def redact_secrets(text: str, secret_values: list[str] | set[str] | None = None) -> str:
    """Scrub known secret values and common token/secret patterns from logs or error strings."""
    if not text:
        return ""
    redacted = text
    if secret_values:
        # Sort longest first so partial substrings don't leave fragments
        for secret in sorted({s for s in secret_values if s and len(s) >= 3}, key=len, reverse=True):
            redacted = redacted.replace(secret, "[REDACTED]")
    redacted = _GENERIC_TOKEN_RE.sub("[REDACTED_SECRET]", redacted)
    return redacted


def check_destination_empty(target_dir: Path) -> tuple[bool, bool, list[str]]:
    """Return (exists, is_empty, existing_entries) including dotfiles and .git."""
    if not target_dir.exists() and not target_dir.is_symlink():
        return False, True, []
    if not target_dir.is_dir() or target_dir.is_symlink():
        return True, False, [target_dir.name]
    entries = sorted(p.name for p in target_dir.iterdir())
    return True, len(entries) == 0, entries


def sha256_file(path: Path) -> str:
    """Compute hex SHA-256 of a file."""
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def resolve_binary(name: str, explicit_path: str | Path | None = None) -> Path | None:
    """Locate action-server or rcc binary on disk or PATH."""
    env_key = "RCC_BOOTSTRAPPER_ACTION_SERVER_BIN" if name == "action-server" else "RCC_BOOTSTRAPPER_RCC_BIN"
    candidate = explicit_path or os.environ.get(env_key)
    if candidate:
        p = Path(candidate).expanduser()
        if p.is_file():
            return p.resolve()
        found = shutil.which(str(candidate))
        return Path(found).resolve() if found else None

    versioned_bin = _default_home() / "versions" / "0.6.0" / "bin"
    versioned_name = (
        "actions-runtime-1.1.1-windows64.exe" if sys.platform == "win32" else "actions-runtime-1.1.1-linux64"
    )
    if name == "rcc":
        versioned_name = "rcc-windows64.exe" if sys.platform == "win32" else "rcc-linux64"
    versioned_probe = versioned_bin / versioned_name
    if versioned_probe.is_file():
        return versioned_probe.resolve()

    found = shutil.which(name)
    if found:
        return Path(found).resolve()

    # Check common Linuxbrew / local locations
    fallback_dirs = [
        Path("/home/linuxbrew/.linuxbrew/bin"),
        Path.home() / ".local" / "bin",
    ]
    exe_name = f"{name}.exe" if sys.platform == "win32" and not name.endswith(".exe") else name
    for directory in fallback_dirs:
        probe = directory / exe_name
        if probe.is_file():
            return probe.resolve()
    return None


def inspect_binary_health(
    name: str,
    *,
    explicit_path: str | Path | None = None,
    pinned_version: str | None = None,
) -> BinaryHealth:
    """Inspect version and SHA-256 digest of action-server or rcc."""
    resolved = resolve_binary(name, explicit_path)
    if not resolved or not resolved.is_file():
        return BinaryHealth(
            name=name,
            available=False,
            pinned_version=pinned_version,
        )

    digest = sha256_file(resolved)
    version_str: str | None = None
    try:
        cmd = [str(resolved), "version"]
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
        raw_out = (proc.stdout or proc.stderr or "").strip()
        if not raw_out and name == "rcc":
            proc2 = subprocess.run([str(resolved), "--version"], capture_output=True, text=True, timeout=10, check=False)
            raw_out = (proc2.stdout or proc2.stderr or "").strip()
        if raw_out:
            first_line = raw_out.splitlines()[0].strip()
            version_str = first_line
    except Exception:
        version_str = None

    known_set = KNOWN_VALID_RUNTIME_SHA256 if name == "action-server" else KNOWN_VALID_RCC_SHA256
    return BinaryHealth(
        name=name,
        available=True,
        executable_path=str(resolved),
        version=version_str,
        sha256=digest,
        pinned_version=pinned_version,
        digest_verified=digest in known_set,
    )


def verify_binary_before_execution(
    name: str,
    explicit_path: str | Path | None = None,
    *,
    expected_sha256: str | None = None,
) -> tuple[Path, str]:
    """Resolve binary and enforce fail-closed SHA-256 verification before execution.

    Raises PolicyViolationError if the binary is missing or its SHA-256 digest
    does not match the pinned release allowlist (or explicit expected_sha256).
    """
    resolved = resolve_binary(name, explicit_path)
    if not resolved or not resolved.is_file():
        raise PolicyViolationError(
            f"{name.replace('-', '_')}_binary_missing",
            f"Required binary '{name}' was not found on PATH.",
        )

    actual_digest = sha256_file(resolved)
    allowed = (
        {expected_sha256.lower()}
        if expected_sha256
        else (KNOWN_VALID_RUNTIME_SHA256 if name == "action-server" else KNOWN_VALID_RCC_SHA256)
    )
    if actual_digest.lower() not in allowed:
        raise PolicyViolationError(
            "untrusted_binary_digest",
            f"Refusing to execute '{resolved}': SHA-256 '{actual_digest}' is not in the pinned release allowlist for {name}.",
        )
    return resolved, actual_digest
