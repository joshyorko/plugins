# Luna Factory UX redesign: slice brief (shared by slices 1–5)

You are building ONE slice of the owner-approved five-slice redesign of the Luna Factory MCP Apps UI (`plugins/luna-factory/ui`).

## Spec
- The spec is `/home/kdlocpanda/.cache/luna-factory-briefs/ux-review-2026-10-14.md`. Read all of it once: §2 problems, §3 IA and glossary, §4 design system tokens, §5 per-view wireframes. Then build exactly your slice from §7, and meet every acceptance bullet for it.
- Don't pull work forward from later slices, except what your slice's acceptance needs. Don't undo earlier slices.
- The owner's taste is in `/home/kdlocpanda/.claude/projects/-home-kdlocpanda-second-brain-Areas-plugins/memory/luna-ux-simplicity.md`:
  - Minimal, professional and calm.
  - A repo picker, one box and go.
  - No forms full of options, no raw IDs, no pill-shaped expanders.
  - Keep the moon marks. hivecommons/hive is the prior art the owner trusts.

## Machine safety (mandatory: the machine has 15 GB RAM, and most of it is in use)
- Run everything with `nice -n 10`. Run vitest as `npx vitest run --maxWorkers=1`.
- Browser captures go through the lock and run one at a time: `flock /tmp/luna-browser.lock nice -n 10 npm run test:host`.
- No dev server, no watch mode, no second Chrome. If `free -g` shows under 2 GB available, wait 60 s before a heavy step.
- This is UI-only work. Don't run cargo unless you change server code; you shouldn't need to. If you must, use the locked podman command in `/home/kdlocpanda/.cache/luna-factory-briefs/lf-brief-v2.md`.
- Python: the venv is `$HOME/.cache/lf-venv`. It has no pytest, so use `python -m unittest`. The test is `tests/test_container_build.py` → `test_current_head_release_delta_is_reviewed`. If you change files outside `container/` or `ui/`, add them to `ALLOWED_RELEASE_DELTA` in `container/build_image.py`.

## Mechanics
- Build `dist` with `npm run build`, commit `ui/dist/index.html`, and keep the bundle parity test green.
- Re-capture with `npm run test:host`. Commit the changed PNGs under `ui/test/dogfood-visual-snapshots/`.
- If you add or remove captures, update the capture count asserted in `ui/test/dogfood-snapshots.test.ts`.
- Look at EVERY capture yourself, with the Read tool on each PNG, light and dark, wide, narrow and mobile. Fix what looks wrong.
  - You are the design reviewer. The bar is "professional, enterprise-grade product".
  - If an animation is caught mid-fade, keep main's copy of that PNG and say so.
- Required checks before pushing: `npm run typecheck`, the full vitest run, `npm run build`, `npm run test:host` (PASS, zero mutation calls, zero automatic messages), and the release-delta unittest.

## Git and PR
- Work in the worktree you're given, on its branch. It was cut from the current `origin/main`.
- Make one commit, in the form `feat(luna-factory): <slice summary> (UX slice N/5)`, ending with the trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Push with `git push -u origin HEAD`.
- Open a DRAFT PR against main with `gh pr create --draft`. The PR body needs:
  - purpose;
  - the slice's acceptance bullets, each with ✅ or an explanation;
  - the checks you ran;
  - the list of changed captures;
  - `Part of #67` (conversation-first mission control design; never write Closes);
  - the final line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
- Don't merge. Don't comment on other PRs. Don't create issues.
- Never touch secrets, keys, tunnels, the dogfood stack or the server config.

## Result
Report back exactly:
- the head SHA and the PR URL;
- a per-acceptance-bullet status;
- the checks you ran, with results;
- the capture count;
- the 3–5 most representative screenshot paths;
- what you deliberately left for later slices.

Never claim a check you didn't run.
