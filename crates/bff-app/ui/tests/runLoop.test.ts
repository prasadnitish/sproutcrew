import test from "node:test";
import assert from "node:assert/strict";
import { runSteps, type RunIO } from "../src/runLoop.ts";

/** A fake app: steps become ready as the ones they wait for are done, and every call is written down. */
function fake(opts: { waits?: Record<string, string[]>; questions?: boolean; failOn?: string; refuseBegin?: boolean; stopAfter?: number } = {}) {
  const waits = opts.waits ?? { a: [], b: ["a"], c: ["a"], d: ["a"], e: ["a"], f: ["b", "c", "d", "e"] };
  const done = new Set<string>();
  const log: string[] = [];
  let stop = false;
  let waves = 0;
  const io: RunIO = {
    async call<T>(cmd: string): Promise<T> {
      log.push(cmd);
      if (cmd === "begin_run" && opts.refuseBegin) throw "A run is already going.";
      if (cmd === "ready_stages") return Object.keys(waits).filter((id) => !done.has(id) && waits[id].every((w) => done.has(w))) as T;
      if (cmd === "fact_questions") return (opts.questions ? [{ id: "q" }] : []) as T;
      return undefined as T;
    },
    async run(cmd: string, args: Record<string, unknown>) {
      const ids = cmd === "run_stage" ? [args.stageId as string] : (args.stageIds as string[]);
      log.push(`${cmd}:${ids.join(",")}`);
      if (opts.failOn && ids.includes(opts.failOn)) throw `${opts.failOn} did not finish`;
      ids.forEach((id) => done.add(id));
    },
    async afterWave() { waves++; if (opts.stopAfter && waves >= opts.stopAfter) stop = true; },
    stopped: () => stop,
    resetStop: () => { stop = false; },
  };
  return { io, log, done };
}

test("ready steps run together, three at a time, until none is left", async () => {
  const f = fake();
  assert.equal(await runSteps(f.io, "live"), "finished");
  const runs = f.log.filter((l) => l.startsWith("run_"));
  assert.deepEqual(runs, ["run_stage:a", "run_wave:b,c,d", "run_stage:e", "run_stage:f"]);
  assert.equal(f.log[0], "begin_run");
  assert.ok(f.log.includes("end_run"));
  assert.ok(f.log.includes("notify_run"));
});

test("Stop ends the run after the step in flight, and nothing new starts", async () => {
  const f = fake({ stopAfter: 1 });
  assert.equal(await runSteps(f.io, "live"), "stopped");
  assert.deepEqual(f.log.filter((l) => l.startsWith("run_")), ["run_stage:a"]);
  assert.ok(f.log.includes("end_run"));
});

test("a run held for answers is not finished, and sends no notification", async () => {
  const f = fake({ waits: { a: [] }, questions: true });
  assert.equal(await runSteps(f.io, "live"), "questions");
  assert.ok(f.log.includes("end_run"));
  assert.ok(!f.log.includes("notify_run"));
});

test("a failed step ends the run, lets the hold go and passes the reason on", async () => {
  const f = fake({ waits: { a: [] }, failOn: "a" });
  await assert.rejects(runSteps(f.io, "live"), /a did not finish/);
  assert.ok(f.log.includes("end_run"));
  assert.ok(f.log.includes("notify_run"));
});

test("a second run is refused and does not let go of the first one's hold", async () => {
  const f = fake({ refuseBegin: true });
  await assert.rejects(runSteps(f.io, "live"), /already going/);
  assert.deepEqual(f.log, ["begin_run"]);
});

test("a step is tried once per run, so one that keeps coming back ready is not run in a loop", async () => {
  const f = fake({ waits: { a: [] } });
  // The fake marks nothing done for this step, so it would be ready forever.
  f.io.run = async (cmd, args) => { f.log.push(`${cmd}:${args.stageId}`); };
  assert.equal(await runSteps(f.io, "live"), "finished");
  assert.deepEqual(f.log.filter((l) => l.startsWith("run_")), ["run_stage:a"]);
});
