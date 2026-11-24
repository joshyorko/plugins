#!/usr/bin/env python3
"""Cross-platform lifecycle manager and MCP verifier for the RCC Actions Bootstrapper.

Enforces:
- Mandatory fail-closed SHA-256 verification of action-server and rcc before execution.
- External per-user datadir (never inside the git checkout unless explicitly overridden for isolated temp tests).
- Strict instance ownership via bootstrapper-instance.json + PID/process identity checks.
- Refusal to overwrite, signal, or attach implicitly to unrelated processes on occupied ports.
- Coordinated SQLite online backup before upgrades or migrations, plus offline restore.
- Full Streamable HTTP /mcp handshake + tool/resource/prompt verification under actual authorization.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import secrets
import shlex
import shutil
import signal
import socket
import sqlite3
import stat
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

if __name__ == "__main__":
    sys.dont_write_bytecode = True

SCRIPT_DIR = Path(__file__).resolve().parent
PLUGIN_ROOT = SCRIPT_DIR.parent
from install_bootstrapper import (  # noqa: E402
    PACKAGE_SHA256,
    verify_extracted_package,
    verify_installed_package,
)

_configured_package_dir = os.environ.get("RCC_BOOTSTRAPPER_PACKAGE_DIR")
if os.environ.get("RCC_BOOTSTRAPPER_HOME"):
    _bootstrapper_home = Path(os.environ["RCC_BOOTSTRAPPER_HOME"]).expanduser()
elif sys.platform == "win32":
    local_app_data = os.environ.get("LOCALAPPDATA") or (Path.home() / "AppData" / "Local")
    _bootstrapper_home = Path(local_app_data) / "rcc-bootstrapper"
else:
    xdg_data_home = os.environ.get("XDG_DATA_HOME") or (Path.home() / ".local" / "share")
    _bootstrapper_home = Path(xdg_data_home) / "rcc-bootstrapper"
_installed_package_dir = _bootstrapper_home / "versions" / "0.6.0" / "package"
if _configured_package_dir:
    BOOTSTRAPPER_PKG_DIR = Path(_configured_package_dir).expanduser().resolve()
elif _installed_package_dir.is_dir():
    receipt = json.loads((_installed_package_dir.parent / "install-receipt.json").read_text(encoding="utf-8"))
    if receipt.get("package_sha256") != PACKAGE_SHA256:
        raise RuntimeError("installed package receipt does not match the pinned package version")
    verify_installed_package(_installed_package_dir.parent / "package.zip", _installed_package_dir)
    BOOTSTRAPPER_PKG_DIR = _installed_package_dir.resolve()
else:
    BOOTSTRAPPER_PKG_DIR = PLUGIN_ROOT / "bootstrapper"
if _configured_package_dir:
    _configured_archive = BOOTSTRAPPER_PKG_DIR.parent / "package.zip"
    _candidate_sha256 = os.environ.get("RCC_BOOTSTRAPPER_CANDIDATE_SHA256")
    if _candidate_sha256 is not None:
        verify_installed_package(_configured_archive, BOOTSTRAPPER_PKG_DIR, expected_sha256=_candidate_sha256)
    elif _configured_archive.is_file():
        verify_extracted_package(_configured_archive, BOOTSTRAPPER_PKG_DIR)
BOOTSTRAPPER_SRC_DIR = BOOTSTRAPPER_PKG_DIR / "src"

if str(BOOTSTRAPPER_SRC_DIR) not in sys.path:
    sys.path.insert(0, str(BOOTSTRAPPER_SRC_DIR))

from rcc_bootstrapper.policy import (  # noqa: E402
    KNOWN_VALID_RCC_SHA256,
    KNOWN_VALID_RUNTIME_SHA256,
    PINNED_SHA256_BY_ARTIFACT,
    PolicyViolationError,
    default_datadir,
    get_approved_roots,
    get_trusted_project_roots,
    default_robocorp_home,
    inspect_binary_health,
    redact_secrets,
    validate_safe_identifier,
    verify_binary_before_execution,
)

INSTANCE_METADATA_FILENAME = "bootstrapper-instance.json"


def _ensure_private_runtime_dir(path: Path, label: str) -> Path:
    path = path.expanduser().absolute()
    if path.is_symlink():
        raise PolicyViolationError("symlink_runtime_home", f"Refusing symlink runtime directory for {label}.")
    if path.exists() and not path.is_dir():
        raise PolicyViolationError("invalid_runtime_home", f"Runtime directory for {label} is not a directory.")
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.is_symlink() or not path.is_dir():
        raise PolicyViolationError("invalid_runtime_home", f"Runtime directory for {label} changed during startup.")
    return path.resolve()


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _windows_process_state(pid: int) -> tuple[bool, str | None]:
    """Query liveness and creation identity without sending a Windows signal."""
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    kernel32.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        if ctypes.get_last_error() == 5:
            raise PermissionError(f"Cannot inspect process {pid}")
        return False, None
    try:
        code = wintypes.DWORD()
        if not kernel32.GetExitCodeProcess(handle, ctypes.byref(code)):
            raise OSError(ctypes.get_last_error(), "GetExitCodeProcess failed")
        if code.value != 259:  # STILL_ACTIVE
            return False, None
        creation, exit_time, kernel, user = (wintypes.FILETIME() for _ in range(4))
        if not kernel32.GetProcessTimes(
            handle, ctypes.byref(creation), ctypes.byref(exit_time), ctypes.byref(kernel), ctypes.byref(user)
        ):
            raise OSError(ctypes.get_last_error(), "GetProcessTimes failed")
        ticks = (creation.dwHighDateTime << 32) | creation.dwLowDateTime
        return True, f"windows-creation:{ticks}"
    finally:
        kernel32.CloseHandle(handle)


def _proc_start_token(pid: int) -> str | None:
    """Return an OS-specific process identity token (e.g. /proc/<pid>/stat starttime on Linux)."""
    if pid <= 0:
        return None
    if sys.platform == "win32":
        return _windows_process_state(pid)[1]
    if sys.platform.startswith("linux"):
        stat_path = Path(f"/proc/{pid}/stat")
        if not stat_path.is_file():
            return None
        try:
            content = stat_path.read_text(encoding="utf-8", errors="ignore")
            rparen = content.rfind(")")
            if rparen == -1:
                return None
            fields = content[rparen + 2 :].split()
            # field 19 after ')' is starttime (22nd field overall in proc(5))
            if len(fields) >= 20:
                return f"linux-starttime:{fields[19]}"
        except Exception:
            return None
    return f"pid:{pid}"


_SPAWNED_RUNTIME_PROCS: dict[int, subprocess.Popen[str]] = {}


def _is_pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    spawned = _SPAWNED_RUNTIME_PROCS.get(pid)
    if spawned is not None and spawned.poll() is not None:
        _SPAWNED_RUNTIME_PROCS.pop(pid, None)
        return False
    if sys.platform == "win32":
        return _windows_process_state(pid)[0]
    if sys.platform.startswith("linux"):
        stat_path = Path(f"/proc/{pid}/stat")
        if not stat_path.is_file():
            return False
        try:
            content = stat_path.read_text(encoding="utf-8", errors="ignore")
            rparen = content.rfind(")")
            if rparen != -1:
                state_char = content[rparen + 2 : rparen + 3]
                if state_char == "Z":
                    return False
        except Exception:
            pass
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except OSError:
        return False


def _is_port_open(host: str, port: int, timeout: float = 0.4) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def _instance_metadata_path(datadir: Path) -> Path:
    return datadir / INSTANCE_METADATA_FILENAME


def _load_instance_metadata(datadir: Path) -> dict[str, Any] | None:
    meta_path = _instance_metadata_path(datadir)
    if not meta_path.is_file():
        return None
    try:
        return json.loads(meta_path.read_text(encoding="utf-8"))
    except Exception:
        return None


def _save_instance_metadata(datadir: Path, payload: dict[str, Any]) -> None:
    datadir.mkdir(parents=True, exist_ok=True)
    meta_path = _instance_metadata_path(datadir)
    tmp = meta_path.with_suffix(f".tmp-{uuid.uuid4().hex[:6]}")
    tmp.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    os.replace(tmp, meta_path)


def inspect_owned_instance(datadir: Path) -> dict[str, Any]:
    """Inspect whether datadir has a live owned runtime or stale metadata."""
    meta = _load_instance_metadata(datadir)
    if not meta:
        return {"owned_metadata_present": False, "state": "stopped", "metadata": None}

    pid = int(meta.get("pid") or 0)
    expected_token = meta.get("proc_start_token")
    alive = _is_pid_alive(pid)
    actual_token = _proc_start_token(pid) if alive else None

    if not alive or (expected_token and actual_token and expected_token != actual_token):
        return {
            "owned_metadata_present": True,
            "state": "stale",
            "stale_reason": "pid_not_alive" if not alive else "pid_reused_by_different_process",
            "metadata": meta,
        }

    host = str(meta.get("host") or "127.0.0.1")
    port = int(meta.get("port") or 8765)
    listening = _is_port_open(host, port)
    return {
        "owned_metadata_present": True,
        "state": "running" if listening else "starting_or_unresponsive",
        "listening": listening,
        "metadata": meta,
    }


def _reconcile_stale_metadata(datadir: Path) -> bool:
    """Archive and clear stale bootstrapper-instance.json if the recorded PID is dead or reused."""
    status = inspect_owned_instance(datadir)
    if status["state"] == "stale":
        meta_path = _instance_metadata_path(datadir)
        if meta_path.is_file():
            stale_archive = datadir / f"bootstrapper-instance.stale-{int(time.time())}.json"
            os.replace(meta_path, stale_archive)
        return True
    return False


def coordinated_sqlite_backup(
    datadir: Path,
    *,
    label: str = "manual",
) -> dict[str, Any]:
    """Create a coordinated SQLite online backup of server.db and runs.db while no writer is active."""
    status = inspect_owned_instance(datadir)
    if status["state"] == "running":
        raise PolicyViolationError(
            "backup_requires_stopped_server",
            "Refusing to run SQLite backup while the owned runtime instance is actively running; stop it first.",
        )

    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    safe_label = "".join(c if c.isalnum() or c in "._-" else "-" for c in label)[:48] or "snapshot"
    backup_dir = datadir / "backups" / f"{timestamp}-{safe_label}"
    backup_dir.mkdir(parents=True, exist_ok=True)

    backed_up_files: list[dict[str, Any]] = []
    for db_name in ("server.db", "runs.db"):
        src_db = datadir / db_name
        if src_db.is_file():
            dst_db = backup_dir / db_name
            src_conn = sqlite3.connect(str(src_db))
            dst_conn = sqlite3.connect(str(dst_db))
            try:
                with dst_conn:
                    src_conn.backup(dst_conn)
            finally:
                dst_conn.close()
                src_conn.close()
            digest = hashlib.sha256(dst_db.read_bytes()).hexdigest()
            backed_up_files.append(
                {
                    "name": db_name,
                    "size_bytes": dst_db.stat().st_size,
                    "sha256": digest,
                }
            )

    for json_name in ("server-info.json", INSTANCE_METADATA_FILENAME):
        src_json = datadir / json_name
        if src_json.is_file():
            dst_json = backup_dir / json_name
            shutil.copy2(src_json, dst_json)
            backed_up_files.append(
                {
                    "name": json_name,
                    "size_bytes": dst_json.stat().st_size,
                    "sha256": hashlib.sha256(dst_json.read_bytes()).hexdigest(),
                }
            )

    manifest = {
        "created_at": _now_iso(),
        "datadir": str(datadir),
        "backup_dir": str(backup_dir),
        "label": safe_label,
        "files": backed_up_files,
    }
    (backup_dir / "backup-manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return manifest


def restore_sqlite_backup(datadir: Path, backup_dir: Path) -> dict[str, Any]:
    """Restore backed up SQLite files into datadir while the runtime is stopped."""
    status = inspect_owned_instance(datadir)
    if status["state"] == "running":
        raise PolicyViolationError(
            "restore_requires_stopped_server",
            "Refusing to restore SQLite backup while the owned runtime instance is running; stop it first.",
        )
    if not backup_dir.is_dir():
        raise PolicyViolationError("backup_dir_not_found", f"Backup directory '{backup_dir}' does not exist.")

    datadir.mkdir(parents=True, exist_ok=True)
    restored: list[str] = []
    for db_name in ("server.db", "runs.db"):
        src_db = backup_dir / db_name
        if src_db.is_file():
            dst_db = datadir / db_name
            for wal_suffix in ("-wal", "-shm"):
                wal_file = Path(f"{dst_db}{wal_suffix}")
                if wal_file.exists():
                    wal_file.unlink()
            shutil.copy2(src_db, dst_db)
            restored.append(db_name)

    return {
        "restored_at": _now_iso(),
        "datadir": str(datadir),
        "backup_dir": str(backup_dir),
        "restored_files": restored,
    }


def _parse_mcp_response(raw_bytes: bytes, content_type: str) -> list[dict[str, Any]]:
    """Parse application/json or text/event-stream JSON-RPC messages from Actions Runtime /mcp."""
    text = raw_bytes.decode("utf-8", errors="replace").strip()
    if not text:
        return []
    if "text/event-stream" in content_type.lower() or text.startswith("event:") or text.startswith("data:"):
        messages: list[dict[str, Any]] = []
        data_lines: list[str] = []
        for line in text.splitlines():
            if line.startswith("data:"):
                data_lines.append(line[len("data:") :].strip())
            elif not line.strip() and data_lines:
                joined = "\n".join(data_lines)
                data_lines = []
                try:
                    messages.append(json.loads(joined))
                except Exception:
                    pass
        if data_lines:
            try:
                messages.append(json.loads("\n".join(data_lines)))
            except Exception:
                pass
        return messages
    parsed = json.loads(text)
    return parsed if isinstance(parsed, list) else [parsed]


class McpHttpClient:
    """Minimal Streamable HTTP MCP client for Actions Runtime /mcp verification."""

    def __init__(self, mcp_url: str, *, api_key: str | None = None, timeout: float = 30.0) -> None:
        self.mcp_url = mcp_url
        self.api_key = api_key
        self.timeout = timeout
        self.session_id: str | None = None
        self._next_id = 1

    def request(
        self,
        method: str,
        params: dict[str, Any] | None = None,
        *,
        is_notification: bool = False,
    ) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "jsonrpc": "2.0",
            "method": method,
        }
        if params is not None:
            payload["params"] = params
        if not is_notification:
            req_id = self._next_id
            self._next_id += 1
            payload["id"] = req_id

        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"
        if self.session_id:
            headers["Mcp-Session-Id"] = self.session_id

        req = urllib.request.Request(
            self.mcp_url,
            data=json.dumps(payload).encode("utf-8"),
            headers=headers,
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                sid = resp.headers.get("Mcp-Session-Id") or resp.headers.get("mcp-session-id")
                if sid:
                    self.session_id = sid
                ctype = resp.headers.get("Content-Type", "application/json")
                body = resp.read()
                if is_notification or resp.status == 202:
                    return {"status": resp.status, "notification_accepted": True}
                messages = _parse_mcp_response(body, ctype)
                for msg in messages:
                    if "id" in msg:
                        return msg
                return messages[0] if messages else {}
        except urllib.error.HTTPError as exc:
            err_body = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"MCP HTTP {exc.code} on {method}: {err_body}") from exc


def _prepare_runtime_launch(
    *,
    datadir: Path,
    host: str = "127.0.0.1",
    port: int = 8765,
    package_dir: Path = BOOTSTRAPPER_PKG_DIR,
    approved_roots: list[str | Path] | None = None,
    trusted_roots: list[str | Path] | None = None,
    api_key: str | None = None,
    runtime_binary: str | Path | None = None,
    rcc_binary: str | Path | None = None,
    runtime_home: str | Path | None = None,
    robocorp_home: str | Path | None = None,
    expected_runtime_sha256: str | None = None,
    expected_package_sha256: str | None = None,
    allow_already_running: bool = True,
) -> dict[str, Any]:
    """Validate auth, state dirs, port ownership, binary digests, and build launch context."""
    resolved_package_dir = package_dir.expanduser().resolve()
    installed_archive = resolved_package_dir.parent / "package.zip"
    installed_receipt = installed_archive.parent / "install-receipt.json"
    if expected_package_sha256 is not None or (installed_archive.is_file() and installed_receipt.is_file()):
        verify_installed_package(installed_archive, resolved_package_dir, expected_sha256=expected_package_sha256)
    elif installed_archive.is_file():
        verify_extracted_package(installed_archive, resolved_package_dir)
    resolved_api_key = api_key if api_key is not None else os.environ.get("RCC_BOOTSTRAPPER_API_KEY")
    if not resolved_api_key or not resolved_api_key.strip():
        raise PolicyViolationError(
            "runtime_api_key_required",
            "Set RCC_BOOTSTRAPPER_API_KEY or pass --api-key before starting the MCP endpoint.",
        )

    raw_datadir = datadir.expanduser().absolute()
    if raw_datadir.is_symlink() or raw_datadir.parent.is_symlink():
        raise PolicyViolationError("symlink_runtime_state", "Refusing symlink runtime state directory.")
    datadir = _ensure_private_runtime_dir(raw_datadir, "datadir")

    # Reconcile stale metadata if present
    stale_reconciled = _reconcile_stale_metadata(datadir)
    current = inspect_owned_instance(datadir)
    if current["state"] == "running":
        meta = current["metadata"] or {}
        if allow_already_running and int(meta.get("port") or 0) == port and str(meta.get("host") or "") == host:
            return {
                "already_running": True,
                "stale_reconciled": stale_reconciled,
                "instance": meta,
            }
        raise PolicyViolationError(
            "owned_instance_running_on_different_port"
            if allow_already_running
            else "owned_instance_already_running",
            f"Owned runtime in '{datadir}' is already running on {meta.get('host')}:{meta.get('port')}.",
        )

    # Check if target port is already occupied by an unrelated process
    if _is_port_open(host, port):
        raise PolicyViolationError(
            "port_occupied_by_unrelated_process",
            f"Port {host}:{port} is already occupied by an unowned process. "
            "Refusing to kill, reconfigure, or implicitly attach to a foreign listener.",
        )

    # Do not need host-installed tools to report an occupied listener, but always
    # verify both pinned binaries before starting the owned Runtime process.
    verified_as, as_sha256 = verify_binary_before_execution(
        "action-server",
        runtime_binary,
        expected_sha256=expected_runtime_sha256,
    )
    verified_rcc, rcc_sha256 = verify_binary_before_execution("rcc", rcc_binary)
    as_health = inspect_binary_health("action-server", explicit_path=verified_as)

    # Check if server-info.json from a previous runtime version exists -> automatic coordinated SQLite backup
    server_info_file = datadir / "server-info.json"
    auto_backup = None
    if server_info_file.is_file() and (datadir / "server.db").is_file():
        try:
            prev_info = json.loads(server_info_file.read_text(encoding="utf-8"))
            prev_ver = str(prev_info.get("version") or prev_info.get("runtime_version") or "unknown")
            curr_ver = str(as_health.version or "unknown")
            if prev_ver != curr_ver:
                auto_backup = coordinated_sqlite_backup(
                    datadir,
                    label=f"auto-upgrade-{prev_ver}-to-{curr_ver}",
                )
        except Exception:
            pass

    roots = get_approved_roots(approved_roots)
    trusted_items = get_trusted_project_roots() if trusted_roots is None else trusted_roots
    trusted_project_roots = [Path(p).expanduser().resolve() for p in trusted_items]
    env = os.environ.copy()
    env["RCC_BOOTSTRAPPER_DATADIR"] = str(datadir)
    env["RCC_BOOTSTRAPPER_APPROVED_ROOTS"] = os.pathsep.join(str(r) for r in roots)
    env["RCC_BOOTSTRAPPER_TRUSTED_PROJECT_ROOTS"] = os.pathsep.join(str(r) for r in trusted_project_roots)
    env["RCC_BOOTSTRAPPER_ACTION_SERVER_BIN"] = str(verified_as)
    env["RCC_BOOTSTRAPPER_RCC_BIN"] = str(verified_rcc)
    requested_robocorp_home = Path(robocorp_home) if robocorp_home else default_robocorp_home()
    isolated_home = (
        Path(runtime_home).expanduser()
        if runtime_home
        else datadir.parent / "action-server-home"
    )
    isolated_home = _ensure_private_runtime_dir(isolated_home, "Action Server cache root")
    for key, path in (
        ("ACTIONS_HOME", isolated_home / "actions"),
        ("ROBOTS_HOME", isolated_home / "robots"),
        ("UV_CACHE_DIR", isolated_home / "uv-cache"),
        ("ROBOCORP_HOME", requested_robocorp_home),
    ):
        env[key] = str(_ensure_private_runtime_dir(path, key))
    env["PYTHONDONTWRITEBYTECODE"] = "1"

    resolved_pkg = package_dir.resolve()
    cmd = [
        str(verified_as),
        "start",
        "--address",
        host,
        "--port",
        str(port),
        "--dir",
        str(resolved_pkg),
        "--datadir",
        str(datadir),
        "--api-key",
        resolved_api_key,
    ]
    return {
        "already_running": False,
        "stale_reconciled": stale_reconciled,
        "datadir": datadir,
        "package_dir": resolved_pkg,
        "host": host,
        "port": port,
        "roots": roots,
        "trusted_project_roots": trusted_project_roots,
        "verified_as": verified_as,
        "as_sha256": as_sha256,
        "as_version": as_health.version,
        "verified_rcc": verified_rcc,
        "rcc_sha256": rcc_sha256,
        "resolved_api_key": resolved_api_key,
        "auto_backup": auto_backup,
        "env": env,
        "cmd": cmd,
    }


def _build_instance_metadata(launch: dict[str, Any], pid: int) -> dict[str, Any]:
    host = launch["host"]
    port = launch["port"]
    return {
        "instance_id": str(uuid.uuid4()),
        "pid": pid,
        "proc_start_token": _proc_start_token(pid),
        "host": host,
        "port": port,
        "mcp_url": f"http://{host}:{port}/mcp",
        "openapi_url": f"http://{host}:{port}/openapi.json",
        "datadir": str(launch["datadir"]),
        "package_dir": str(launch["package_dir"]),
        "approved_roots": [str(r) for r in launch["roots"]],
        "trusted_project_roots": [str(r) for r in launch["trusted_project_roots"]],
        "runtime_binary": str(launch["verified_as"]),
        "runtime_version": launch["as_version"],
        "runtime_sha256": launch["as_sha256"],
        "rcc_binary": str(launch["verified_rcc"]),
        "rcc_sha256": launch["rcc_sha256"],
        "auth_mode": "api-key" if launch["resolved_api_key"] else "loopback-no-auth",
        "started_at": _now_iso(),
        "auto_backup": launch["auto_backup"],
    }


def start_owned_runtime(
    *,
    datadir: Path,
    host: str = "127.0.0.1",
    port: int = 8765,
    package_dir: Path = BOOTSTRAPPER_PKG_DIR,
    approved_roots: list[str | Path] | None = None,
    trusted_roots: list[str | Path] | None = None,
    api_key: str | None = None,
    runtime_binary: str | Path | None = None,
    rcc_binary: str | Path | None = None,
    runtime_home: str | Path | None = None,
    robocorp_home: str | Path | None = None,
    expected_runtime_sha256: str | None = None,
    expected_package_sha256: str | None = None,
    startup_timeout: float = 90.0,
) -> dict[str, Any]:
    """Start an owned action-server instance after fail-closed binary verification and port ownership checks."""
    launch = _prepare_runtime_launch(
        datadir=datadir,
        host=host,
        port=port,
        package_dir=package_dir,
        approved_roots=approved_roots,
        trusted_roots=trusted_roots,
        api_key=api_key,
        runtime_binary=runtime_binary,
        rcc_binary=rcc_binary,
        runtime_home=runtime_home,
        robocorp_home=robocorp_home,
        expected_runtime_sha256=expected_runtime_sha256,
        expected_package_sha256=expected_package_sha256,
        allow_already_running=True,
    )
    if launch["already_running"]:
        return {
            "status": "already_running_owned",
            "stale_reconciled": launch["stale_reconciled"],
            "instance": launch["instance"],
        }

    datadir = launch["datadir"]
    resolved_api_key = launch["resolved_api_key"]
    stale_reconciled = launch["stale_reconciled"]

    stdout_log = datadir / "bootstrapper_runtime_stdout.log"
    stderr_log = datadir / "bootstrapper_runtime_stderr.log"
    out_handle = stdout_log.open("a", encoding="utf-8")
    err_handle = stderr_log.open("a", encoding="utf-8")

    popen_kwargs: dict[str, Any] = {
        "cwd": str(launch["package_dir"]),
        "env": launch["env"],
        "stdout": out_handle,
        "stderr": err_handle,
        "text": True,
    }
    if sys.platform == "win32":
        popen_kwargs["creationflags"] = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
    else:
        popen_kwargs["start_new_session"] = True

    proc = subprocess.Popen(launch["cmd"], **popen_kwargs)
    _SPAWNED_RUNTIME_PROCS[proc.pid] = proc
    out_handle.close()
    err_handle.close()

    instance_meta = _build_instance_metadata(launch, proc.pid)
    _save_instance_metadata(datadir, instance_meta)

    def startup_diagnostics() -> str:
        tails = []
        for log in (stdout_log, stderr_log, datadir / "server_log.txt"):
            if log.is_file():
                with log.open("rb") as stream:
                    stream.seek(max(0, log.stat().st_size - 3000))
                    tail = stream.read(3000).decode("utf-8", errors="replace")
                tails.append(f"{log.name}: {redact_secrets(tail, [resolved_api_key])}")
        return "\n".join(tails)

    deadline = time.monotonic() + startup_timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            err_tail = startup_diagnostics()
            _reconcile_stale_metadata(datadir)
            raise PolicyViolationError(
                "runtime_exited_during_startup",
                f"action-server exited with code {proc.returncode} during startup: {err_tail}",
            )
        if _is_port_open(host, port):
            return {
                "status": "started",
                "stale_reconciled": stale_reconciled,
                "instance": instance_meta,
            }
        time.sleep(0.25)

    # Timed out waiting for listener; terminate our spawned child
    diagnostics = startup_diagnostics()
    stop_owned_runtime(datadir=datadir)
    raise PolicyViolationError(
        "runtime_startup_timeout",
        f"Timed out after {startup_timeout}s waiting for action-server to listen on {host}:{port}.\n{diagnostics}",
    )


def serve_owned_runtime(
    *,
    datadir: Path,
    host: str = "127.0.0.1",
    port: int = 8765,
    package_dir: Path = BOOTSTRAPPER_PKG_DIR,
    approved_roots: list[str | Path] | None = None,
    trusted_roots: list[str | Path] | None = None,
    api_key: str | None = None,
    runtime_binary: str | Path | None = None,
    rcc_binary: str | Path | None = None,
    runtime_home: str | Path | None = None,
    robocorp_home: str | Path | None = None,
    expected_runtime_sha256: str | None = None,
) -> int:
    """Run an owned action-server instance in the foreground for systemd --user or interactive supervision."""
    launch = _prepare_runtime_launch(
        datadir=datadir,
        host=host,
        port=port,
        package_dir=package_dir,
        approved_roots=approved_roots,
        trusted_roots=trusted_roots,
        api_key=api_key,
        runtime_binary=runtime_binary,
        rcc_binary=rcc_binary,
        runtime_home=runtime_home,
        robocorp_home=robocorp_home,
        expected_runtime_sha256=expected_runtime_sha256,
        allow_already_running=False,
    )
    datadir = launch["datadir"]

    popen_kwargs: dict[str, Any] = {
        "cwd": str(launch["package_dir"]),
        "env": launch["env"],
    }
    if sys.platform == "win32":
        popen_kwargs["creationflags"] = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
    else:
        popen_kwargs["start_new_session"] = True

    proc = subprocess.Popen(launch["cmd"], **popen_kwargs)
    _SPAWNED_RUNTIME_PROCS[proc.pid] = proc
    instance_meta = _build_instance_metadata(launch, proc.pid)
    _save_instance_metadata(datadir, instance_meta)

    stopping = False
    previous_handlers: dict[int, Any] = {}

    def _terminate_child() -> None:
        if proc.poll() is not None:
            return
        if sys.platform == "win32":
            subprocess.run(
                ["taskkill", "/PID", str(proc.pid), "/T", "/F"],
                capture_output=True,
                text=True,
                timeout=15.0,
                check=False,
            )
            return
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
        except ProcessLookupError:
            return
        except Exception:
            try:
                proc.terminate()
            except Exception:
                pass

    def _handle_stop(signum: int, _frame: Any) -> None:
        nonlocal stopping
        stopping = True
        _terminate_child()

    for sig_name in ("SIGTERM", "SIGINT"):
        sig = getattr(signal, sig_name, None)
        if sig is not None:
            try:
                previous_handlers[int(sig)] = signal.signal(sig, _handle_stop)
            except (ValueError, OSError):
                pass

    try:
        while True:
            try:
                code = proc.wait(timeout=0.5)
                return 0 if stopping else int(code)
            except subprocess.TimeoutExpired:
                if stopping:
                    try:
                        proc.wait(timeout=15.0)
                    except subprocess.TimeoutExpired:
                        if sys.platform != "win32":
                            try:
                                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
                            except Exception:
                                pass
                        else:
                            _terminate_child()
                        proc.wait(timeout=5.0)
                    return 0
    finally:
        for sig_num, handler in previous_handlers.items():
            try:
                signal.signal(sig_num, handler)
            except (ValueError, OSError):
                pass
        _SPAWNED_RUNTIME_PROCS.pop(proc.pid, None)
        if proc.poll() is None:
            _terminate_child()
        meta_path = _instance_metadata_path(datadir)
        current_meta = _load_instance_metadata(datadir)
        if current_meta and current_meta.get("instance_id") == instance_meta["instance_id"]:
            meta_path.unlink(missing_ok=True)


def _default_systemd_unit_dir() -> Path:
    xdg_config = os.environ.get("XDG_CONFIG_HOME")
    base = Path(xdg_config).expanduser() if xdg_config else (Path.home() / ".config")
    return (base / "systemd" / "user").absolute()


def _default_systemd_env_file(service_name: str) -> Path:
    xdg_config = os.environ.get("XDG_CONFIG_HOME")
    base = Path(xdg_config).expanduser() if xdg_config else (Path.home() / ".config")
    return (base / "rcc-bootstrapper" / f"{service_name}.env").absolute()


def _validate_no_control_chars(value: str, label: str) -> str:
    if any(ord(ch) < 32 or ord(ch) == 127 for ch in value):
        raise PolicyViolationError(
            "invalid_service_value",
            f"Refusing control character in {label}.",
        )
    return value


def _systemd_quote_arg(value: str) -> str:
    _validate_no_control_chars(value, "systemd ExecStart argument")
    escaped = value.replace("\\", "\\\\").replace('"', '\\"').replace("%", "%%")
    return f'"{escaped}"'


def _systemd_quote_unit_path(value: str) -> str:
    _validate_no_control_chars(value, "systemd unit path")
    escaped = value.replace("\\", "\\\\").replace('"', '\\"').replace("%", "%%")
    if any(ch.isspace() for ch in value) or any(ch in value for ch in ('"', "\\")):
        return f'"{escaped}"'
    return escaped


def _read_env_file_key(env_file: Path, key: str) -> str | None:
    if not env_file.is_file():
        return None
    for raw_line in env_file.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        found_key, found_val = line.split("=", 1)
        if found_key.strip() == key:
            val = found_val.strip()
            if len(val) >= 2 and val[0] == val[-1] and val[0] in ("'", '"'):
                val = val[1:-1]
            return val or None
    return None


def _write_private_env_file(env_file: Path, values: dict[str, str]) -> None:
    env_file = env_file.expanduser().absolute()
    if env_file.is_symlink() or env_file.parent.is_symlink():
        raise PolicyViolationError("symlink_service_path", f"Refusing symlink env file path: {env_file}")
    if env_file.exists() and not env_file.is_file():
        raise PolicyViolationError("invalid_service_path", f"Env file path is not a regular file: {env_file}")
    _ensure_private_runtime_dir(env_file.parent, "systemd environment directory")

    lines = [
        "# Managed by plugins/rcc/scripts/bootstrapper_runtime.py (mode 0600)",
    ]
    for key, val in values.items():
        _validate_no_control_chars(key, "env key")
        _validate_no_control_chars(val, f"env value for {key}")
        if "'" in val:
            escaped = val.replace("\\", "\\\\").replace('"', '\\"').replace("$", "\\$")
            lines.append(f'{key}="{escaped}"')
        elif any(ch.isspace() or ch in '#$"' for ch in val):
            lines.append(f"{key}='{val}'")
        else:
            lines.append(f"{key}={val}")
    payload = ("\n".join(lines) + "\n").encode("utf-8")

    tmp_path = env_file.with_name(f".{env_file.name}.tmp-{uuid.uuid4().hex[:8]}")
    fd = os.open(tmp_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(tmp_path, 0o600)
        os.replace(tmp_path, env_file)
    except Exception:
        tmp_path.unlink(missing_ok=True)
        raise


def render_systemd_unit(
    *,
    service_name: str = "rcc-bootstrapper",
    datadir: Path,
    env_file: Path,
    approved_roots: list[str | Path],
    trusted_roots: list[str | Path] | None = None,
    host: str = "127.0.0.1",
    port: int = 8765,
    python_executable: str | Path | None = None,
    runtime_binary: str | Path | None = None,
    rcc_binary: str | Path | None = None,
) -> tuple[str, list[str]]:
    """Render a Linux systemd --user unit running `bootstrapper_runtime.py serve`."""
    safe_name = validate_safe_identifier(service_name, field_name="service_name")
    if not approved_roots:
        raise PolicyViolationError(
            "approved_root_required",
            "Pass at least one --approved-root when configuring a persistent systemd user service.",
        )
    if not host or any(ch.isspace() for ch in host):
        raise PolicyViolationError("invalid_host", "Service host must be a non-empty hostname or IP address.")
    _validate_no_control_chars(host, "service host")
    if not (1 <= int(port) <= 65535):
        raise PolicyViolationError("invalid_port", f"Service port must be between 1 and 65535, got {port}.")

    py_bin = str(Path(python_executable).expanduser().absolute() if python_executable else Path(sys.executable).absolute())
    script_path = str(Path(__file__).resolve())
    resolved_datadir = str(datadir.expanduser().absolute())
    resolved_env_file = str(env_file.expanduser().absolute())
    resolved_approved = [str(Path(p).expanduser().resolve()) for p in approved_roots]
    resolved_trusted = [str(Path(p).expanduser().resolve()) for p in (trusted_roots or [])]

    argv = [
        py_bin,
        script_path,
        "serve",
        "--datadir",
        resolved_datadir,
        "--host",
        host,
        "--port",
        str(int(port)),
    ]
    for root in resolved_approved:
        argv.extend(["--approved-root", root])
    for root in resolved_trusted:
        argv.extend(["--trusted-root", root])
    if runtime_binary:
        argv.extend(["--runtime-binary", str(Path(runtime_binary).expanduser().resolve())])
    if rcc_binary:
        argv.extend(["--rcc-binary", str(Path(rcc_binary).expanduser().resolve())])

    exec_start = " ".join(_systemd_quote_arg(arg) for arg in argv)
    env_line = f"EnvironmentFile={_systemd_quote_unit_path(resolved_env_file)}"
    unit_text = (
        "[Unit]\n"
        f"Description=RCC Actions & Robot Bootstrapper MCP Runtime ({safe_name})\n"
        "After=network-online.target\n"
        "\n"
        "[Service]\n"
        "Type=simple\n"
        f"{env_line}\n"
        f"ExecStart={exec_start}\n"
        "Restart=on-failure\n"
        "RestartSec=3\n"
        "TimeoutStopSec=20\n"
        "NoNewPrivileges=yes\n"
        "\n"
        "[Install]\n"
        "WantedBy=default.target\n"
    )
    return unit_text, argv


def install_systemd_service(
    *,
    service_name: str = "rcc-bootstrapper",
    datadir: Path | None = None,
    unit_dir: Path | None = None,
    env_file: Path | None = None,
    approved_roots: list[str | Path] | None = None,
    trusted_roots: list[str | Path] | None = None,
    host: str = "127.0.0.1",
    port: int = 8765,
    api_key: str | None = None,
    generate_api_key: bool = False,
    rotate_api_key: bool = False,
    python_executable: str | Path | None = None,
    runtime_binary: str | Path | None = None,
    rcc_binary: str | Path | None = None,
    print_only: bool = False,
    enable: bool = False,
) -> dict[str, Any]:
    """Install an opt-in Linux systemd --user service and mode-0600 env file for the bootstrapper runtime."""
    if sys.platform != "linux" and not print_only:
        raise PolicyViolationError(
            "systemd_linux_only",
            "systemd --user service installation is supported on Linux only.",
        )

    safe_name = validate_safe_identifier(service_name, field_name="service_name")
    unit_name = f"{safe_name}.service"
    resolved_datadir = (datadir.expanduser().absolute() if datadir else default_datadir())
    resolved_unit_dir = (unit_dir.expanduser().absolute() if unit_dir else _default_systemd_unit_dir())
    resolved_env_file = (env_file.expanduser().absolute() if env_file else _default_systemd_env_file(safe_name))
    unit_path = resolved_unit_dir / unit_name

    unit_text, argv = render_systemd_unit(
        service_name=safe_name,
        datadir=resolved_datadir,
        env_file=resolved_env_file,
        approved_roots=list(approved_roots or []),
        trusted_roots=list(trusted_roots or []),
        host=host,
        port=port,
        python_executable=python_executable,
        runtime_binary=runtime_binary,
        rcc_binary=rcc_binary,
    )

    mcp_url = f"http://{host}:{int(port)}/mcp"
    codex_add_cmd = (
        f"codex mcp add {safe_name} --url {mcp_url} --bearer-token-env-var RCC_BOOTSTRAPPER_API_KEY"
    )
    systemctl_cmds = [
        "systemctl --user daemon-reload",
        f"systemctl --user enable --now {unit_name}",
    ]

    if print_only:
        return {
            "status": "rendered",
            "service_name": safe_name,
            "unit_name": unit_name,
            "unit_path": str(unit_path),
            "env_file": str(resolved_env_file),
            "mcp_url": mcp_url,
            "exec_argv": argv,
            "unit_content": unit_text,
            "systemctl_commands": systemctl_cmds,
            "codex_mcp_add_command": codex_add_cmd,
        }

    if resolved_unit_dir.is_symlink() or unit_path.is_symlink():
        raise PolicyViolationError("symlink_service_path", f"Refusing symlink systemd unit path: {unit_path}")
    if resolved_env_file.is_symlink() or resolved_env_file.parent.is_symlink():
        raise PolicyViolationError("symlink_service_path", f"Refusing symlink env file path: {resolved_env_file}")

    existing_key = _read_env_file_key(resolved_env_file, "RCC_BOOTSTRAPPER_API_KEY")
    env_provided_key = os.environ.get("RCC_BOOTSTRAPPER_API_KEY")
    api_key_source: str
    chosen_key: str | None = None

    if api_key is not None and api_key.strip():
        chosen_key = api_key.strip()
        api_key_source = "provided"
    elif rotate_api_key:
        chosen_key = secrets.token_urlsafe(32)
        api_key_source = "rotated"
    elif existing_key:
        chosen_key = existing_key
        api_key_source = "existing_env_file"
    elif env_provided_key and env_provided_key.strip() and not generate_api_key:
        chosen_key = env_provided_key.strip()
        api_key_source = "environment"
    elif generate_api_key:
        chosen_key = secrets.token_urlsafe(32)
        api_key_source = "generated"
    else:
        raise PolicyViolationError(
            "runtime_api_key_required",
            "Provide --api-key, export RCC_BOOTSTRAPPER_API_KEY, pass --generate-api-key, or keep an existing env file.",
        )

    env_values: dict[str, str] = {"RCC_BOOTSTRAPPER_API_KEY": chosen_key}
    if os.environ.get("RCC_BOOTSTRAPPER_HOME"):
        env_values["RCC_BOOTSTRAPPER_HOME"] = str(Path(os.environ["RCC_BOOTSTRAPPER_HOME"]).expanduser().absolute())
    if os.environ.get("RCC_BOOTSTRAPPER_PACKAGE_DIR"):
        env_values["RCC_BOOTSTRAPPER_PACKAGE_DIR"] = str(
            Path(os.environ["RCC_BOOTSTRAPPER_PACKAGE_DIR"]).expanduser().resolve()
        )
    _write_private_env_file(resolved_env_file, env_values)

    resolved_unit_dir.mkdir(parents=True, exist_ok=True)
    tmp_unit = unit_path.with_name(f".{unit_path.name}.tmp-{uuid.uuid4().hex[:8]}")
    try:
        tmp_unit.write_text(unit_text, encoding="utf-8")
        os.chmod(tmp_unit, 0o644)
        os.replace(tmp_unit, unit_path)
    except Exception:
        tmp_unit.unlink(missing_ok=True)
        raise

    enabled_now = False
    if enable:
        for cmd in (
            ["systemctl", "--user", "daemon-reload"],
            ["systemctl", "--user", "enable", "--now", unit_name],
        ):
            proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
            if proc.returncode != 0:
                detail = (proc.stderr or proc.stdout or "").strip()
                raise PolicyViolationError(
                    "systemctl_failed",
                    f"Command '{' '.join(cmd)}' failed (exit {proc.returncode}): {detail}",
                )
        enabled_now = True

    return {
        "status": "installed_and_enabled" if enabled_now else "installed",
        "service_name": safe_name,
        "unit_name": unit_name,
        "unit_path": str(unit_path),
        "env_file": str(resolved_env_file),
        "env_file_mode": oct(stat.S_IMODE(resolved_env_file.stat().st_mode)),
        "api_key_source": api_key_source,
        "mcp_url": mcp_url,
        "enabled": enabled_now,
        "systemctl_commands": systemctl_cmds,
        "codex_mcp_add_command": codex_add_cmd,
        "shell_export_hint": f"set -a && source {shlex.quote(str(resolved_env_file))} && set +a",
    }


def uninstall_systemd_service(
    *,
    service_name: str = "rcc-bootstrapper",
    unit_dir: Path | None = None,
    env_file: Path | None = None,
    remove_env_file: bool = False,
    run_systemctl: bool = True,
) -> dict[str, Any]:
    """Remove an opt-in Linux systemd --user unit without touching datadir or SQLite state."""
    if sys.platform != "linux":
        raise PolicyViolationError(
            "systemd_linux_only",
            "systemd --user service removal is supported on Linux only.",
        )

    safe_name = validate_safe_identifier(service_name, field_name="service_name")
    unit_name = f"{safe_name}.service"
    resolved_unit_dir = (unit_dir.expanduser().absolute() if unit_dir else _default_systemd_unit_dir())
    resolved_env_file = (env_file.expanduser().absolute() if env_file else _default_systemd_env_file(safe_name))
    unit_path = resolved_unit_dir / unit_name

    if unit_path.is_symlink():
        raise PolicyViolationError("symlink_service_path", f"Refusing symlink systemd unit path: {unit_path}")
    if remove_env_file and resolved_env_file.is_symlink():
        raise PolicyViolationError("symlink_service_path", f"Refusing symlink env file path: {resolved_env_file}")

    systemctl_ran = False
    if run_systemctl and shutil.which("systemctl"):
        subprocess.run(
            ["systemctl", "--user", "disable", "--now", unit_name],
            capture_output=True,
            text=True,
            check=False,
        )
        systemctl_ran = True

    unit_removed = False
    if unit_path.is_file():
        unit_path.unlink()
        unit_removed = True

    env_removed = False
    if remove_env_file and resolved_env_file.is_file():
        resolved_env_file.unlink()
        env_removed = True

    if systemctl_ran:
        subprocess.run(
            ["systemctl", "--user", "daemon-reload"],
            capture_output=True,
            text=True,
            check=False,
        )

    return {
        "status": "uninstalled",
        "service_name": safe_name,
        "unit_name": unit_name,
        "unit_path": str(unit_path),
        "unit_removed": unit_removed,
        "env_file": str(resolved_env_file),
        "env_file_removed": env_removed,
        "systemctl_invoked": systemctl_ran,
    }


def stop_owned_runtime(*, datadir: Path, timeout: float = 15.0) -> dict[str, Any]:
    """Stop the owned action-server instance recorded in datadir without touching unowned processes."""
    datadir = datadir.expanduser().resolve()
    state_info = inspect_owned_instance(datadir)
    state = state_info["state"]
    meta = state_info["metadata"]

    if state == "stopped":
        return {"status": "already_stopped", "datadir": str(datadir)}
    if state == "stale":
        _reconcile_stale_metadata(datadir)
        return {"status": "reconciled_stale", "datadir": str(datadir), "stale_reason": state_info.get("stale_reason")}

    pid = int((meta or {}).get("pid") or 0)
    try:
        if sys.platform == "win32":
            # PyInstaller's parent owns a child Runtime and dependency processes.
            # Stop the whole verified owned tree, not just its launcher.
            killed = subprocess.run(
                ["taskkill", "/PID", str(pid), "/T", "/F"],
                capture_output=True, text=True, timeout=timeout, check=False,
            )
            if killed.returncode and _is_pid_alive(pid):
                raise PolicyViolationError("runtime_stop_failed", "Could not terminate the owned Windows process tree.")
        else:
            os.killpg(os.getpgid(pid), signal.SIGTERM)
    except ProcessLookupError:
        pass
    except Exception:
        if sys.platform == "win32":
            raise
        try:
            os.kill(pid, signal.SIGTERM)
        except Exception:
            pass

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not _is_pid_alive(pid):
            break
        time.sleep(0.1)

    if _is_pid_alive(pid):
        if sys.platform == "win32":
            raise PolicyViolationError("runtime_stop_failed", "Owned Windows process tree is still running; ownership metadata retained.")
        try:
            os.killpg(os.getpgid(pid), signal.SIGKILL)
        except Exception:
            pass

    if _is_pid_alive(pid):
        raise PolicyViolationError("runtime_stop_failed", "Owned Runtime is still running; ownership metadata retained.")

    meta_path = _instance_metadata_path(datadir)
    if meta_path.is_file():
        meta_path.unlink(missing_ok=True)

    return {
        "status": "stopped",
        "pid": pid,
        "datadir": str(datadir),
        "terminated": not _is_pid_alive(pid),
    }


def run_mcp_smoke(
    *,
    mcp_url: str,
    api_key: str | None = None,
    verify_unauthenticated_rejected: bool = False,
) -> dict[str, Any]:
    """Drive MCP initialize -> notifications/initialized -> tools/list -> resources/list -> prompts/list -> bootstrapper_discover."""
    unauth_probe: dict[str, Any] | None = None
    if verify_unauthenticated_rejected and api_key:
        unauth_client = McpHttpClient(mcp_url, api_key=None, timeout=10.0)
        try:
            unauth_client.request(
                "initialize",
                {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "rcc-bootstrapper-unauth-probe", "version": "0.6.0"},
                },
            )
            raise RuntimeError("Expected unauthenticated MCP request to be rejected with HTTP 401/403, but it succeeded.")
        except RuntimeError as exc:
            unauth_probe = {"rejected_as_expected": True, "error": str(exc)}

    client = McpHttpClient(mcp_url, api_key=api_key, timeout=60.0)
    init_resp = client.request(
        "initialize",
        {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "rcc-bootstrapper-smoke", "version": "0.6.0"},
        },
    )
    notif_resp = client.request("notifications/initialized", {}, is_notification=True)
    tools_resp = client.request("tools/list", {})
    resources_resp = client.request("resources/list", {})
    resource_templates_resp = client.request("resources/templates/list", {})
    prompts_resp = client.request("prompts/list", {})
    discover_call = client.request(
        "tools/call",
        {
            "name": "bootstrapper_discover",
            "arguments": {},
        },
    )

    tools_list = (tools_resp.get("result") or {}).get("tools") or []
    tool_names = sorted(str(t.get("name")) for t in tools_list if isinstance(t, dict))

    resources_list = (resources_resp.get("result") or {}).get("resources") or []
    resource_uris = sorted(str(r.get("uri")) for r in resources_list if isinstance(r, dict))

    resource_templates_list = (resource_templates_resp.get("result") or {}).get("resourceTemplates") or []
    resource_template_uris = sorted(str(rt.get("uriTemplate")) for rt in resource_templates_list if isinstance(rt, dict))

    prompts_list = (prompts_resp.get("result") or {}).get("prompts") or []
    prompt_names = sorted(str(p.get("name")) for p in prompts_list if isinstance(p, dict))

    expected_bootstrapper_tools = {
        "bootstrapper_discover",
        "bootstrapper_plan",
        "bootstrapper_scaffold",
        "bootstrapper_validate",
        "bootstrapper_run",
        "bootstrapper_status",
        "bootstrapper_cancel",
        "bootstrapper_results",
    }
    missing = sorted(expected_bootstrapper_tools - set(tool_names))
    if missing:
        raise RuntimeError(f"Runtime /mcp did not expose expected bootstrapper tools: {missing}")

    call_result = discover_call.get("result") or {}
    if call_result.get("isError"):
        raise RuntimeError(f"bootstrapper_discover returned isError=True: {discover_call}")

    return {
        "mcp_url": mcp_url,
        "authenticated": bool(api_key),
        "unauthenticated_rejection_probe": unauth_probe,
        "server_info": (init_resp.get("result") or {}).get("serverInfo"),
        "protocol_version": (init_resp.get("result") or {}).get("protocolVersion"),
        "initialized_notification": notif_resp,
        "discovered_tool_names": tool_names,
        "discovered_resource_uris": resource_uris,
        "discovered_resource_template_uris": resource_template_uris,
        "discovered_prompt_names": prompt_names,
        "bootstrapper_discover_invocation": call_result,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="RCC Actions Bootstrapper Runtime Lifecycle & MCP Verifier")
    sub = parser.add_subparsers(dest="command", required=True)

    doc_p = sub.add_parser("doctor", help="Verify local action-server and rcc binaries and fail-closed SHA-256 digests.")
    doc_p.add_argument("--runtime-binary", default=None)
    doc_p.add_argument("--rcc-binary", default=None)

    start_p = sub.add_parser("start", help="Start an owned action-server instance hosting plugins/rcc/bootstrapper.")
    start_p.add_argument("--datadir", default=None)
    start_p.add_argument("--host", default="127.0.0.1")
    start_p.add_argument("--port", type=int, default=8765)
    start_p.add_argument("--approved-root", action="append", dest="approved_roots", default=None)
    start_p.add_argument("--trusted-root", action="append", dest="trusted_roots", default=None)
    start_p.add_argument("--api-key", default=None)
    start_p.add_argument("--runtime-binary", default=None)
    start_p.add_argument("--rcc-binary", default=None)
    start_p.add_argument("--expected-runtime-sha256", default=None)

    serve_p = sub.add_parser(
        "serve",
        help="Run the owned action-server instance in the foreground (for systemd --user or interactive use).",
    )
    serve_p.add_argument("--datadir", default=None)
    serve_p.add_argument("--host", default="127.0.0.1")
    serve_p.add_argument("--port", type=int, default=8765)
    serve_p.add_argument("--approved-root", action="append", dest="approved_roots", default=None)
    serve_p.add_argument("--trusted-root", action="append", dest="trusted_roots", default=None)
    serve_p.add_argument("--api-key", default=None)
    serve_p.add_argument("--runtime-binary", default=None)
    serve_p.add_argument("--rcc-binary", default=None)
    serve_p.add_argument("--expected-runtime-sha256", default=None)

    svc_install_p = sub.add_parser(
        "systemd-install",
        help="Install an optional Linux systemd --user service and mode-0600 env file for the bootstrapper runtime.",
    )
    svc_install_p.add_argument("--service-name", default="rcc-bootstrapper")
    svc_install_p.add_argument("--datadir", default=None)
    svc_install_p.add_argument("--unit-dir", default=None)
    svc_install_p.add_argument("--env-file", default=None)
    svc_install_p.add_argument("--host", default="127.0.0.1")
    svc_install_p.add_argument("--port", type=int, default=8765)
    svc_install_p.add_argument("--approved-root", action="append", dest="approved_roots", default=None)
    svc_install_p.add_argument("--trusted-root", action="append", dest="trusted_roots", default=None)
    svc_install_p.add_argument("--api-key", default=None)
    svc_install_p.add_argument("--generate-api-key", action="store_true")
    svc_install_p.add_argument("--rotate-api-key", action="store_true")
    svc_install_p.add_argument("--python", dest="python_executable", default=None)
    svc_install_p.add_argument("--runtime-binary", default=None)
    svc_install_p.add_argument("--rcc-binary", default=None)
    svc_install_p.add_argument("--print-only", action="store_true")
    svc_install_p.add_argument("--enable", action="store_true")

    svc_uninstall_p = sub.add_parser(
        "systemd-uninstall",
        help="Disable and remove an opt-in Linux systemd --user service without touching datadir state.",
    )
    svc_uninstall_p.add_argument("--service-name", default="rcc-bootstrapper")
    svc_uninstall_p.add_argument("--unit-dir", default=None)
    svc_uninstall_p.add_argument("--env-file", default=None)
    svc_uninstall_p.add_argument("--remove-env-file", action="store_true")
    svc_uninstall_p.add_argument("--no-systemctl", action="store_true")

    stop_p = sub.add_parser("stop", help="Stop the owned action-server instance for datadir.")
    stop_p.add_argument("--datadir", default=None)

    status_p = sub.add_parser("status", help="Inspect the status of the owned action-server instance.")
    status_p.add_argument("--datadir", default=None)

    backup_p = sub.add_parser("backup", help="Create a coordinated SQLite backup of server.db and runs.db.")
    backup_p.add_argument("--datadir", default=None)
    backup_p.add_argument("--label", default="manual")

    restore_p = sub.add_parser("restore", help="Restore a SQLite backup snapshot into datadir.")
    restore_p.add_argument("--datadir", default=None)
    restore_p.add_argument("--backup-dir", required=True)

    smoke_p = sub.add_parser("mcp-smoke", help="Run MCP initialize, discovery, and tool call against /mcp.")
    smoke_p.add_argument("--mcp-url", default="http://127.0.0.1:8765/mcp")
    smoke_p.add_argument("--api-key", default=None)
    smoke_p.add_argument("--verify-unauthenticated-rejected", action="store_true")

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    try:
        if args.command == "doctor":
            as_health = inspect_binary_health("action-server", explicit_path=args.runtime_binary, pinned_version="1.1.1")
            rcc_health = inspect_binary_health("rcc", explicit_path=args.rcc_binary, pinned_version="18.19.11")
            verify_binary_before_execution("action-server", args.runtime_binary)
            verify_binary_before_execution("rcc", args.rcc_binary)
            out = {
                "status": "ok",
                "fail_closed_sha256_verified": True,
                "action_server": as_health.model_dump(),
                "rcc": rcc_health.model_dump(),
                "pinned_allowlist": {
                    "runtime": sorted(KNOWN_VALID_RUNTIME_SHA256),
                    "rcc": sorted(KNOWN_VALID_RCC_SHA256),
                    "artifacts": PINNED_SHA256_BY_ARTIFACT,
                },
            }
            print(json.dumps(out, indent=2))
            return 0

        if args.command == "start":
            ddir = Path(args.datadir) if args.datadir else default_datadir()
            res = start_owned_runtime(
                datadir=ddir,
                host=args.host,
                port=args.port,
                approved_roots=args.approved_roots,
                trusted_roots=args.trusted_roots,
                api_key=args.api_key,
                runtime_binary=args.runtime_binary,
                rcc_binary=args.rcc_binary,
                expected_runtime_sha256=args.expected_runtime_sha256,
            )
            print(json.dumps(res, indent=2))
            return 0

        if args.command == "serve":
            ddir = Path(args.datadir) if args.datadir else default_datadir()
            return serve_owned_runtime(
                datadir=ddir,
                host=args.host,
                port=args.port,
                approved_roots=args.approved_roots,
                trusted_roots=args.trusted_roots,
                api_key=args.api_key,
                runtime_binary=args.runtime_binary,
                rcc_binary=args.rcc_binary,
                expected_runtime_sha256=args.expected_runtime_sha256,
            )

        if args.command == "systemd-install":
            res = install_systemd_service(
                service_name=args.service_name,
                datadir=Path(args.datadir) if args.datadir else None,
                unit_dir=Path(args.unit_dir) if args.unit_dir else None,
                env_file=Path(args.env_file) if args.env_file else None,
                approved_roots=args.approved_roots,
                trusted_roots=args.trusted_roots,
                host=args.host,
                port=args.port,
                api_key=args.api_key,
                generate_api_key=args.generate_api_key,
                rotate_api_key=args.rotate_api_key,
                python_executable=args.python_executable,
                runtime_binary=args.runtime_binary,
                rcc_binary=args.rcc_binary,
                print_only=args.print_only,
                enable=args.enable,
            )
            if args.print_only:
                print(res["unit_content"], end="")
            else:
                print(json.dumps(res, indent=2))
            return 0

        if args.command == "systemd-uninstall":
            res = uninstall_systemd_service(
                service_name=args.service_name,
                unit_dir=Path(args.unit_dir) if args.unit_dir else None,
                env_file=Path(args.env_file) if args.env_file else None,
                remove_env_file=args.remove_env_file,
                run_systemctl=not args.no_systemctl,
            )
            print(json.dumps(res, indent=2))
            return 0

        if args.command == "stop":
            ddir = Path(args.datadir) if args.datadir else default_datadir()
            res = stop_owned_runtime(datadir=ddir)
            print(json.dumps(res, indent=2))
            return 0

        if args.command == "status":
            ddir = Path(args.datadir).expanduser().resolve() if args.datadir else default_datadir()
            res = inspect_owned_instance(ddir)
            print(json.dumps(res, indent=2))
            return 0

        if args.command == "backup":
            ddir = Path(args.datadir).expanduser().resolve() if args.datadir else default_datadir()
            res = coordinated_sqlite_backup(ddir, label=args.label)
            print(json.dumps(res, indent=2))
            return 0

        if args.command == "restore":
            ddir = Path(args.datadir).expanduser().resolve() if args.datadir else default_datadir()
            res = restore_sqlite_backup(ddir, Path(args.backup_dir).expanduser().resolve())
            print(json.dumps(res, indent=2))
            return 0

        if args.command == "mcp-smoke":
            key = args.api_key if args.api_key is not None else os.environ.get("RCC_BOOTSTRAPPER_API_KEY")
            res = run_mcp_smoke(
                mcp_url=args.mcp_url,
                api_key=key,
                verify_unauthenticated_rejected=args.verify_unauthenticated_rejected,
            )
            print(json.dumps(res, indent=2))
            return 0

    except PolicyViolationError as exc:
        print(json.dumps({"status": "error", "code": exc.code, "message": exc.message}, indent=2), file=sys.stderr)
        return 2
    except Exception as exc:
        print(json.dumps({"status": "error", "code": "runtime_error", "message": str(exc)}, indent=2), file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
