// Agent roster derived from the server's owner/worker projection. Identities are never invented.
import type { FactoryGraph, RunView } from "./domain";
import type { Liveness } from "./lunar";

export interface Agent {
  thread: string;
  role: "coordinator" | "worker";
  /** Stable within one projection: the server's worker order. */
  ordinal: number;
  label: string;
  liveness: Liveness;
  /** The task whose persisted owner_thread is this agent, if any. */
  taskId: string | null;
}

export function roster(run: RunView | undefined, graph: FactoryGraph | null): Agent[] {
  const presentation = run?.control ? run.presentation : undefined;
  if (!run || !presentation) return [];
  const tasks: Array<{ id: string; owner_thread: string | null }> = graph?.run_id === run.id ? graph.nodes : run.control?.tasks ?? [];
  const boundTask = (thread: string) => tasks.find(task => task.owner_thread === thread)?.id ?? null;
  const agents: Agent[] = [];
  const owner = presentation.owner.thread_id;
  if (owner) agents.push({ thread: owner, role: "coordinator", ordinal: 0, label: "Luna", liveness: presentation.owner.liveness, taskId: boundTask(owner) });
  let ordinal = 0;
  for (const worker of presentation.workers) {
    if (worker.thread_id === owner || agents.some(agent => agent.thread === worker.thread_id)) continue;
    ordinal += 1;
    agents.push({ thread: worker.thread_id, role: "worker", ordinal, label: `Worker ${ordinal}`, liveness: worker.liveness, taskId: boundTask(worker.thread_id) });
  }
  return agents;
}

export const livenessLabel: Record<Liveness, string> = { active: "Active", idle: "Idle", unknown: "Liveness unknown" };
