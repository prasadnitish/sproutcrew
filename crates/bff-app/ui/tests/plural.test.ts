import test from "node:test";
import assert from "node:assert/strict";
import { count, needInput } from "../src/plural.ts";

test("one of a thing is singular", () => {
  assert.equal(count(1, "project"), "1 project");
  assert.equal(count(0, "project"), "0 projects");
  assert.equal(count(3, "project"), "3 projects");
  assert.equal(count(2, "process", "processes"), "2 processes");
  assert.equal(count(1200, "request"), "1,200 requests");
});

test("the inbox count agrees with its verb", () => {
  assert.equal(needInput(1), "1 item needs input");
  assert.equal(needInput(0), "0 items need input");
  assert.equal(needInput(4), "4 items need input");
});
