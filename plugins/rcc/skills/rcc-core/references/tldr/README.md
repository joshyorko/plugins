# RCC TLDR page

[`rcc.page.md`](rcc.page.md) is a standard tldr page for the RCC command line. It lives under this skill's `references/` directory so the packaged skill includes it.

Tealdeer is optional; agents use the specialist recipes without installing it. Before creating a project, select `template_name` from `rcc robot init --json`. Review generated configuration and hooks before resolving or executing a robot.

## Preview

Use Tealdeer's `--render` option to render the page without installing it:

```sh
tldr --render plugins/rcc/skills/rcc-core/references/tldr/rcc.page.md
```

## Install for Tealdeer

Tealdeer reads custom pages from its custom pages directory. On Linux the default is usually `~/.local/share/tealdeer/pages/`. Get the actual path with `tldr --show-paths`, set `custom_pages_dir` to that path, and copy the file there as `rcc.page.md`. The guard below stops if that filename already exists:

```sh
custom_pages_dir="$HOME/.local/share/tealdeer/pages"
if test -e "${custom_pages_dir%/}/rcc.page.md"; then
  printf '%s\n' 'rcc.page.md already exists; keep or back it up before replacing it.' >&2
else
  install -D plugins/rcc/skills/rcc-core/references/tldr/rcc.page.md "${custom_pages_dir%/}/rcc.page.md"
fi
```

Replace the example directory with the `Custom pages dir` path reported on your system.
After installation, run `tldr rcc` to view the page.
On a fresh Tealdeer installation, run `tldr --update` once if it reports a missing page cache. This downloads Tealdeer's upstream pages; it is not required for `--render` or for the agent recipes.

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

The MCP server research confirmed that it retrieves and searches existing tldr pages. Existing skill links provide discovery here, so a new server would add setup without improving RCC command selection. The Copilot skill's example-first format is useful; its generator and tool-specific prompting are unnecessary for a maintained static page.

## Validation

Run the plugin contracts from the repository root with `python3 -m unittest discover -s plugins/rcc/tests -v`, alongside `bin/check`. Set `RCC_RECIPE_RUNTIME=1` for the disposable broken-environment check with RCC v18.19.7 installed. That check diagnoses a missing environment file without executing a robot task; it does not prove dependency installation or a successful task run.
