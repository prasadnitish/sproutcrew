import test from "node:test";
import assert from "node:assert/strict";
import { amount } from "../src/amount.ts";

test("a plain positive number is an amount, with or without spaces around it", () => {
  assert.equal(amount("10"), 10);
  assert.equal(amount(" 0.50 "), 0.5);
  assert.equal(amount("1e3"), 1000);
});

test("text that only starts like a number is not an amount", () => {
  for (const t of ["1o", "1,000", "10 000", "5.5.5", "$5", "abc", "", "  ", "0", "-3", "Infinity", "NaN"]) {
    assert.equal(amount(t), null, t);
  }
});
