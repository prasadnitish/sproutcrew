import test from "node:test";
import assert from "node:assert/strict";
import { costParts, heldStop, plainWhy } from "../src/failures.ts";

const timeout =
  "stopped: OpenRouter did not answer within 5 minutes, so SproutCrew cannot tell whether the request was charged. It counted the most the request could have cost and stopped all AI work, so nothing more is spent until you have checked.";
const held =
  "stopped: AI work is on hold. OpenRouter did not answer within 5 minutes, so SproutCrew cannot tell whether the request was charged. Check your usage with the provider, then quit and reopen SproutCrew to go on.";

test("a charge that could not be confirmed, and every step refused after it, is a held stop", () => {
  assert.equal(heldStop(timeout), true);
  assert.equal(heldStop(held), true);
  assert.equal(heldStop("the answer was unusable: no text"), false);
  assert.equal(heldStop("paused: a stop was requested before the step started"), false);
});

test("a failure note reads as a sentence without its label or repeated advice", () => {
  assert.equal(
    plainWhy(held),
    "OpenRouter did not answer within 5 minutes, so SproutCrew cannot tell whether the request was charged.",
  );
  assert.equal(plainWhy("the answer was unusable: no text."), "The answer was unusable: no text.");
  assert.equal(plainWhy("paused: this project reached its limit"), "This project reached its limit.");
});

test("the cost meter rounds the figure and says how many requests have no known cost", () => {
  assert.deepEqual(costParts("$0.011793 (1 entry unknown)"), { center: "$0.01", sub: "Plus 1 request whose cost is not known" });
  assert.deepEqual(costParts("$0.004381"), { center: "under $0.01", sub: "As reported by the provider" });
  assert.deepEqual(costParts("$1.2 (3 entries unknown)"), { center: "$1.20", sub: "Plus 3 requests whose cost is not known" });
  assert.equal(costParts("$0").center, "$0");
});
