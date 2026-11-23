import test from "node:test";
import assert from "node:assert/strict";
import { replaceModel, scopeOf, sideModels } from "../src/modelChange.ts";

const team = [
  { role: "writer", route: "R12", model: "anthropic/claude-haiku-5.5", effort: "low" },
  { role: "editor", route: "R12", model: "anthropic/claude-haiku-5.5", effort: "high" },
  { role: "web_researcher", route: "R12", model: "perplexity/sonar", effort: "low" },
  { role: "reviewer", route: "R12", model: "mistralai/mistral-small-2603", effort: "medium" },
];
const stages = [
  { id: "facts", title: "Check", owner_role: "writer" },
  { id: "web_research", title: "Web", owner_role: "web_researcher" },
  { id: "outline", title: "Shape", owner_role: "writer" },
  { id: "edit", title: "Edit", owner_role: "editor" },
  { id: "review", title: "Check", owner_role: "reviewer" },
];
const writing = (r: string) => r !== "reviewer";
const plan = { team, stages };

test("the writer's steps on the replaced model change, each keeping its own route and effort", () => {
  const out = replaceModel(plan, writing, "anthropic/claude-haiku-5.5", "openai/gpt-6-luna")!;
  const by = Object.fromEntries(out.map((s) => [s.id, s.uses]));
  assert.deepEqual(by.facts, { route: "R12", model: "openai/gpt-6-luna", effort: "low" });
  assert.deepEqual(by.edit, { route: "R12", model: "openai/gpt-6-luna", effort: "high" });
});

test("the web step and the other side are left alone", () => {
  const out = replaceModel(plan, writing, "anthropic/claude-haiku-5.5", "openai/gpt-6-luna")!;
  assert.equal(out.find((s) => s.id === "web_research")!.uses, undefined);
  assert.equal(out.find((s) => s.id === "review")!.uses, undefined);
});

test("a step deliberately on another model is not overwritten", () => {
  const own = stages.map((s) => (s.id === "outline" ? { ...s, uses: { route: "R12", model: "google/gemini-3-flash", effort: "medium" } } : s));
  const out = replaceModel({ team, stages: own }, writing, "anthropic/claude-haiku-5.5", "openai/gpt-6-luna")!;
  assert.equal(out.find((s) => s.id === "outline")!.uses!.model, "google/gemini-3-flash");
  assert.equal(scopeOf({ team, stages: own }, writing, "anthropic/claude-haiku-5.5").steps.length, 2);
});

test("a change is refused when a step it would touch is not on OpenRouter", () => {
  const direct = team.map((t) => (t.role === "editor" ? { ...t, route: "R3", model: "claude-sonnet-5-5" } : t));
  // The writer and the editor are on different models here, so the editor is out of scope and the writer's change goes through.
  assert.ok(replaceModel({ team: direct, stages }, writing, "anthropic/claude-haiku-5.5", "openai/gpt-6-luna"));
  const mixed = team.map((t) => (t.role === "editor" ? { ...t, route: "R3" } : t));
  assert.equal(scopeOf({ team: mixed, stages }, writing, "anthropic/claude-haiku-5.5").onOpenRouter, false);
  assert.equal(replaceModel({ team: mixed, stages }, writing, "anthropic/claude-haiku-5.5", "openai/gpt-6-luna"), null);
});

test("direct-route steps outside the replaced model stay as they are", () => {
  const direct = team.map((t) => (t.role === "editor" ? { ...t, route: "R3", model: "claude-sonnet-5-5" } : t));
  const out = replaceModel({ team: direct, stages }, writing, "anthropic/claude-haiku-5.5", "openai/gpt-6-luna")!;
  assert.equal(out.find((s) => s.id === "edit")!.uses, undefined);
});

test("the most used model comes first, and the web step's model comes last", () => {
  assert.deepEqual(sideModels(plan, writing), ["anthropic/claude-haiku-5.5", "perplexity/sonar"]);
});
