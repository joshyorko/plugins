// Tools the host model can call on this view. They read or move the view only: no server mutation,
// no execution, no proof. Raw thread identities never leave the view.
import { z } from "zod";
import { roster } from "./agents";
import { layoutWaves } from "./campaign-map";
import type { WorkbenchController } from "./controller";
import { runId } from "./domain";
import { taskTone, taskToneLabel } from "./lunar";
import { statusLabel } from "./narrative";
import { missionTasks } from "./graph-view";

const clip = (value: string, length: number) => value.length > length ? `${value.slice(0, length - 1)}…` : value;

/** A bounded description of what the user is looking at right now. */
export function describeView(controller: WorkbenchController, surface: string) {
  const { state } = controller;
  const run = controller.selected;
  if (!run) {
    return {
      surface, view: "home", connected: state.connected,
      runs: state.runs.slice(0, 20).map(item => ({ run_id: item.id, repository: item.repository, objective: clip(item.objective, 160), status: statusLabel(item) })),
    };
  }
  const { tasks, graph } = missionTasks(state, run);
  const layout = layoutWaves(tasks);
  const agents = roster(run, graph);
  const byId = new Map(tasks.map(task => [task.id, task]));
  const node = tasks.find(task => task.id === state.selectedNodeId);
  const agent = agents.find(item => item.thread === state.selectedAgent);
  const counts = run.control ? run.presentation?.criteria : undefined;
  return {
    surface, view: state.viewMode, connected: state.connected, stale: state.graphStale,
    run: { run_id: run.id, repository: run.repository, objective: clip(run.objective, 300), status: statusLabel(run), proof: counts ? `${counts.proven} of ${counts.mandatory} criteria proven` : "unverified", revision: run.control?.revision ?? null },
    plan: { tasks: tasks.length, waves: layout.waves.length, prerequisites_met: layout.prerequisitesMet.size },
    agents: agents.map(item => ({ label: item.label, role: item.role, liveness: item.liveness, task_id: item.taskId })),
    ...(node ? { task: {
      task_id: node.id, title: clip(node.title, 300), state: taskToneLabel[taskTone(node.state)], wave: (layout.depth.get(node.id) ?? 0) + 1,
      waits_on: node.dependencies.map(id => clip(byId.get(id)?.title ?? id, 120)),
      unblocks: (layout.dependents.get(node.id) ?? []).map(id => clip(byId.get(id)?.title ?? id, 120)),
    } } : {}),
    ...(agent ? { agent: { label: agent.label, role: agent.role, liveness: agent.liveness, task_id: agent.taskId } } : {}),
  };
}

type ToolResult = { content: Array<{ type: "text"; text: string }>; structuredContent: Record<string, unknown>; isError?: boolean };
const ok = (value: Record<string, unknown>): ToolResult => ({ content: [{ type: "text", text: JSON.stringify(value) }], structuredContent: value });
const fail = (message: string, value: Record<string, unknown>): ToolResult => ({ content: [{ type: "text", text: message }], structuredContent: { error: message, ...value }, isError: true });

export const viewToolSchemas = {
  focusTask: z.object({ task_id: z.string().min(1).max(256) }),
  focusAgent: z.object({ label: z.string().min(1).max(40) }),
  showView: z.object({ mode: z.enum(["map", "lanes"]) }),
  openRun: z.object({ run_id: z.string().min(1).max(64) }),
};

export interface ViewToolHost { surface(): string; readPlan(): void }
/** Handlers kept free of the App so they can be tested against a controller directly. */
export function viewToolHandlers(controller: WorkbenchController, host: ViewToolHost) {
  const view = () => describeView(controller, host.surface());
  return {
    read: async (): Promise<ToolResult> => ok(view()),
    focusTask: async ({ task_id }: z.infer<typeof viewToolSchemas.focusTask>): Promise<ToolResult> => {
      if (!controller.state.graph || controller.state.graph.run_id !== controller.state.selectedId) return fail("Open a run with a loaded plan first.", view());
      if (!controller.state.graph.nodes.some(node => node.id === task_id)) return fail("That task is not in the open plan.", view());
      controller.selectNode(task_id, false);
      return ok(view());
    },
    focusAgent: async ({ label }: z.infer<typeof viewToolSchemas.focusAgent>): Promise<ToolResult> => {
      const agent = roster(controller.selected, controller.state.graph).find(item => item.label.toLowerCase() === label.trim().toLowerCase());
      if (!agent) return fail("No reported agent has that label.", view());
      controller.selectAgent(agent.thread, false);
      return ok(view());
    },
    showView: async ({ mode }: z.infer<typeof viewToolSchemas.showView>): Promise<ToolResult> => { controller.setViewMode(mode); return ok(view()); },
    openRun: async ({ run_id }: z.infer<typeof viewToolSchemas.openRun>): Promise<ToolResult> => {
      if (!runId.safeParse(run_id).success || !controller.state.runs.some(run => run.id === run_id)) return fail("That run is not in the current list.", view());
      await controller.select(run_id, false);
      host.readPlan();
      return ok(view());
    },
  };
}

/** Registers the view tools on an MCP App. Call before connecting. */
export function registerViewTools(app: { registerTool: (name: string, config: Record<string, unknown>, cb: (args: never) => Promise<ToolResult>) => unknown }, controller: WorkbenchController, host: ViewToolHost): void {
  const handlers = viewToolHandlers(controller, host);
  const annotations = { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false };
  app.registerTool("luna_read_view", { title: "Read the Luna Factory view", description: "Describe what the Luna Factory view currently shows: the open run, its plan waves, the selected task and agent. Read-only; does not contact the server.", annotations }, handlers.read as never);
  app.registerTool("luna_focus_task", { title: "Focus a task", description: "Select a task in the open Campaign Map by task_id so the user sees it in the inspector. View only; never changes the plan or starts work.", inputSchema: viewToolSchemas.focusTask, annotations }, handlers.focusTask as never);
  app.registerTool("luna_focus_agent", { title: "Focus an agent", description: "Select a reported agent by its label (for example \"Luna\" or \"Worker 2\") and its task. View only.", inputSchema: viewToolSchemas.focusAgent, annotations }, handlers.focusAgent as never);
  app.registerTool("luna_show_view", { title: "Switch Map or Lanes", description: "Switch the campaign view between the dependency Map and the agent Lanes. View only.", inputSchema: viewToolSchemas.showView, annotations }, handlers.showView as never);
  app.registerTool("luna_open_run", { title: "Open a run", description: "Open a run from the current list in the Luna Factory view and read its plan. View only.", inputSchema: viewToolSchemas.openRun, annotations }, handlers.openRun as never);
}
