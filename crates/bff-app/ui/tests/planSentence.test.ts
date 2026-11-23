import test from "node:test";
import assert from "node:assert/strict";
import { planSentence } from "../src/planSentence.ts";

const writing = [
  { id: "facts", title: "Check what's missing", owner_role: "writer" },
  { id: "outline", title: "Shape the piece", owner_role: "writer" },
  { id: "draft", title: "Write the first draft", owner_role: "writer" },
  { id: "edit", title: "Edit for voice and clarity", owner_role: "editor" },
  { id: "review", title: "Check the writing and its claims", owner_role: "reviewer" },
  { id: "finish", title: "Write the finished piece", owner_role: "writer" },
];

test("the writing plan names the writer, the editor and a second check from a different company", () => {
  assert.equal(
    planSentence(writing, "A complete piece.", false),
    "First the crew checks what's missing, then the writer and the editor do the work, a second check from a different company reads it, and the writer finishes it.",
  );
});

test("the same maker is said plainly", () => {
  assert.match(planSentence(writing, "", true), /a second check from the same company reads it/);
});

test("a second check whose maker is unknown makes no claim about the company", () => {
  const s = planSentence(writing, "", null);
  assert.match(s, /a second check reads it/);
  assert.doesNotMatch(s, /company/);
});

test("a plan with no fact check starts with its first worker", () => {
  assert.match(planSentence(writing.slice(1), "", false), /^The writer and the editor do the work, /);
});

test("a plan with no second check has no second check in its sentence", () => {
  const s = planSentence(writing.filter((x) => x.owner_role !== "reviewer"), "", false);
  assert.doesNotMatch(s, /second check/);
  assert.match(s, /the writer and the editor do the work/);
});

test("role names come from the label function, and checking roles are the ones it names", () => {
  const steps = [
    { id: "frame", title: "Frame", owner_role: "analyst" },
    { id: "numbers", title: "Recompute", owner_role: "numbers" },
    { id: "final", title: "Finish", owner_role: "editor" },
  ];
  const s = planSentence(steps, "", false, { label: (r) => `${r} role`, checking: (r) => r === "numbers" });
  assert.equal(s, "The analyst role and the editor role do the work, a second check from a different company reads it, and the editor role finishes it.");
});

test("a long list of roles is cut to four", () => {
  const steps = ["a", "b", "c", "d", "e", "f"].map((r) => ({ id: r, title: r, owner_role: r }));
  assert.match(planSentence(steps, "", false), /^The a, the b, the c, the d and 2 others do the work/);
});

test("a plan with no steps gives the outcome's promise", () => {
  assert.equal(planSentence([], "A complete piece.", false), "A complete piece.");
});
