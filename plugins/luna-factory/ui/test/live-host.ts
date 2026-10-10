// Real-server host: the production App talks to a disposable luna-factoryd through a read-only proxy.
import { AppBridge, PostMessageTransport } from "@modelcontextprotocol/ext-apps/app-bridge";

declare global { interface Window { lunaRpc(name: string, args: Record<string, unknown>): Promise<Record<string, unknown>>; lunaLiveHost: { calls: string[]; contexts: unknown[]; messages: unknown[]; initialized: boolean } } }
const params = new URLSearchParams(location.search);
const element = document.getElementById("factory");
if (!(element instanceof HTMLIFrameElement) || !element.contentWindow) throw new Error("Missing host iframe");
const iframe = element;
const surface = params.get("surface") ?? "global";
const runId = params.get("run") ?? "";
const live = { calls: [] as string[], contexts: [] as unknown[], messages: [] as unknown[], initialized: false };
window.lunaLiveHost = live;
const entry = surface === "thread" ? "open_factory_panel" : surface === "inline" ? "get_factory_run" : "open_factory";
const host = new AppBridge(null, { name: "Live-server basic host", version: "0.2.1" }, {
  serverTools: {}, updateModelContext: { structuredContent: {}, text: {} }, message: { text: {} },
  experimental: { "openai/modelContext": {}, "openai/message": {}, "openai/deepLink": {} },
}, { hostContext: { theme: params.get("theme") === "dark" ? "dark" : "light", platform: params.get("platform") === "mobile" ? "mobile" : "desktop", displayMode: params.get("mode") === "fullscreen" ? "fullscreen" : "inline", availableDisplayModes: ["inline", "fullscreen"], toolInfo: { tool: { name: entry, inputSchema: { type: "object" } } } } });
host.oncalltool = async request => { live.calls.push(request.name); return await window.lunaRpc(request.name, request.arguments ?? {}) as never; };
host.onupdatemodelcontext = async request => { live.contexts.push(request); return { _meta: { "openai/modelContext": { updateId: `live-${live.contexts.length}` } } }; };
host.onmessage = async request => { live.messages.push(request); return {}; };
host.onrequestdisplaymode = async ({ mode }) => { host.setHostContext({ displayMode: mode }); return { mode }; };
host.oninitialized = async () => {
  const opening = surface === "inline" ? await window.lunaRpc("get_factory_run", { run_id: runId }) : await window.lunaRpc(entry === "open_factory_panel" ? "open_factory_panel" : "open_factory", {});
  await host.sendToolInput({ arguments: surface === "inline" ? { run_id: runId } : {} });
  await host.sendToolResult(opening as never);
  const node = params.get("node");
  const revision = params.get("revision");
  if (surface !== "inline" && runId) host.setHostContext({ "openai/deepLink": { url: node && revision ? `/runs/${runId}?task=${node}&revision=${revision}` : `/runs/${runId}` } });
  live.initialized = true;
};
const view = iframe.contentWindow;
if (!view) throw new Error("Missing iframe window");
await host.connect(new PostMessageTransport(view, view));
iframe.src = "/index.html";
