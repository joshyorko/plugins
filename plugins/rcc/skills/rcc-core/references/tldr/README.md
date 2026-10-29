# RCC TLDR page

[`rcc.page.md`](rcc.page.md) is a standard tldr page for the RCC command line. It lives under this skill's `references/` directory so the packaged skill includes it.

Tealdeer is optional; agents use the specialist recipes without installing it. Before creating a project, select `template_name` from `rcc robot init --json`. Review generated configuration and hooks before resolving or executing a robot.

## Preview

Run the shell examples from the `rcc-core` skill directory, the directory containing its `SKILL.md`. In a checkout this is `plugins/rcc/skills/rcc-core`; for an installed plugin, use the skill directory shown by your agent. The examples work in either location.

Use Tealdeer's `--render` option to render the page without installing it or creating a cache:

```sh
tldr --render references/tldr/rcc.page.md
```

## Managed daily use

Tealdeer must already be installed. Installing the RCC plugin alone does not create a custom page. To opt in from a marketplace installation, use the plugin's Tealdeer setup starter prompt or run the packaged helper from the `rcc-core` skill directory:

```sh
python3 scripts/tealdeer.py install
```

The helper reads `tldr --show-paths`; it does not edit Tealdeer configuration or download anything. It prefers a managed symlink and falls back to a managed copy if the filesystem does not allow symlinks. Use `install --mode copy` to request copies explicitly. Existing regular files and unrelated symlinks are preserved, even if their content matches the bundled page.

An existing symlink to a canonical RCC page is adopted without changing its inode or target. Adopted links continue to follow their existing checkout; update that checkout to refresh the page. The helper never modifies that checkout or removes an adopted user link during uninstall.

For a stable checkout, a new link points directly to its page and follows updates immediately. For a versioned marketplace bundle, a new link points to a durable managed page outside the plugin cache. That page remains available when old cache versions are removed. Explicit setup registers one owned `SessionStart` entry in `${CODEX_HOME:-~/.codex}/hooks.json` and a durable helper beside the receipt. Existing hook entries are preserved. Codex must allow and trust this user hook; setup does not bypass host trust policy. Codex 0.164 skips bundled hooks for portable Agent Plugins, so this integration does not depend on a bundled hook or an upgrade callback.

After an upgrade, the user hook reads the active RCC version from the local Codex cache and refreshes the managed page without network access. It follows Codex 0.164's version selection, including its `local` cache entry. If hooks are disabled, synchronize manually from the upgraded bundle:

```sh
python3 scripts/tealdeer.py sync
```

Bootstrap installations can opt in with `--with-rcc-tldr` or `--rcc-tldr-mode copy`. Subsequent installer runs synchronize an opted-in page without requiring the flag again. Setup creates only an empty Tealdeer cache directory for offline custom-page lookup; `tldr rcc` then works from any directory. Automatic Tealdeer upstream updates remain governed by your existing configuration; use `tldr --no-auto-update rcc` to require offline lookup.

Ownership is recorded in `${XDG_STATE_HOME:-~/.local/state}/agent-plugins/rcc-tealdeer/state.json`, separate from plugin versions. User edits or replacements are preserved instead of overwritten. Inspect the receipt or disable the integration independently of the plugin:

```sh
python3 scripts/tealdeer.py status
python3 scripts/tealdeer.py uninstall
```

Uninstall removes only unchanged pages created by the helper, its exact owned hook entry, its unchanged durable helper, and its receipt. It preserves adopted links, changed pages or hooks, Tealdeer configuration, cache contents, and the RCC plugin. A symlinked or malformed Codex hooks configuration is preserved and requires manual synchronization instead of automatic hook registration. The repository's bootstrap uninstaller also disables the integration. Uninstall before removing the plugin if you want the managed page removed; the native plugin manager does not invoke an external-file uninstall callback.

## Manual unmanaged setup

Tealdeer reads custom pages from its custom pages directory. Run `tldr --show-paths` first. The variables below use Linux's XDG defaults; if your configuration reports different paths, set them to the reported `Custom pages dir` and `Cache dir` before continuing. The guard keeps an existing custom page.

Tealdeer 1.9.1 also requires a `tldr-pages` directory under its cache directory before serving custom pages. Creating that empty directory enables offline lookup; it does not download upstream pages or clear an existing cache.

```sh
custom_pages_dir="${XDG_DATA_HOME:-$HOME/.local/share}/tealdeer/pages"
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/tealdeer"
if test -e "${custom_pages_dir%/}/rcc.page.md"; then
  printf '%s\n' 'rcc.page.md already exists; keep or back it up before replacing it.' >&2
else
  install -D references/tldr/rcc.page.md "${custom_pages_dir%/}/rcc.page.md" &&
    mkdir -p "${cache_dir%/}/tldr-pages" &&
    tldr --no-auto-update rcc
fi
```

After installation, `tldr rcc` displays the page. The setup command uses `--no-auto-update` so it also works when automatic cache updates are enabled. Use `tldr --update` only if you also want to download upstream pages. Neither preview nor custom-page lookup requires that download.

If you prefer a separate directory, set an absolute `custom_pages_dir` in Tealdeer's configuration:

```toml
[directories]
custom_pages_dir = "/home/you/.local/share/tealdeer/rcc-pages/"
```

Then copy `rcc.page.md` into that directory. Tealdeer custom pages use the `<command>.page.md` filename; a `.patch.md` file extends an upstream page instead of replacing it.

## Format and sources

The page follows tldr's example-first Markdown format: a short description, concrete commands, and `{{placeholder}}` values. The tldr project recommends about five examples and caps a page at eight. The examples reflect RCC's local automation workflows and were checked against the installed v18.19.7 help output. `rcc ht vars` can build environments, download packages, and run hooks; `rcc run` executes the selected task.

- [tldr page format and contribution guide](https://github.com/tldr-pages/tldr/blob/main/CONTRIBUTING.md)
- [Tealdeer custom pages](https://tealdeer-rs.github.io/tealdeer/usage_custom_pages.html)
- [Tealdeer custom page directory configuration](https://tealdeer-rs.github.io/tealdeer/config_directories.html)
- [Tealdeer command page](https://github.com/tealdeer-rs/tealdeer/blob/main/pages/tealdeer.md)
- [GitHub Copilot create-tldr-page skill](https://github.com/github/awesome-copilot/blob/main/skills/create-tldr-page/SKILL.md)
- [tldr MCP server](https://github.com/onatm/tldr-mcp-server)
- [RCC source](https://github.com/joshyorko/rcc)
- [Codex 0.164 installed-version selection](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core-plugin-common/src/installed.rs#L47)
- [Codex 0.164 portable-plugin hook boundary](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core-plugins/src/loader.rs#L973)

The MCP server research confirmed that it retrieves and searches existing tldr pages. Existing skill links provide discovery here, so a new server would add setup without improving RCC command selection. The Copilot skill's example-first format is useful; its generator and tool-specific prompting are unnecessary for a maintained static page.

## Validation

Run the plugin contracts from the repository root with `python3 -m unittest discover -s plugins/rcc/tests -v`, alongside `bin/check`. Set `RCC_RECIPE_RUNTIME=1` for the disposable broken-environment check with RCC v18.19.7 installed. That check diagnoses a missing environment file without executing a robot task; it does not prove dependency installation or a successful task run.
