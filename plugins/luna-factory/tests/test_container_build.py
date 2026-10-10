import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
import socket
import shutil
import sys
from pathlib import Path


CONTAINER_DIR = Path(__file__).parents[1] / "container"
SPEC = importlib.util.spec_from_file_location("luna_factory_container_build", CONTAINER_DIR / "build_image.py")
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)
INIT_SPEC = importlib.util.spec_from_file_location("luna_factory_container_init", CONTAINER_DIR / "init_private.py")
INIT = importlib.util.module_from_spec(INIT_SPEC)
INIT_SPEC.loader.exec_module(INIT)
CANARY_SPEC = importlib.util.spec_from_file_location("luna_factory_container_canary", CONTAINER_DIR / "acceptance_canary.py")
CANARY = importlib.util.module_from_spec(CANARY_SPEC)
CANARY_SPEC.loader.exec_module(CANARY)
sys.modules["acceptance_canary"] = CANARY
GRAPH_CANARY_SPEC = importlib.util.spec_from_file_location("luna_factory_planning_graph_canary", CONTAINER_DIR / "planning_graph_canary.py")
GRAPH_CANARY = importlib.util.module_from_spec(GRAPH_CANARY_SPEC)
GRAPH_CANARY_SPEC.loader.exec_module(GRAPH_CANARY)


def provenance():
    return {
        "plugin": {"version": "0.2.1"},
        "source": {
            "repository": "https://github.com/joshyorko/plugins",
            "tag": "v0.2.1",
            "commit": BUILD.RELEASE_COMMIT,
            "tree": BUILD.RELEASE_TREE,
        },
        "artifact": {
            "name": BUILD.RELEASE_ARCHIVE_NAME,
            "sha256": BUILD.RELEASE_ARCHIVE_SHA256,
        },
        "components": {
            "binary": {"sha256": BUILD.RELEASE_BINARY_SHA256},
            "ui": {"sha256": BUILD.RELEASE_UI_SHA256},
            "skill": {"sha256": BUILD.RELEASE_SKILL_SHA256},
        },
    }


