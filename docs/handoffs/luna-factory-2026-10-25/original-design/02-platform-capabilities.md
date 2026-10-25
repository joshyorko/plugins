# ChatGPT platform capabilities that shape the design (checked 2026-10-10)

Sources: developers.openai.com/plugins/build/chatgpt-ui, /plugins/build/extensions,
/apps-sdk/reference, /apps-sdk/concepts/ui-guidelines and design-guidelines,
github.com/openai/mcp-extensions (docs/spec.md, plugins/bits-and-bolts),
github.com/modelcontextprotocol/ext-apps (SEP-1865, spec 2026-01-26).
"Verified" means read on a fetched page or in source. Nothing here has been seen
rendered in an authenticated ChatGPT session.

## Surfaces

| Surface | What it is | Design consequence |
| --- | --- | --- |
| Inline card | Rendered in the conversation under a tool result. Auto-fits its content, with no nested scrolling, tabs or drill-in. At most 2 actions: one primary and one optional secondary. | A glanceable "pulse" of the campaign, with one primary action: open fullscreen. |
| Fullscreen | Immersive view. ChatGPT's composer stays overlaid at the bottom and shimmers while a reply streams. | Mission control lives here. Reserve the bottom about 96px for the composer. The composer is the control. |
| PiP | Pinned overlay that must respond to composer input. Mobile may show it as fullscreen. `openai/ui.availableDisplayModes` is typed `inline \| fullscreen` only. | Treat as optional or future. Don't depend on it. |
| Global entrypoint | Sidebar entry. Opens as a permanent tab with its own thread, in fullscreen. | The multi-campaign home. |
| Thread entrypoint | Side-panel tab, one instance per thread. Opens in fullscreen. | The campaign this conversation is about, which is the natural place for "watch #101 while I chat". |
| Mobile | Inline cards can grow to device height. No `send:false` drafts, no OpenAI forms, no deep links on Android. | A single-column attention-first layout. Decisions fall back to plain chat replies. |

## Bridge capabilities used by the design

- `ui/update-model-context` sets context without sending a message. Each call replaces the
  last one, and the host sends only the latest on the next user turn. ChatGPT adds
  `openai/title` chip labels, `openai/thumbnail` and `annotations.audience: ["assistant"]` for
  hidden structured context. Desktop shows it as a removable composer chip. **This is how
  "select an agent, then ask ChatGPT" works.** Luna already sends bounded selected-run fields.
- `ui/message` with `_meta["openai/message"] {target: "active", send}`. Desktop supports
  `send:false`, which puts the text in the composer as an editable draft. Mobile only supports
  `send:true`. **Suggested asks should prefill the composer (`send:false`) on desktop,** so the
  user edits and sends. Recheck the gap-matrix claim that the SDK can't type `send:false`:
  Bits & Bolts calls `openai.message.send` with `send: !checked`.
- App-only tools (`_meta.ui.visibility: ["app"]`) let the view read state that the model doesn't
  see. There is **no server push**. Live updates mean polling an app-only read tool while the view
  is visible, which Luna already does with `refresh_factory` every 30s.
- View-registered tools: Bits & Bolts uses `app.registerTool("read_view" | "open_part" | ...)` so the
  model can read and drive the live view. **This is how "show me Worker 4" from chat can move the
  fullscreen view.** Requires new UI work, not new backend work.
- Composer @-mentions (Desktop only): an `openai/mentions.searchTool` returning ResourceLinks.
  Luna's `search_factory_mentions` already returns run and task links. Extend it to campaigns,
  issues and agents. `_meta["openai/extensions"]["mentions/search"]` is officially deprecated.
- Deep links: `{chatgpt}://plugins/{id}@{mkt}/app/{tool}?path=…`, delivered to the view as
  `hostContext["openai/deepLink"]`.
- OpenAI forms (`openai/elicitation`, MRTR, protocol 2026-07-28 or later; not on mobile) handle
  truly necessary structured input such as an authority boundary.
- Structured settings (`openai/settings {readTool, updateTool}`) can hold advanced defaults
  outside the main UI.

## Design rules adopted

- Use system background, fonts and neutrals. Brand color only on accents and the primary button.
  No gradients or decorative patterns. Monochrome outline icons. No logo in content, because
  ChatGPT adds it.
- WCAG AA contrast, layouts that survive text resizing, and state never shown by color alone.
- Inline: headline, then supporting text, then CTA. Don't duplicate ChatGPT's own features, and
  the model's reply shouldn't repeat what the card shows.
- Don't overload PiP with controls.
