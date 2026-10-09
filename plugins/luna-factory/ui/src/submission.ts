import type { WorkbenchController } from "./controller";

/** Implicit form submission can omit submitter; execution requires the named action. */
export function submitNewRun(
  controller: Pick<WorkbenchController, "createGraph" | "mutate">,
  intent: string | undefined,
  args: Record<string, unknown>,
): Promise<boolean> {
  return intent === "start" ? controller.mutate("start_factory", args) : controller.createGraph(args);
}
