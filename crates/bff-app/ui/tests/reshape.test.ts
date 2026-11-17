import { test } from "node:test";
import assert from "node:assert/strict";
import { reshape, stepId } from "../src/graph.ts";

const chain = [
  { id: "frame", title: "Frame", owner_role: "analyst" },
  { id: "analyse", title: "Analyse", owner_role: "analyst" },
  { id: "recommend", title: "Recommend", owner_role: "strategist" },
  { id: "deck", title: "Deck", owner_role: "presenter" },
  { id: "final", title: "Final", owner_role: "reviewer" },
];
const after = (s: { after?: string[] }[], i: number) => s[i].after;

test("nothing off and nothing added gives the same steps back", () => {
  assert.equal(reshape(chain, [], []), chain);
});

test("leaving out a step joins its neighbours, so nothing waits for a step that is gone", () => {
  const out = reshape(chain, ["deck"], []);
  assert.deepEqual(out.map((s) => s.id), ["frame", "analyse", "recommend", "final"]);
  assert.deepEqual(after(out, 3), ["recommend"]);
});

test("leaving out two in a row still joins the ends", () => {
  const out = reshape(chain, ["analyse", "recommend"], []);
  assert.deepEqual(out.map((s) => s.id), ["frame", "deck", "final"]);
  assert.deepEqual(after(out, 1), ["frame"]);
});

test("leaving out the first step makes the next one a start", () => {
  const out = reshape(chain, ["frame"], []);
  assert.deepEqual(after(out, 0), []);
});

test("a step that waits for several keeps the ones still there", () => {
  const dag = [
    { id: "build", title: "b", owner_role: "x" },
    { id: "sec", title: "s", owner_role: "x", after: ["build"] },
    { id: "ux", title: "u", owner_role: "x", after: ["build"] },
    { id: "fix", title: "f", owner_role: "x", after: ["sec", "ux"] },
  ];
  assert.deepEqual(after(reshape(dag, ["ux"], []), 2), ["sec"]);
  assert.deepEqual(after(reshape(dag, ["sec", "ux"], []), 1), ["build"]);
});

test("an added step goes right after the step it waits for, and later steps do not wait for it", () => {
  const out = reshape(chain, [], [{ id: "prototype", title: "Prototype", owner_role: "builder", guide: "Build it.", after: "recommend" }]);
  assert.deepEqual(out.map((s) => s.id), ["frame", "analyse", "recommend", "prototype", "deck", "final"]);
  assert.deepEqual(after(out, 3), ["recommend"]);
  assert.deepEqual(after(out, 4), ["recommend"], "the deck still waits for the recommendation");
});

test("a step added after a step that was left out waits for what that one waited for", () => {
  const out = reshape(chain, ["deck"], [{ id: "p", title: "P", owner_role: "builder", guide: "g", after: "deck" }]);
  assert.deepEqual(after(out, out.findIndex((s) => s.id === "p")), ["recommend"]);
});

test("a new step's id comes from its title and is never one already taken", () => {
  assert.equal(stepId("Build a small prototype!", []), "build_a_small_prototype");
  assert.equal(stepId("Deck", ["deck"]), "deck_2");
  assert.equal(stepId("???", []), "step");
});
