import test from "node:test";
import assert from "node:assert/strict";
import { costPart, idleLine, nextAction, noteLine, runningLine, spentSince, statusLine, stoppedLine, workingLine } from "../src/status.ts";
import type { UsageRow } from "../src/usage.ts";

const row = (o: Partial<UsageRow>): UsageRow => ({ at: 100, task: "t1", stage: "draft", role: "writer", route: "R12", model: "m", tokens: 10, dollars: 0.01, dollars_status: "reported", ...o });
const stages = [
  { id: "frame", title: "Framing the question" },
  { id: "draft", title: "Writing the first draft" },
  { id: "check", title: "Checking the draft" },
  { id: "polish", title: "Polishing" },
  { id: "final", title: "Final files" },
];

test("the status line names the step by its title, its place in the plan and the cost so far", () => {
  assert.equal(statusLine(stages[1], 1, 5, 0.01), "Writing the first draft · step 2 of 5 · 1¢ so far");
});

test("no cost leaves the cost part out, and an unknown cost says so", () => {
  assert.equal(statusLine(stages[0], 0, 5, 0), "Framing the question · step 1 of 5");
  assert.equal(statusLine(stages[0], 0, 5, null), "Framing the question · step 1 of 5 · cost unknown so far");
});

test("a small cost is rounded to cents, a tiny one is not shown as zero, and a dollar or more is shown in dollars", () => {
  assert.equal(costPart(0.0004), "under 1¢ so far");
  assert.equal(costPart(0.034), "3¢ so far");
  assert.equal(costPart(0.994), "99¢ so far");
  assert.equal(costPart(1.2), "$1.20 so far");
  assert.equal(costPart(0), "");
});

test("steps that run together are named together", () => {
  assert.equal(runningLine([stages[2], stages[3]], [2, 3], 5, 0.02), "Checking the draft and Polishing · steps 3 and 4 of 5 · 2¢ so far");
  assert.equal(runningLine([stages[1], stages[2], stages[3]], [1, 2, 3], 5, 0), "Writing the first draft, Checking the draft and Polishing · steps 2, 3 and 4 of 5");
});

test("the working line reads the running steps from the run's text and never shows an ID", () => {
  assert.equal(workingLine("stage draft on live", ["draft"], stages, 0.01), "Writing the first draft · step 2 of 5 · 1¢ so far");
  assert.equal(workingLine("stage check and stage polish on live", ["check", "polish"], stages, 0), "Checking the draft and Polishing · steps 3 and 4 of 5");
  assert.ok(!workingLine("stage draft on live", ["draft"], stages, null).includes("draft on"));
  // Work that is not one of the plan's steps is named plainly, without the route it runs on.
  assert.equal(workingLine("planning the steps on live", [], stages, 0), "Working: planning the steps");
  assert.equal(workingLine("your request for a change on live", [], stages, 0), "Working: your request for a change");
  // A step that is not in the plan is not shown as an ID.
  assert.equal(workingLine("stage gone on live", ["gone"], stages, 0), "Working");
});

test("a line that only repeats the start is not shown again, and a wait or a long run is", () => {
  assert.equal(noteLine("Started: Writing the first draft"), "");
  assert.equal(noteLine("Still working: Writing the first draft (1m 00s so far)"), "Still working: Writing the first draft (1m 00s so far)");
  assert.equal(noteLine("Server busy. Retrying in 4 seconds."), "Server busy. Retrying in 4 seconds.");
});

test("spend counts only the requests since the run started", () => {
  assert.equal(spentSince([row({ at: 50, dollars: 5 }), row({ at: 100, dollars: 0.01 }), row({ at: 120, dollars: 0.02 })], 100), 0.03);
  assert.equal(spentSince([row({ at: 50 })], 100), 0);
  assert.equal(spentSince(undefined, 100), 0);
});

test("a run whose requests never reported a price has an unknown cost, not zero", () => {
  assert.equal(spentSince([row({ dollars: null }), row({ dollars: null })], 0), null);
  // A price that is known for some requests is a floor, not nothing.
  assert.equal(spentSince([row({ dollars: 0.02 }), row({ dollars: null })], 0), 0.02);
});

test("the stopped line says after which step, and before any when none finished", () => {
  assert.equal(stoppedLine(3), "Stopped by you after step 3");
  assert.equal(stoppedLine(1), "Stopped by you after step 1");
  assert.equal(stoppedLine(0), "Stopped by you before any step finished");
});

test("an idle project says where the work stands", () => {
  const p = { steps: 5, done: 2, approved: true, failed: false, stopped: false };
  assert.equal(idleLine({ ...p, steps: 0, done: 0 }), "No plan yet");
  assert.equal(idleLine({ ...p, approved: false, done: 0 }), "The plan is waiting for your approval");
  assert.equal(idleLine(p), "2 of 5 steps done");
  assert.equal(idleLine({ ...p, failed: true }), "A step did not finish · 2 of 5 done");
  assert.equal(idleLine({ ...p, stopped: true }), "Stopped by you after step 2");
  assert.equal(idleLine({ ...p, done: 5 }), "All 5 steps are done");
  assert.equal(idleLine({ ...p, steps: 1, done: 1 }), "The step is done");
});

test("the next action is the plan until the work is done, then the results", () => {
  assert.deepEqual(nextAction({ steps: 0, done: 0, approved: false }), { label: "Suggest a plan", tab: "work" });
  assert.deepEqual(nextAction({ steps: 5, done: 0, approved: false }), { label: "Review the plan", tab: "work" });
  assert.deepEqual(nextAction({ steps: 5, done: 2, approved: true }), { label: "Open the plan", tab: "work" });
  assert.deepEqual(nextAction({ steps: 5, done: 5, approved: true }), { label: "View the results", tab: "share" });
});
