#!/usr/bin/env python3
"""Create a new private config/state layout for rootless Podman."""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import sys
from pathlib import Path
from typing import Any


HERE = Path(__file__).resolve().parent
DEFAULT_ROOT = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "luna-factory-oci/v021"


def require_free_loopback_port(port: int) -> None:
    if not 1024 <= port <= 65535:
        raise ValueError("host port must be between 1024 and 65535")
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        try:
            listener.bind(("127.0.0.1", port))
        except OSError as error:
            raise ValueError(f"127.0.0.1:{port} is not free") from error


def _env_value(value: str) -> str:
    if "\n" in value or "\r" in value:
        raise ValueError("deployment values must not contain newlines")
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("$", "$$") + '"'


def prepare_private(root: Path, port: int, image: str) -> dict[str, str]:
    require_free_loopback_port(port)
    tag = re.fullmatch(r"localhost/luna-factory:[A-Za-z0-9_.-]+", image)
    digest = re.fullmatch(r"localhost/luna-factory@sha256:[a-f0-9]{64}", image)
    if not (tag or digest):
        raise ValueError("image must be a local Luna Factory tag or sha256 digest reference")
    root = root.expanduser().absolute()
    root.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        root.mkdir(mode=0o700)
    except FileExistsError as error:
        raise FileExistsError("private deployment path already exists; choose a new path") from error

    config_dir = root / "config"
    state_dir = root / "state"
    config_dir.mkdir(mode=0o700)
    state_dir.mkdir(mode=0o700)
    operator: dict[str, Any] = json.loads((HERE / "operator.example.json").read_text(encoding="utf-8"))
    origin = f"http://127.0.0.1:{port}"
    operator["published_origin"] = origin
    config_path = config_dir / "operator.json"
    with config_path.open("x", encoding="utf-8") as destination:
        destination.write(json.dumps(operator, indent=2) + "\n")
    config_path.chmod(0o600)

    values = {
        "LUNA_FACTORY_IMAGE": image,
        "LUNA_HOST_PORT": str(port),
        "LUNA_PUBLISHED_ORIGIN": origin,
        "LUNA_CONFIG_DIR": str(config_dir),
        "LUNA_STATE_DIR": str(state_dir),
    }
    env_path = root / ".env"
    with env_path.open("x", encoding="utf-8") as destination:
        destination.write("".join(f"{key}={_env_value(value)}\n" for key, value in values.items()))
    env_path.chmod(0o600)
    return {"root": str(root), "config": str(config_path), "state": str(state_dir), "env": str(env_path), "origin": origin}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--host-port", type=int, default=18788)
    parser.add_argument("--image", required=True)
    args = parser.parse_args()
    try:
        result = prepare_private(args.root, args.host_port, args.image)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"private deployment setup rejected: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
