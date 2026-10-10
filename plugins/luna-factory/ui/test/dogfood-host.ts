import { AppBridge, PostMessageTransport } from "@modelcontextprotocol/ext-apps/app-bridge";
import { McpUiHostStylesSchema } from "@modelcontextprotocol/ext-apps";
import { fixtureBackends, fixtureGraph, fixturePlanningRun, fixtureRun, fixtureWorkbench } from "./fixtures";

const params = new URLSearchParams(location.search);
const element = document.getElementById("factory");
if (!(element instanceof HTMLIFrameElement) || !element.contentWindow) throw new Error("Missing simulator iframe");
const iframe = element;
const scenario = params.get("scenario") ?? "planning";
const surface = params.get("surface") ?? "global";
const theme = params.get("theme") === "dark" ? "dark" : "light";
const platform = params.get("platform") === "mobile" ? "mobile" : "desktop";
const largeFontStyles = McpUiHostStylesSchema.parse({ variables: { "--font-text-md-size": "20px", "--font-text-sm-size": "18px" } });
const run = scenario === "decision" ? fixtureRun({ state: "NEEDS_INPUT", pending_decision: { id: "fixture-decision", question: "Which approved scope should the owner use?" }, active_workers: 0 })
  : scenario === "blocked" ? fixtureRun({ state: "BLOCKED", pending_decision: null, active_workers: 0, blocker: "Native ownership is unverified. Keep the claim held." }) : fixturePlanningRun();
const graph = fixtureGraph(run.control?.revision ?? 6);
graph.graph.run_id = run.id;
graph.graph.criteria = run.control?.criteria ?? [];
graph.graph.nodes = ["objective", "task-a", "task-b"].map(id => ({ ...graph.graph.nodes[0]!, id, title: id === "objective" ? run.objective : `Candidate ${id}`, criterion_ids: graph.graph.criteria.map(criterion => criterion.id), dependencies: id === "task-b" ? ["task-a"] : [] }));
graph.graph.planning_only = run.planning_only === true;
graph.graph.claim = run.presentation?.claim ?? graph.graph.claim;
if (params.get("proposal") === "1") graph.proposal = { id: "fixture-change", base_revision: graph.graph.revision - 1, actor: "local_operator", idempotency_key: "fixture-key", fingerprint: "fixture-hash", subject: run.current_subject, status: "proposed", applied_revision: null, change: { kind: "set_dependencies", node_id: "task-b", dependencies: [] } };
const workbench = { ...fixtureWorkbench, runs: [run], selected_run: run, capabilities: { ...fixtureWorkbench.capabilities, execution: { eligible: false, reason: "qualification_unverified" } } };
interface SimulatorInstrumentation { calls: string[]; contexts: unknown[]; messages: unknown[]; sizes: unknown[]; initialized: boolean; disconnect(): Promise<void>; }
const instrumentation: SimulatorInstrumentation = { calls: [], contexts: [], messages: [], sizes: [], initialized: false, disconnect: async () => { await host.teardownResource({}); await host.close(); } };
declare global { interface Window { lunaDogfoodHost: SimulatorInstrumentation; } }
window.lunaDogfoodHost = instrumentation;
const host = new AppBridge(null, { name: "Synthetic basic host", version: "0.2.1" }, {
  serverTools: {}, updateModelContext: { structuredContent: {}, text: {} }, message: { text: {} },
  experimental: { "openai/modelContext": {}, "openai/message": {}, "openai/deepLink": {} },
}, { hostContext: { theme, platform, displayMode: params.get("mode") === "fullscreen" ? "fullscreen" : "inline", availableDisplayModes: ["inline", "fullscreen"], ...(params.get("font") === "large" ? { styles: largeFontStyles } : {}), toolInfo: { tool: { name: surface === "thread" ? "open_factory_panel" : surface === "inline" ? "get_factory_run" : "open_factory", inputSchema: { type: "object" } } } } });
host.oncalltool = async request => {
  instrumentation.calls.push(request.name);
  if (scenario === "error") return { isError: true, content: [{ type: "text", text: "Fixture read failed. Reconnect to read current state." }] };
  if (request.name === "get_factory_backends") return { content: [], structuredContent: fixtureBackends };
  if (request.name === "get_factory_graph") return { content: [], structuredContent: graph };
  if (request.name === "get_factory_run") return { content: [], structuredContent: run };
  if (["refresh_factory", "open_factory"].includes(request.name)) return { content: [], structuredContent: workbench };
  return { isError: true, content: [{ type: "text", text: "The synthetic visual host is read-only; no mutation was sent." }] };
};
host.onupdatemodelcontext = async request => { instrumentation.contexts.push(request); return { _meta: { "openai/modelContext": { updateId: `fixture-${instrumentation.contexts.length}` } } }; };
host.onmessage = async request => { instrumentation.messages.push(request); return {}; };
host.onsizechange = size => { instrumentation.sizes.push(size); };
host.onrequestdisplaymode = async ({ mode }) => { host.setHostContext({ displayMode: mode }); return { mode }; };
host.oninitialized = async () => {
  await host.sendToolInput({ arguments: { run_id: run.id } });
  await host.sendToolResult(scenario === "error" ? { isError: true, content: [{ type: "text", text: "Fixture read failed. Reconnect to read current state." }] } : { content: [], structuredContent: workbench });
  if (params.get("graph") === "1" && scenario !== "error") host.setHostContext({ "openai/deepLink": { url: `/runs/${run.id}?task=task-b&revision=${graph.graph.revision}` } });
  instrumentation.initialized = true;
};
const view = iframe.contentWindow;
if (!view) throw new Error("Missing iframe window");
await host.connect(new PostMessageTransport(view, view));
iframe.src = "/index.html";
