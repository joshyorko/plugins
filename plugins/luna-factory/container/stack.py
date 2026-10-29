#!/usr/bin/env python3
"""Start, inspect, or stop one explicitly named Luna Factory Compose project."""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys


HERE = Path(__file__).resolve().parent


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True, help="Existing private deployment directory")
    parser.add_argument("--project-name", required=True, help="The same unique project name for every action")
    parser.add_argument("--github-egress", action="store_true", help="Include the GitHub-only egress override")
    parser.add_argument("--file", type=Path, action="append", default=[], help="Additional Compose override; repeat on every action")
    parser.add_argument("action", choices=("up", "down", "status"))
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,62}", args.project_name):
        parser.error("project name must contain only lowercase letters, digits, underscores or hyphens")
    env_file = args.root.expanduser().absolute() / ".env"
    if not env_file.is_file():
        parser.error("deployment .env is missing; run init_private.py first")
    command = ["podman", "compose", "--project-name", args.project_name,
               "--env-file", str(env_file), "--file", str(HERE / "compose.yaml")]
    if args.github_egress:
        command.extend(["--file", str(HERE / "compose.github-egress.yaml")])
    for override in args.file:
        override = override.expanduser().absolute()
        if not override.is_file():
            parser.error("Compose override file is missing")
        command.extend(["--file", str(override)])
    try:
        if args.action == "up":
            subprocess.run([*command, "config", "--quiet"], check=True)
            subprocess.run([*command, "up", "--detach", "--wait", "--wait-timeout", "60"], check=True)
        elif args.action == "down":
            subprocess.run([*command, "down"], check=True)
        else:
            subprocess.run([*command, "ps", "--all"], check=True)
    except subprocess.CalledProcessError as error:
        return error.returncode if error.returncode > 0 else 1
    except OSError as error:
        print(f"Compose command unavailable: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
