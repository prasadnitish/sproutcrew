import { test } from "node:test";
import assert from "node:assert/strict";
import { guessStep } from "../src/steps.ts";

const steps = ["frame", "recommend", "review", "document", "deck", "final"].map((id) => ({ id }));

test("the first id in the list wins, not the first step in plan order", () => {
  assert.equal(guessStep(steps, ["document", "client_doc", "recommend"]), "document");
});

test("falls back to a later id when the first is absent", () => {
  assert.equal(guessStep(steps.filter((s) => s.id !== "document"), ["document", "recommend"]), "recommend");
});

test("empty when nothing matches", () => {
  assert.equal(guessStep(steps, ["nothing"]), "");
});
