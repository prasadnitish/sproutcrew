import { test } from "node:test";
import assert from "node:assert/strict";
import { layout, wrap } from "../src/diagram.ts";

const stages = [
  { id: "a", title: "Frame the problem", side: "writing" as const },
  { id: "b", title: "Recompute every number", side: "checking" as const },
  { id: "c", title: "Write the recommendation", side: "writing" as const },
];

test("each step gets a column in order and a lane by its side", () => {
  const l = layout(stages);
  assert.deepEqual(l.nodes.map((n) => n.id), ["a", "b", "c"]);
  assert.ok(l.nodes[0].x < l.nodes[1].x && l.nodes[1].x < l.nodes[2].x);
  assert.equal(l.nodes[0].y, l.nodes[2].y, "the two writing steps share a lane");
  assert.ok(l.nodes[0].y < l.nodes[1].y, "writing is above checking");
});

test("an edge joins each step to the next, and there is one fewer edge than steps", () => {
  const l = layout(stages);
  assert.equal(l.edges.length, 2);
  assert.deepEqual(l.edges.map((e) => [e.from, e.to]), [["a", "b"], ["b", "c"]]);
  assert.equal(layout([]).edges.length, 0);
  assert.equal(layout([stages[0]]).edges.length, 0);
});

test("the canvas is wide enough for every node and tall enough for both lanes", () => {
  const l = layout(stages);
  assert.ok(l.width > l.nodes[2].x);
  assert.ok(l.height > l.nodes[1].y);
});

test("a long title is wrapped into at most three short lines and cut with an ellipsis", () => {
  assert.deepEqual(wrap("Frame the problem", 14), ["Frame the", "problem"]);
  const w = wrap("Remove the signs of machine writing from the document", 14);
  assert.ok(w.length <= 3 && w.every((x) => x.length <= 15), JSON.stringify(w));
  assert.ok(w[w.length - 1].endsWith("…"));
  assert.deepEqual(wrap("", 14), []);
});

const diamond = [
  { id: "spec", title: "Write the spec", side: "writing" as const },
  { id: "build", title: "Build it", side: "writing" as const },
  { id: "security", title: "Security check", side: "checking" as const, needs: ["build"] },
  { id: "ux", title: "UX check", side: "checking" as const, needs: ["build"] },
  { id: "fix", title: "Fix what they found", side: "writing" as const, needs: ["security", "ux"] },
];

test("steps that wait for the same step share a column and are stacked inside their lane", () => {
  const l = layout(diamond);
  const n = (id: string) => l.nodes.find((x) => x.id === id)!;
  assert.equal(n("security").x, n("ux").x, "side by side in time");
  assert.notEqual(n("security").y, n("ux").y, "stacked so they do not overlap");
  assert.ok(n("fix").x > n("security").x);
  assert.ok(n("build").x < n("security").x);
});

test("an edge joins each step to every step it waits for", () => {
  const l = layout(diamond);
  assert.deepEqual(
    l.edges.map((e) => `${e.from}>${e.to}`).sort(),
    ["build>security", "build>ux", "security>fix", "spec>build", "ux>fix"],
  );
});

test("a step with no list waits for the one before it, so a plain list is a chain", () => {
  const l = layout(stages);
  assert.deepEqual(l.edges.map((e) => `${e.from}>${e.to}`), ["a>b", "b>c"]);
});

test("the canvas grows to hold the stacked steps", () => {
  const flat = layout(stages);
  const tall = layout(diamond);
  assert.ok(tall.height > flat.height);
  assert.ok(tall.nodes.every((n) => n.y < tall.height - 40));
});

test("a step run again counts each result, and hovering says model, time and cost, never a zero for unknown", async () => {
  const { runsOf, detailLine } = await import("../src/diagram.ts");
  assert.equal(runsOf([{ name: "draft" }, { name: "edit" }, { name: "draft" }], "draft"), 2);
  assert.equal(runsOf([], "draft"), 0);
  assert.equal(detailLine("anthropic/claude-haiku-5.5", 75.9, 0.0021, true), "anthropic/claude-haiku-5.5 · 1 min 16 s · under $0.01");
  assert.equal(detailLine("m", 125, 0.3, true), "m · 2 min 5 s · $0.30");
  assert.equal(detailLine("m", null, null, false), "m · cost unknown");
});
