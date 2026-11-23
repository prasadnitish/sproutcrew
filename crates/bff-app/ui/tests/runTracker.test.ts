import test from "node:test";
import assert from "node:assert/strict";
import { RunTracker } from "../src/runTracker.ts";

test("work is idle until something starts, and idle again when everything has returned", () => {
  const t = new RunTracker();
  assert.equal(t.idle, true);
  const a = t.start();
  const b = t.start();
  assert.equal(t.idle, false);
  assert.deepEqual(t.finish(a), { idle: false, released: false });
  assert.deepEqual(t.finish(b), { idle: true, released: false });
});

test("a stop is over only when all the work that was in flight at Stop has returned", () => {
  const t = new RunTracker();
  const run = t.start();
  assert.equal(t.stop(), true);
  assert.equal(t.stopping, true);
  // Pressing Stop again changes nothing.
  assert.equal(t.stop(), false);
  // A question asked after Stop is not part of the stopped run, and ending it does not end the stop.
  const ask = t.start();
  assert.deepEqual(t.finish(ask), { idle: false, released: false });
  assert.equal(t.stopping, true);
  assert.deepEqual(t.finish(run), { idle: true, released: true });
  assert.equal(t.stopping, false);
});

test("a stop with nothing in flight has nothing to wait for", () => {
  const t = new RunTracker();
  assert.equal(t.stop(), false);
  assert.equal(t.stopping, false);
});

test("a run can be stopped again after the first stop is over", () => {
  const t = new RunTracker();
  const first = t.start();
  t.stop();
  t.finish(first);
  const second = t.start();
  assert.equal(t.stop(), true);
  assert.deepEqual(t.finish(second), { idle: true, released: true });
});
