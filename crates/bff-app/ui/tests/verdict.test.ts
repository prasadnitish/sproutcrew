import test from "node:test";
import assert from "node:assert/strict";
import { asks, firstAndRest, headline, resultLine, ticked, type Check } from "../src/verdict.ts";

const fix = (n: number): Check => ({ verdict: "fix_first", flagged: Array.from({ length: n }, (_, i) => `problem ${i + 1}`) });

test("the headline says the verdict and how many things the check listed", () => {
  assert.equal(headline(fix(3)), "Fix first, 3 things");
  assert.equal(headline(fix(1)), "Fix first, 1 thing");
  assert.equal(headline(fix(0)), "Fix first");
  assert.equal(headline({ verdict: "ready", flagged: [] }), "Ready");
  assert.equal(headline({ verdict: "do_not_send", flagged: ["x", "y"] }), "Do not send, 2 things");
  assert.equal(headline({ verdict: null, flagged: [] }), "No verdict");
});

test("a ready check lists nothing, even when its text had dash lines", () => {
  assert.equal(headline({ verdict: "ready", flagged: ["what it checked"] }), "Ready");
});

test("the green tick is for a step with no verdict to give and for a check that says ready", () => {
  assert.equal(ticked(undefined), true);
  assert.equal(ticked({ verdict: "ready", flagged: [] }), true);
  assert.equal(ticked(fix(2)), false);
  assert.equal(ticked({ verdict: "do_not_send", flagged: [] }), false);
  assert.equal(ticked({ verdict: null, flagged: [] }), false, "no verdict is not a pass");
});

test("the result says what became of the check", () => {
  assert.deepEqual(resultLine(fix(3), true), { tone: "warn", text: "Second check: Fix first · 3 notes sent to the final step" });
  assert.deepEqual(resultLine(fix(1), true), { tone: "warn", text: "Second check: Fix first · 1 note sent to the final step" });
  assert.deepEqual(resultLine(fix(0), true), { tone: "warn", text: "Second check: Fix first · notes sent to the final step" });
  assert.equal(resultLine(fix(3), false), null, "nothing was applied yet");
  assert.deepEqual(resultLine({ verdict: "do_not_send", flagged: [] }, false), { tone: "stop", text: "Not ready: the second check said do not send", hint: "Run the second check again, or use Ask for a change to fix the draft." });
  assert.deepEqual(resultLine({ verdict: null, flagged: [] }, true), { tone: "warn", text: "Second check: no verdict, so the final version was not checked. Read it before you send it" });
  assert.equal(resultLine({ verdict: "ready", flagged: [] }, true), null);
});

test("a line that says no fix is needed is not counted as a note or a thing to fix", () => {
  const c: Check = { verdict: "fix_first", flagged: ["The date is wrong. Fix: use 14 January.", "The title is fine. No fix needed.", "The totals add up (NO FIX NEEDED)", "The tone is off. Fix: plainer words."] };
  assert.deepEqual(asks(c), ["The date is wrong. Fix: use 14 January.", "The tone is off. Fix: plainer words."]);
  assert.equal(headline(c), "Fix first, 2 things");
  assert.deepEqual(resultLine(c, true), { tone: "warn", text: "Second check: Fix first · 2 notes sent to the final step" });
  // The live run's second check listed eleven lines and most were "no fix needed".
  const eleven: Check = { verdict: "fix_first", flagged: [...Array.from({ length: 8 }, (_, i) => `Claim ${i + 1} is supported. No fix needed.`), "Add the date.", "Name the owner.", "Cut the last line."] };
  assert.deepEqual(resultLine(eleven, true), { tone: "warn", text: "Second check: Fix first · 3 notes sent to the final step" });
  assert.equal(headline({ verdict: "fix_first", flagged: ["Fine. No fix needed."] }), "Fix first");
});

test("a long list shows its first three lines and keeps the rest, all of it still there", () => {
  const lines = Array.from({ length: 18 }, (_, i) => `claim ${i + 1}`);
  const { shown, rest } = firstAndRest(lines);
  assert.deepEqual(shown, ["claim 1", "claim 2", "claim 3"]);
  assert.equal(rest.length, 15);
  assert.deepEqual([...shown, ...rest], lines);
  assert.deepEqual(firstAndRest(["a", "b", "c"]), { shown: ["a", "b", "c"], rest: [] });
  assert.deepEqual(firstAndRest([]), { shown: [], rest: [] });
  assert.deepEqual(firstAndRest(lines, lines.length).rest, [], "a list shown whole has no rest");
});
