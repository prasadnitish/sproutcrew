import test from "node:test";
import assert from "node:assert/strict";
import { makerName, secondCheckBy } from "../src/maker.ts";

test("a model id names its maker", () => {
  assert.equal(makerName("mistralai/mistral-small-2603"), "Mistral");
  assert.equal(makerName("anthropic/claude-haiku-5.5"), "Anthropic");
  assert.equal(makerName("openai/gpt-6-luna"), "OpenAI");
  assert.equal(makerName("google/gemini-3-flash"), "Google");
  assert.equal(makerName("deepseek/deepseek-v4"), "DeepSeek");
  assert.equal(makerName("meta-llama/llama-4"), "Meta");
});

test("a maker that is not in the list is its prefix with a capital", () => {
  assert.equal(makerName("qwen/qwen3-max"), "Qwen");
  assert.equal(makerName("cohere/command-r"), "Cohere");
});

test("a model id on a direct route, with no prefix, still names its maker when the family is known", () => {
  assert.equal(makerName("claude-sonnet-5-5"), "Anthropic");
  assert.equal(makerName("gpt-6.1-terra"), "OpenAI");
  assert.equal(makerName("gemini-3-pro"), "Google");
  assert.equal(makerName("something-else"), "");
  assert.equal(makerName(""), "");
});

test("the second check is named by its maker and says whether the writers are the same company", () => {
  assert.equal(secondCheckBy(["mistralai/mistral-small-2603"], ["anthropic/claude-haiku-5.5"]), "Second check by Mistral, a different company");
  assert.equal(secondCheckBy(["anthropic/claude-sonnet-5"], ["anthropic/claude-haiku-5.5"]), "Second check by Anthropic, the same company");
  assert.equal(secondCheckBy(["mistralai/a"], ["anthropic/b", "mistralai/c"]), "Second check by Mistral, the same company", "one writer from the same company is enough to say so");
  assert.equal(secondCheckBy(["claude-sonnet-5-5"], ["gpt-6.1-terra"]), "Second check by Anthropic, a different company");
});

test("a second check by several makers lists them, and one it cannot name makes no claim about companies", () => {
  assert.equal(secondCheckBy(["mistralai/a", "google/b"], ["anthropic/c"]), "Second check by Mistral and Google, a different company");
  assert.equal(secondCheckBy(["mistralai/a", "anthropic/b"], ["anthropic/c"]), "Second check by Mistral and Anthropic");
  assert.equal(secondCheckBy(["mystery"], ["anthropic/c"]), "Second check by another AI");
  assert.equal(secondCheckBy([], ["anthropic/c"]), "");
});
