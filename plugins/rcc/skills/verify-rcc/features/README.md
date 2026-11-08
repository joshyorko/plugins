# RCC verification map

Read the owning feature before driving. The primary surface is the installed RCC CLI; Tealdeer and portable/Codex/Claude/Hermes packaging are secondary surfaces. Use the Launch and Doctor instructions in [the skill](../SKILL.md) first. There is no shared server or account to seed.

Each run uses fresh owned scratch with separate retained evidence. Command transcripts must identify the user path and resulting state; skipped or unavailable paths remain unverified. An expected environment failure is successful only when the task did not execute. Run only a journey whose dependency and side-effect requirements fit the current request.

- [Health and failure boundary](health.md): diagnostics and missing-environment refusal before task execution.
- [Onboarding and local fixtures](onboarding.md): template catalog, first task PASS/artifacts, overwrite refusal, producer/consumer wiring, and dummy FileSecrets.
- [Offline quick reference](quickref.md): managed page opt-in, real Tealdeer lookup, configuration preservation, and uninstall.
- [Distribution and discovery](distribution.md): generated views, manifest contracts, canonical skill links, and Hermes registration.

All four are independently selectable by `scripts/verify-rcc drive`. The onboarding journey deliberately runs its starter and local SDK chain together; it requires the listed template and its network-provisioned dependencies. Distribution contracts do not qualify native client installation, graphical rendering, hosted services, release publication, or Windows runtime behavior.
