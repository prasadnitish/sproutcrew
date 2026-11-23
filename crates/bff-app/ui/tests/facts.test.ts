import test from "node:test";
import assert from "node:assert/strict";
import { answersOf, asking, fieldNote, startValues, WAITING } from "../src/facts.ts";

const qs = [
  { ask: "What does the class cost?", from: "pricing-notes.txt", suggested: "$45 a person" },
  { ask: "When is the next class?", from: null, suggested: null },
  { ask: "Who runs it?", from: null, suggested: "Maya" },
];

test("the form shows only while a step is held for the person's answers, and only for that reason", () => {
  assert.equal(WAITING, "Waiting for your answers");
  assert.equal(asking({ outline: "Waiting for your answers" }), true);
  assert.equal(asking({ finish: "The second check said do not send" }), false);
  assert.equal(asking({}), false);
  assert.equal(asking(undefined), false);
});

test("a field starts with the suggestion, and empty when there is none", () => {
  assert.deepEqual(startValues(qs), ["$45 a person", "", "Maya"]);
});

test("a suggestion is labelled with its file, and one with no file just says it is suggested", () => {
  assert.equal(fieldNote(qs[0]), "From pricing-notes.txt");
  assert.equal(fieldNote(qs[1]), "");
  assert.equal(fieldNote(qs[2]), "Suggested");
});

test("an empty or blank field is skipped, and Skip all skips every one", () => {
  assert.deepEqual(answersOf([" $48 ", "  ", ""], false), ["$48", null, null]);
  assert.deepEqual(answersOf(["$45", "x", "y"], true), [null, null, null]);
});
