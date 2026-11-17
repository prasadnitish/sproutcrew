import { test } from "node:test";
import assert from "node:assert/strict";
import { fuzzy } from "../src/fuzzy.ts";

test("an empty search matches everything, and a missing letter matches nothing", () => {
  assert.equal(fuzzy("", "Work"), 0);
  assert.equal(fuzzy("  ", "Work"), 0);
  assert.equal(fuzzy("wz", "Work"), null);
});

test("letters in order match, and a run at the start of a word scores higher", () => {
  assert.notEqual(fuzzy("wrk", "Work of mock-1"), null);
  assert.ok((fuzzy("wor", "Work")!) > (fuzzy("wor", "A sworn tour")!));
  assert.equal(fuzzy("WORK", "work"), fuzzy("work", "WORK"));
});
