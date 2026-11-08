# Offline RCC quick reference

A user opts into the packaged RCC page, reads it with the actual Tealdeer CLI from another directory, and removes the owned page without changing Tealdeer configuration.

## Sub-features

- `quickref-enable`: the packaged helper installs a managed page and an owned hook in a disposable Codex home.
- `quickref-read`: offline lookup renders RCC commands through the real Tealdeer client.
- `quickref-remove`: uninstall removes the managed page and preserves configuration.
- `quickref-cache`: offline lookup leaves the download cache without files.

## How to get to it (user POV)

- In the `rcc-core` skill directory, use `python3 scripts/tealdeer.py install` to opt in.
- Run `tldr --no-auto-update rcc` from any directory.
- Use the packaged helper's `status` and `uninstall` commands to inspect and disable the integration.

## Driving it with verify-rcc

Preconditions: Launch and Doctor passed; `tldr --version` identifies Tealdeer. No cache download or real user hook configuration is required.

- **Drive the managed lifecycle:** run `"$verification_python" "$verify_rcc" drive quickref --run-dir "$verification_run"`. The helper selects isolated XDG paths and passes an explicit scratch `--codex-home` during setup.
- **Inspect the page:** command evidence must show `tldr --no-auto-update rcc` invoked outside the skill directory, exit zero, and rendered `rcc version`.
- **Inspect removal:** `quickref-state.json` must report configuration preserved, managed page removed, and zero cache files. Command evidence includes install, status, and uninstall.

## Gotchas

- Other `tldr` implementations are not qualified by this recipe; report the missing Tealdeer precondition.
- This journey covers the fresh managed-page path. Adoption, edited-page preservation, and marketplace-cache upgrade branches are separate cases in `tests/test_rcc_tealdeer.py`, exercised by the distribution suite.
- Empty cache creation is an expected side effect. The filesystem check proves no downloaded cache files, not a packet-level audit of every subprocess.
- Creating a hook file does not prove a native Codex session invoked it; native hook trust and client-upgrade acceptance remain unverified.
