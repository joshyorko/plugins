# Luna Factory UX redesign (direction selection)

Design artifacts only. No production code in `Areas/plugins` was changed.

- `01-ux-audit.md`: audit of the current workbench (PR #66, UI 0.2.1 fixture captures and source).
- `02-platform-capabilities.md`: ChatGPT surfaces and bridge APIs that shape the design.
- `03-design-spec.md`: the three directions, the recommendation (B, Campaign Map), screen hierarchy,
  interactions, agent visualization, surfaces, progressive disclosure, a supported-today vs.
  backend matrix (including the CAS timeline source), prioritized acceptance criteria and the
  build handoff.
- `mockup-fixture.md`, `design-brief.md`: the shared campaign data and the brief behind every mockup.
- `mockups/{a,b,c}-{overview,swarm,inspect,inline,mobile}.html` and `shots/*.png`: the mockups.
  A = Flight Recorder, B = Campaign Map, C = Dispatch.
- `compare.html`: side-by-side gallery.

## Implementation status (2026-10-10)

- Direction selected: B Campaign Map (primary) + A Lanes (synchronized) + C narrative line, with the lunar identity.
- Working slice: https://github.com/joshyorko/plugins/pull/68 (branch `luna-factory/mission-control-ui`, based on `main` after #66).
- Source of truth for engineering: `plugins/luna-factory/docs/mission-control.md` in that PR. It holds the design, truth rules, motion storyboard and contract matrix.
- Issue graph: #67 with sub-issues #69–#82.
- Evidence:
  - `ui/test/live-visual-snapshots/`: a real `luna-factoryd` rendering the actual #67 issue graph.
  - `ui/test/dogfood-visual-snapshots/`: synthetic AppBridge captures.

The mockups in this folder stay design references. They are not backend evidence.
