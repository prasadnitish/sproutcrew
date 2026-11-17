import { test } from "node:test";
import assert from "node:assert/strict";
import { checksOf, modelOf, readySteps, runningIds, sameProviderCheck } from "../src/graph.ts";

const plan = [
  { id: "a" },
  { id: "b", after: ["a"] },
  { id: "c", after: ["a"] },
  { id: "d", after: ["b", "c"] },
  { id: "e" },
];

test("the first step is ready at the start, and a step with no list follows the one before it", () => {
  assert.deepEqual(readySteps(plan, []), ["a"]);
  assert.deepEqual(readySteps(plan, ["a", "b", "c", "d"]), ["e"]);
});

test("steps that wait for the same step are ready together, and a step waits for all it names", () => {
  assert.deepEqual(readySteps(plan, ["a"]), ["b", "c"]);
  assert.deepEqual(readySteps(plan, ["a", "b"]), ["c"]);
  assert.deepEqual(readySteps(plan, ["a", "b", "c"]), ["d"]);
});

test("nothing is ready when everything is done", () => {
  assert.deepEqual(readySteps(plan, ["a", "b", "c", "d", "e"]), []);
});

test("every step named as running is found, one or several", () => {
  assert.deepEqual(runningIds("stage b and stage c on live"), ["b", "c"]);
  assert.deepEqual(runningIds("stage deck on replay"), ["deck"]);
  assert.deepEqual(runningIds(""), []);
  assert.deepEqual(runningIds("a question about final on live"), []);
});

const team = [
  { role: "writer", route: "R2", model: "claude-opus-5-5", effort: "high" },
  { role: "checker", route: "R1", model: "gpt-6.1-sol", effort: "high" },
];
const flow = [
  { id: "draft", owner_role: "writer" },
  { id: "polish", owner_role: "writer" },
  { id: "numbers", owner_role: "checker" },
  { id: "final", owner_role: "checker", after: ["numbers"] },
];
const checking = (role: string) => role === "checker";

test("a step uses its own model when it has one, and its role's when it does not", () => {
  assert.equal(modelOf(team, flow[0])?.model, "claude-opus-5-5");
  const own = { ...flow[0], uses: { route: "R2", model: "claude-sonnet-5-5", effort: "low" } };
  assert.deepEqual(modelOf(team, own), { role: "writer", route: "R2", model: "claude-sonnet-5-5", effort: "low" });
  assert.equal(modelOf(team, { id: "x", owner_role: "nobody" }), undefined);
});

test("the checks on a step are the later checking steps that read it, directly or through others", () => {
  assert.deepEqual(checksOf(flow, "draft", checking).map((s) => s.id), ["numbers", "final"]);
  assert.deepEqual(checksOf(flow, "final", checking), []);
  const apart = [{ id: "a", owner_role: "writer" }, { id: "b", owner_role: "writer", after: ["a"] }, { id: "c", owner_role: "checker", after: ["a"] }];
  assert.deepEqual(checksOf(apart, "b", checking), [], "a check that does not read a step is not a check on it");
});

test("a check on the same provider as all the work it reads is flagged, and a mix is not", () => {
  assert.equal(sameProviderCheck(team, flow, flow[2], checking), false, "Codex checking Claude");
  assert.equal(sameProviderCheck(team, flow, flow[0], checking), false, "a writing step is never flagged");
  const claudeCheck = { ...flow[2], uses: { route: "R2", model: "claude-sonnet-5-5", effort: "high" } };
  assert.equal(sameProviderCheck(team, [flow[0], flow[1], claudeCheck], claudeCheck, checking), true);
  const mixed = [{ ...flow[0], uses: { route: "R1", model: "gpt-6.1-sol", effort: "high" } }, flow[1], claudeCheck];
  assert.equal(sameProviderCheck(team, mixed, claudeCheck, checking), false, "one step on another provider still gets a real second look");
});
