import test from "node:test";
import assert from "node:assert/strict";
import { failedRun } from "../src/runs.ts";

test("a wave with a failed row reports each failed step and a clean wave reports nothing", () => {
  assert.equal(failedRun("run_wave", [{ stage: "a", ok: true, note: "" }, { stage: "b", ok: false, note: "paused: stopped on request" }]), "b: paused: stopped on request");
  assert.equal(failedRun("run_wave", [{ stage: "a", ok: true, note: "" }]), "");
  assert.equal(failedRun("run_stage", "done"), "");
});
