"""Contract checks for the human and agent RCC operational recipes."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


PLUGIN = Path(__file__).resolve().parents[1]
SKILLS = PLUGIN / "skills"
ROUTER = SKILLS / "rcc" / "SKILL.md"
TLDR = SKILLS / "rcc-core" / "references" / "tldr" / "rcc.page.md"
README = SKILLS / "rcc-core" / "references" / "tldr" / "README.md"

RECIPES = {
    "rcc-core/references/rcc-source-recipes.md": {
        "RCC health": "rcc-core",
    },
    "rcc-core/references/environment-artifacts.md": {
        "Inspect Environment Artifact": "rcc-core",
    },
    "rcc-robots/references/rcc-command-recipes.md": {
        "New robot": "rcc-robots",
        "Run robot": "rcc-robots",
    },
    "rcc-robots/references/troubleshooting-validation.md": {
        "Debug environment": "rcc-robots",
    },
    "rcc-ci-maintenance/references/environment-artifact-ci.md": {
        "CI/cache troubleshooting": "rcc-ci-maintenance",
    },
}

FIELDS = (
    "Intent",
    "Owner",
    "Preflight",
    "Command",
    "Success evidence",
    "Side effects",
    "Failure routing",
    "Source",
)

SPECIALISTS = {
    "rcc",
    "rcc-core",
    "rcc-robots",
    "rcc-workitems",
    "rcc-robot-framework",
    "rcc-rpaframework",
    "rcc-ci-maintenance",
    "action-server",
}


def markdown_links(text: str) -> list[str]:
    """Return local Markdown link targets, excluding URLs and mailto links."""
    targets = re.findall(r"(?<!!)\[[^\]]+\]\(([^)]+)\)", text)
    return [target.split()[0].strip("<>") for target in targets
            if not re.match(r"(?:[a-z]+:)?//|mailto:", target, re.I)]


class RecipeContractTests(unittest.TestCase):
    def test_expected_recipe_headings_fields_and_owners(self) -> None:
        for relative_path, recipes in RECIPES.items():
            document = SKILLS / relative_path
            text = document.read_text(encoding="utf-8")
            for heading, owner in recipes.items():
                with self.subTest(document=relative_path, recipe=heading):
                    marker = f"## Recipe: {heading}"
                    self.assertIn(marker, text)
                    section = text.split(marker, 1)[1].split("\n## ", 1)[0]
                    for field in FIELDS:
                        match = re.search(
                            rf"\*\*{re.escape(field)}:\*\*\s*(.+)", section
                        )
                        self.assertIsNotNone(match, f"missing {field} in {heading}")
                        self.assertTrue(match.group(1).strip(), f"empty {field} in {heading}")
                    self.assertRegex(section, r"(?s)```[^\n]*\n.+?```")
                    owner_line = re.search(r"(?m)^\*\*Owner:\*\*\s*(.+)$", section)
                    self.assertIsNotNone(owner_line)
                    self.assertIn(f"`${owner}`", owner_line.group(1))

    def test_router_routes_natural_requests_to_exact_recipe_anchors(self) -> None:
        text = ROUTER.read_text(encoding="utf-8")
        expected = {
            "RCC health": "../rcc-core/references/rcc-source-recipes.md#recipe-rcc-health",
            "Inspect Environment Artifact": "../rcc-core/references/environment-artifacts.md#recipe-inspect-environment-artifact",
            "New robot": "../rcc-robots/references/rcc-command-recipes.md#recipe-new-robot",
            "Run robot": "../rcc-robots/references/rcc-command-recipes.md#recipe-run-robot",
            "Debug environment": "../rcc-robots/references/troubleshooting-validation.md#recipe-debug-environment",
            "CI/cache troubleshooting": "../rcc-ci-maintenance/references/environment-artifact-ci.md#recipe-cicache-troubleshooting",
        }
        for phrase, target in expected.items():
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, text)
                self.assertIn(target, text)

    def test_quick_commands_preserve_safety_boundaries(self) -> None:
        for relative_path, recipes in RECIPES.items():
            text = (SKILLS / relative_path).read_text(encoding="utf-8")
            for heading in recipes:
                section = text.split(f"## Recipe: {heading}", 1)[1].split("\n## ", 1)[0]
                commands = "\n".join(re.findall(r"```sh\n(.*?)```", section, re.S))
                with self.subTest(recipe=heading):
                    self.assertNotRegex(commands, r"--(?:force|permissive-local|lockless|liveonly|warranty-voided)\b")
                    self.assertNotRegex(commands, r"\b(?:rm|cleanup|repair|publish|remove)\b")
                    for line in commands.splitlines():
                        if "rcc env lifecycle" in line:
                            self.assertIn('ROBOCORP_HOME="$inspection_home"', line)
                            self.assertIn("RCC_HOLOTREE_MODE=private", line)
                            self.assertIn("nonproduction", section)
                            self.assertIn("stale lease", section)
                            self.assertIn("crash journal", section)

    def test_tldr_has_six_to_eight_single_command_examples(self) -> None:
        text = TLDR.read_text(encoding="utf-8")
        self.assertTrue(README.is_file(), "tldr README is missing")
        lines = text.splitlines()
        self.assertEqual(lines[0], "# rcc")
        self.assertTrue(any(line.startswith("> ") and not line.startswith("> More information:")
                            and line[2:].strip() for line in lines))
        self.assertTrue(any(line.startswith("> ") and "More information:" in line for line in lines))
        self.assertNotRegex(text, r"```|\*\*|__")
        examples = []
        for index, line in enumerate(lines):
            if re.fullmatch(r"- .+:", line):
                self.assertLess(index + 2, len(lines))
                self.assertEqual(lines[index + 1], "")
                command_line = lines[index + 2]
                self.assertRegex(command_line, r"^`[^`]+`$")
                examples.append(command_line[1:-1])
        self.assertGreaterEqual(len(examples), 6)
        self.assertLessEqual(len(examples), 8)
        for example in examples:
            with self.subTest(example=example):
                self.assertRegex(example, r"^rcc(?:\s|$)")
                self.assertNotRegex(example, r"\s+`[^`]+`")

    def test_placeholders_are_balanced_and_nonempty(self) -> None:
        paths = [TLDR]
        paths.extend(SKILLS / path for path in RECIPES)
        for path in paths:
            text = path.read_text(encoding="utf-8")
            index = 0
            while index < len(text):
                if text[index] == "{":
                    self.assertTrue(text.startswith("{{", index), f"single opening brace in {path}")
                    end = text.find("}}", index + 2)
                    self.assertNotEqual(end, -1, f"unclosed placeholder in {path}")
                    content = text[index + 2:end]
                    self.assertTrue(content.strip(), f"empty placeholder in {path}")
                    self.assertNotRegex(content, r"[{}]", f"nested brace in {path}")
                    index = end + 2
                elif text[index] == "}":
                    self.fail(f"unmatched closing brace in {path}")
                else:
                    index += 1

    def test_local_markdown_links_resolve(self) -> None:
        documents = [ROUTER, TLDR, README]
        documents.extend(SKILLS / path for path in RECIPES)
        for document in documents:
            for target in markdown_links(document.read_text(encoding="utf-8")):
                path_part, _, anchor = target.partition("#")
                destination = (document.parent / path_part).resolve() if path_part else document
                with self.subTest(document=document, target=target):
                    self.assertTrue(destination.is_file(), f"broken link: {target}")
                    if anchor:
                        target_text = destination.read_text(encoding="utf-8")
                        anchors = {
                            re.sub(r"[^a-z0-9 -]", "", heading.lower()).strip().replace(" ", "-")
                            for heading in re.findall(r"(?m)^#{1,6}\s+(.+?)\s*#*\s*$", target_text)
                        }
                        self.assertIn(anchor.lower(), anchors, f"broken anchor: {target}")

    def test_eight_specialists_and_generated_skill_links_remain(self) -> None:
        self.assertEqual({path.name for path in SKILLS.iterdir() if path.is_dir()}, SPECIALISTS)
        repo = PLUGIN.parents[1]
        for generated_root in (repo / "skills", repo / ".agents" / "skills"):
            for name in SPECIALISTS:
                link = generated_root / name
                with self.subTest(link=link):
                    self.assertTrue(link.is_symlink(), f"generated skill must be a symlink: {link}")
                    self.assertEqual(link.resolve(), (SKILLS / name).resolve())

    def test_specialist_skills_link_to_their_recipe_anchors(self) -> None:
        specialist_docs = {
            "rcc-core": SKILLS / "rcc-core" / "SKILL.md",
            "rcc-robots": SKILLS / "rcc-robots" / "SKILL.md",
            "rcc-ci-maintenance": SKILLS / "rcc-ci-maintenance" / "SKILL.md",
        }
        for relative_path, recipes in RECIPES.items():
            owner = next(iter(recipes.values()))
            skill_text = specialist_docs[owner].read_text(encoding="utf-8")
            for heading in recipes:
                anchor = re.sub(r"[^a-z0-9 -]", "", f"recipe-{heading}".lower()).strip().replace(" ", "-")
                target = f"{relative_path.split('/', 1)[1]}#{anchor}"
                with self.subTest(owner=owner, target=target):
                    self.assertIn(target, skill_text)


@unittest.skipUnless(shutil.which("tldr"), "Tealdeer is required for TLDR setup checks")
class TealdeerSetupTests(unittest.TestCase):
    def setUp(self) -> None:
        version = subprocess.run(["tldr", "--version"], text=True, capture_output=True,
                                 check=True)
        if not version.stdout.startswith("tealdeer "):
            self.skipTest("TLDR setup checks require the Tealdeer client")
        temporary = tempfile.TemporaryDirectory(prefix="rcc-tldr-setup-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.skill = self.root / "installed skills" / "rcc-core"
        destination = self.skill / "references" / "tldr"
        destination.mkdir(parents=True)
        shutil.copy2(TLDR, destination / TLDR.name)
        self.environment = os.environ.copy()
        self.environment.update({
            "XDG_CONFIG_HOME": str(self.root / "config"),
            "XDG_CACHE_HOME": str(self.root / "cache"),
            "XDG_DATA_HOME": str(self.root / "data"),
            "NO_COLOR": "1",
        })
        config = self.root / "config" / "tealdeer" / "config.toml"
        config.parent.mkdir(parents=True)
        config.write_text("[updates]\nauto_update = true\n")

    def run_example(self, marker: str) -> subprocess.CompletedProcess[str]:
        examples = re.findall(r"```sh\n(.*?)```", README.read_text(), re.S)
        command = next(example for example in examples if marker in example)
        return subprocess.run(["sh", "-eu", "-c", command], cwd=self.skill,
                              env=self.environment, text=True, capture_output=True,
                              timeout=30)

    def test_preview_works_from_an_installed_skill_without_a_cache(self) -> None:
        result = self.run_example("--render")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("rcc version", result.stdout)
        self.assertFalse((self.root / "cache").exists())

    def test_fresh_install_serves_the_page_without_downloading_a_cache(self) -> None:
        result = self.run_example("install -D")
        self.assertEqual(result.returncode, 0, result.stderr)
        page = self.root / "data" / "tealdeer" / "pages" / "rcc.page.md"
        self.assertEqual(page.read_bytes(), TLDR.read_bytes())
        result = subprocess.run(["tldr", "--no-auto-update", "rcc"],
                                cwd=self.root, env=self.environment, text=True,
                                capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("rcc version", result.stdout)

    def test_install_keeps_an_existing_custom_page(self) -> None:
        page = self.root / "data" / "tealdeer" / "pages" / "rcc.page.md"
        page.parent.mkdir(parents=True)
        page.write_text("existing custom page\n")
        result = self.run_example("install -D")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(page.read_text(), "existing custom page\n")
        self.assertIn("already exists", result.stderr)


@unittest.skipUnless(
    os.environ.get("RCC_RECIPE_RUNTIME") == "1" and shutil.which("rcc"),
    "set RCC_RECIPE_RUNTIME=1 with rcc installed to run disposable CLI checks",
)
class DisposableRccRuntimeTests(unittest.TestCase):
    def test_broken_robot_fails_at_environment_boundary_without_running_task(self) -> None:
        with tempfile.TemporaryDirectory(prefix="rcc-recipe-test-") as temp:
            root = Path(temp)
            home = root / "robocorp-home"
            home.mkdir()
            fixture = root / "robot.yaml"
            fixture.write_text(
                "tasks:\n  Main:\n    command:\n      - python\n      - -c\n      - \"from pathlib import Path; Path('TASK_RAN').write_text('yes')\"\n"
                "condaConfigFile: missing-environment.yaml\n",
                encoding="utf-8",
            )
            environment = os.environ.copy()
            environment["ROBOCORP_HOME"] = str(home)
            environment["RCC_HOLOTREE_MODE"] = "private"
            for args in (["version"], ["diagnostics", "--quick", "--json"]):
                result = subprocess.run(["rcc", *args], cwd=root, env=environment,
                                        text=True, capture_output=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                if args == ["version"]:
                    self.assertIn("v18.19.7", result.stdout)
            result = subprocess.run(
                ["rcc", "ht", "vars", "-r", str(fixture), "--json"], cwd=root,
                env=environment, text=True, capture_output=True, timeout=30,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing-environment.yaml", result.stdout + result.stderr)
            self.assertFalse((root / "TASK_RAN").exists())


if __name__ == "__main__":
    unittest.main()