class ContainerBuildTests(unittest.TestCase):
    def test_accepts_only_pinned_release_provenance(self):
        BUILD.validate_provenance(provenance())
        wrong = provenance()
        wrong["source"]["commit"] = "0" * 40
        with self.assertRaisesRegex(ValueError, "source commit"):
            BUILD.validate_provenance(wrong)

    def test_safely_extracts_regular_release_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = Path(tmp) / "bundle.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                member = tarfile.TarInfo("release/bin/luna-factoryd")
                member.size = 3
                tar.addfile(member, io.BytesIO(b"bin"))
            target = Path(tmp) / "unpacked"
            BUILD.safe_extract_archive(archive, target)
            self.assertEqual((target / "release/bin/luna-factoryd").read_bytes(), b"bin")

    def test_rejects_archive_path_traversal(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = Path(tmp) / "bad.tar"
            with tarfile.open(archive, "w:gz") as tar:
                member = tarfile.TarInfo("../outside")
                member.size = 1
                tar.addfile(member, io.BytesIO(b"x"))
            with self.assertRaisesRegex(ValueError, "unsafe archive member"):
                BUILD.safe_extract_archive(archive, Path(tmp) / "unpacked")
            self.assertFalse((Path(tmp) / "outside").exists())

    def test_internal_checksums_fail_closed_on_changed_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            package = Path(tmp)
            (package / "bin").mkdir()
            payload = package / "bin/luna-factoryd"
            payload.write_bytes(b"release")
            digest = hashlib.sha256(b"release").hexdigest()
            (package / "SHA256SUMS").write_text(f"{digest}  bin/luna-factoryd\n")
            BUILD.verify_internal_checksums(package)
            payload.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                BUILD.verify_internal_checksums(package)

    def test_runtime_bases_are_digest_pinned(self):
        containerfile = (CONTAINER_DIR / "Containerfile").read_text()
        self.assertIn("rust:1.99.0-slim-trixie@sha256:", containerfile)
        self.assertIn("python:3.13-slim-trixie@sha256:", containerfile)
        self.assertNotIn(":latest", containerfile)

    def test_build_fingerprint_covers_every_compiled_rust_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            plugin = root / "plugins/luna-factory"
            shutil.copytree(CONTAINER_DIR.parent / "server", plugin / "server")
            shutil.copytree(CONTAINER_DIR, plugin / "container")
            (plugin / "assets").mkdir()
            shutil.copy2(CONTAINER_DIR.parent / "assets/logo.png", plugin / "assets/logo.png")
            before = BUILD.source_patch_sha256(root)
            source = plugin / "server/src/native.rs"
            source.write_bytes(source.read_bytes() + b"\n")
            self.assertNotEqual(before, BUILD.source_patch_sha256(root))

    def test_release_delta_rejects_unreviewed_rust_source_changes(self):
        with self.assertRaisesRegex(ValueError, "unreviewed release source delta"):
            BUILD.validate_release_delta(["plugins/luna-factory/server/src/native.rs"])
        BUILD.validate_release_delta(
            [
                "plugins/luna-factory/server/src/config.rs",
                "plugins/luna-factory/container/Containerfile",
            ]
        )

    def test_compose_keeps_bind_loopback_and_config_read_only(self):
        compose = (CONTAINER_DIR / "compose.yaml").read_text()
        self.assertIn('"127.0.0.1:${LUNA_HOST_PORT:?', compose)
        self.assertIn("source: ${LUNA_CONFIG_DIR:?Set", compose)
        self.assertIn("read_only: true", compose)
        self.assertIn("source: ${LUNA_STATE_DIR:?Set", compose)
        self.assertIn("internal: true", compose)
        self.assertIn("healthcheck:", compose)
        self.assertIn('userns_mode: "keep-id:uid=65532,gid=65532"', compose)

    def test_private_setup_uses_separate_restricted_mounts_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "private" / "deployment"
            with socket.socket() as sock:
                sock.bind(("127.0.0.1", 0))
                port = sock.getsockname()[1]
            image_ref = "localhost/luna-factory@sha256:" + "a" * 64
            INIT.prepare_private(root, port, image_ref)
            config_dir = root / "config"
            state_dir = root / "state"
            operator = json.loads((config_dir / "operator.json").read_text())
            self.assertEqual(operator["published_origin"], f"http://127.0.0.1:{port}")
            self.assertEqual(operator["database"], "/var/lib/luna-factory/runs.sqlite")
            self.assertEqual((config_dir.stat().st_mode & 0o777), 0o700)
            self.assertEqual((state_dir.stat().st_mode & 0o777), 0o700)
            self.assertEqual(((config_dir / "operator.json").stat().st_mode & 0o777), 0o600)
            self.assertIn(image_ref, (root / ".env").read_text())
            with self.assertRaisesRegex(FileExistsError, "already exists"):
                INIT.prepare_private(root, port, image_ref)

    def test_canary_blocks_execution_and_cas_tool_calls_before_network(self):
        client = CANARY.McpClient("http://127.0.0.1:1/mcp")
        for name in ("start_factory", "resume_factory_run", "cancel_factory_run", "inspect_factory_cas"):
            with self.assertRaisesRegex(RuntimeError, "policy rejected"):
                client.call(name)

    def test_planning_graph_canary_uses_only_planning_tools(self):
        self.assertTrue({"create_factory_graph", "get_factory_graph", "propose_factory_change", "apply_factory_change"}.issubset(CANARY.ALLOWED_TOOL_CALLS))
        client = CANARY.McpClient("http://127.0.0.1:1/mcp")
        for name in ("start_factory", "resume_factory_run", "cancel_factory_run", "steer_factory_run", "inspect_factory_cas"):
            with self.assertRaisesRegex(RuntimeError, "policy rejected"):
                client.call(name)

    def test_planning_canary_compose_mounts_only_disposable_repository_read_only(self):
        compose = (CONTAINER_DIR / "compose.planning-canary.yaml").read_text()
        self.assertIn("LUNA_CANARY_REPO_DIR", compose)
        self.assertIn("target: /opt/luna-canary/repo", compose)
        self.assertIn("read_only: true", compose)


if __name__ == "__main__":
    unittest.main()
