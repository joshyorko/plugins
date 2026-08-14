import test from "node:test";
import assert from "node:assert/strict";

import {
  classifyOutcome,
  diffPlanGenerations,
  normalizeControlIntent,
} from "../mcp/domain.mjs";

test("classifies an equivalent existing outcome for reuse", () => {
  assert.equal(
    classifyOutcome({ goal: "ship the dashboard" }, [
      { title: "Ship dashboard", status: "OPEN", body: "The dashboard is tracked here." },
    ]).classification,
    "REUSE_EXISTING",
  );
});

test("invalidates approval when the observed head changes", () => {
  const diff = diffPlanGenerations(
    { generation: "G", observedRepositoryHead: "H", outcomes: [] },
    { generation: "G2", observedRepositoryHead: "H2", outcomes: [] },
  );
  assert.equal(diff.approvalInvalidated, true);
  assert.match(diff.reason, /head/i);
});

test("normalizes every operator surface to one read-only control intent", () => {
  const intent = normalizeControlIntent({
    action: "DRAIN",
    target: { repository: "owner/repo", targetBranch: "main", observedHead: "abc" },
    orchestrationPreset: { executor: "Codex CLI", model: "gpt-5.6-luna", effort: "xhigh" },
  });
  assert.deepEqual(intent, {
    action: "DRAIN",
    target: { repository: "owner/repo", targetBranch: "main", observedHead: "abc" },
    planGeneration: null,
    orchestrationPreset: { executor: "Codex CLI", model: "gpt-5.6-luna", effort: "xhigh" },
    constraints: {},
  });
});
