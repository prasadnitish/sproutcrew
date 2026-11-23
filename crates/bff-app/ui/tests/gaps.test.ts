import test from "node:test";
import assert from "node:assert/strict";
import { answersNote, LEFT_OUT_LINE, leftOut, missing, NOTE_LIMIT, writeAroundNote } from "../src/gaps.ts";

const gaps = [{ what: "the price per seat" }, { what: "the start date" }, { what: "Name the owner." }];

test("the count says detail or details", () => {
  assert.equal(missing(1), "1 detail missing");
  assert.equal(missing(3), "3 details missing");
});

test("what finishing with what is there took out is said as left out, not as missing", () => {
  assert.equal(leftOut(1), "1 detail left out");
  assert.equal(leftOut(6), "6 details left out");
  assert.equal(LEFT_OUT_LINE, "These were left out of the piece:");
});

test("the answers become one note with a line for each answered field", () => {
  assert.equal(answersNote(gaps, { "the price per seat": "$5", "Name the owner.": " Priya  Shah " }), "Details from the person:\nthe price per seat: $5\nName the owner.: Priya Shah");
  assert.equal(answersNote(gaps, { "the start date": "   " }), "", "nothing answered, nothing to save");
  assert.equal(answersNote(gaps, {}), "");
});

test("finishing with what is there tells the step to write around every detail and leave no placeholder", () => {
  assert.equal(
    writeAroundNote(gaps),
    "Write around these missing details; do not leave placeholders: the price per seat; the start date; Name the owner.",
  );
});

test("a long list of details is cut to what a note can hold", () => {
  const many = Array.from({ length: 200 }, (_, i) => ({ what: `detail number ${i} that is quite long indeed` }));
  const note = writeAroundNote(many);
  assert.equal(note.length, NOTE_LIMIT);
  assert.ok(note.startsWith("Write around these missing details; do not leave placeholders: detail number 0"));
});

test("a semicolon inside a detail cannot split it when the note is read back", () => {
  const note = writeAroundNote([{ what: "the price; per seat" }, { what: "the start date" }]);
  assert.equal(note, "Write around these missing details; do not leave placeholders: the price, per seat; the start date");
});
