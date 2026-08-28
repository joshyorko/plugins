from __future__ import annotations

import json
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import mock

from scripts import build_runtime_views


class RuntimeViewsTest(unittest.TestCase):
    def make_temp_repo(self) -> Path:
        root = Path(tempfile.mkdtemp())
        skills_root = root / "plugins" / "37signals" / "skills"
        active = skills_root / "37signals-rails-implement"
        refresh = skills_root / "37signals-product-refresh"
        stale = skills_root / "37signals-model"
        active.mkdir(parents=True)
        refresh.mkdir()
        stale.mkdir()
        (active / "SKILL.md").write_text("# Active\n", encoding="utf-8")
        (refresh / "SKILL.md").write_text("# Refresh\n", encoding="utf-8")
        (stale / "SKILL.md").write_text("# Stale\n", encoding="utf-8")
        (skills_root.parent / "skills.active.yml").write_text(
            '{\n'
            '  "max_active": 10,\n'
            '  "skills": ["37signals-rails-implement", "37signals-product-refresh"],\n'
            '  "recipes": []\n'
            '}\n',
            encoding="utf-8",
        )
        return root

    def test_collect_skills_uses_37signals_active_manifest(self) -> None:
        root = self.make_temp_repo()
        catalog = {"plugins": [{"name": "37signals"}]}

        with mock.patch.object(build_runtime_views, "ROOT", root):
            canonical, standalone = build_runtime_views.collect_skills(catalog)

        self.assertEqual(
            ["37signals-rails-implement", "37signals-product-refresh"],
            list(canonical),
        )
        self.assertEqual(
            ["37signals-rails-implement", "37signals-product-refresh"],
            list(standalone),
        )

    def test_sync_dir_replaces_stale_symlink_with_active_symlink(self) -> None:
        root = self.make_temp_repo()
        target_dir = root / ".agents" / "skills"
        target_dir.mkdir(parents=True)
        stale_link = target_dir / "37signals-model"
        stale_link.symlink_to("../../plugins/37signals/skills/37signals-model")
        active_target = root / "plugins" / "37signals" / "skills" / "37signals-rails-implement"

        with mock.patch.object(build_runtime_views, "ROOT", root), redirect_stdout(StringIO()):
            changed = build_runtime_views.sync_dir(
                target_dir,
                {"37signals-rails-implement": active_target},
                check=False,
            )

        active_link = target_dir / "37signals-rails-implement"
        self.assertTrue(changed)
        self.assertFalse(stale_link.exists() or stale_link.is_symlink())
        self.assertTrue(active_link.is_symlink())
        self.assertEqual(
            Path("../../plugins/37signals/skills/37signals-rails-implement"),
            Path(active_link.readlink()),
        )

    def test_repository_catalog_discovers_luna_factory(self) -> None:
        catalog = json.loads(build_runtime_views.CATALOG_PATH.read_text())

        canonical, standalone = build_runtime_views.collect_skills(catalog)

        expected = build_runtime_views.ROOT / "plugins" / "luna-factory" / "skills" / "luna-factory"
        self.assertEqual(expected, canonical["luna-factory"])
        self.assertEqual(expected, standalone["luna-factory"])


if __name__ == "__main__":
    unittest.main()
