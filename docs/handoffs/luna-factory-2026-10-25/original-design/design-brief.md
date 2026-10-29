# Mockup brief for all three Luna Factory directions

Read `mockup-fixture.md` in this folder first. All directions show the same campaign data.

## Product truth

- ChatGPT is the control surface. The app shows context and visual feedback. It is not a
  second chatbot and has no free-text prompt box of its own.
- The unit of work is the **campaign**: a parent GitHub issue plus its child issue graph,
  run by the Luna coordinator with parallel workers. "Run", "revision", "claim", "turn",
  "subject" and hashes are internal words and must not appear in primary UI.
- Starting is conversational: "luna yolo #101". The app has no start form on the primary path.
- Attention means a real human decision (protected resource, scope change, authority
  ceiling). Retries, polling, unverified evidence and inactive canaries are "Luna is handling"
  or "unverified", never urgent.
- No fake liveness. Motion means a real event happened. Every live element shows its
  freshness ("updated 14:06"). A worker with no event in 10+ minutes looks quiet, not busy.

## ChatGPT surface realism

- **Fullscreen** (1440×900): draw ChatGPT's thin top bar (app icon, "Luna Factory", close X)
  and keep ChatGPT's composer docked at the bottom (pill input "Ask about this campaign…").
  The composer shows a selection-context chip such as "Worker 4 · #109" when something is
  selected. That chip is how the selection reaches the model. It is ChatGPT chrome, not app UI.
- **Inline card** (~720 wide): sits in a conversation under a user message. No internal scrolling,
  no tabs, at most two actions, glanceable. One primary action: open fullscreen.
- **Mobile** (390×844): fullscreen on a phone with the composer at the bottom.
- System font stack only (`-apple-system, BlinkMacSystemFont, "Segoe UI", Inter, Roboto, sans-serif`),
  `ui-monospace` for code. No remote assets. Inline SVG icons only.
- Accessible: real text, 4.5:1 contrast, state never encoded by color alone (shape or text too).

## Anti-goals

No CRUD dashboard, no grid of rectangular stat cards, no decorative pill badges everywhere,
no raw hashes, no giant forms, no nested accordions, no graph editor on the primary path.

## Artboards per direction (one self-contained HTML file each)

1. `<dir>-overview.html`: 1440×900 fullscreen. Multi-campaign home: 4 campaigns, where you're needed.
2. `<dir>-swarm.html`: 1440×900 fullscreen. Campaign #101 running: agents, lanes, graph, events, PRs.
3. `<dir>-inspect.html`: 1440×900 fullscreen. Worker 4 on #109 selected: task, evidence, checks,
   blocker, timeline, subagent. The composer chip shows the selection.
4. `<dir>-inline.html`: 760×560. ChatGPT conversation: the user's YOLO message, an assistant reply line,
   then the inline card. Light theme.
5. `<dir>-mobile.html`: 390×844. Phone fullscreen of the swarm (or attention) view.

Screenshot each with:
`google-chrome --headless --no-sandbox --disable-gpu --hide-scrollbars --window-size=W,H --screenshot=../shots/<name>.png file:///abs/path.html`
