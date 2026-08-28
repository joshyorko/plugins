from __future__ import annotations

from pathlib import Path


def register(ctx):
    base = Path(__file__).parent
    skills_dir = base / "skills"
    if not skills_dir.is_dir():
        return

    for skill_md in sorted(skills_dir.glob("*/SKILL.md")):
        name = skill_md.parent.name
        description = _read_description(skill_md)
        ctx.register_skill(name=name, path=skill_md, description=description)


def _read_description(skill_md: Path) -> str:
    try:
        text = skill_md.read_text(encoding="utf-8")
    except OSError:
        return ""

    if not text.startswith("---"):
        return ""

    end = text.find("\n---", 3)
    if end == -1:
        return ""

    for line in text[3:end].splitlines():
        if line.startswith("description:"):
            return line.split(":", 1)[1].strip().strip("\"'")
    return ""
