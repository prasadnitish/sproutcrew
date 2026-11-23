import { test } from "node:test";
import assert from "node:assert/strict";
import { GROUPS, boardGroups, firstOpen, kindOf, moveFrom } from "../src/queue.ts";

const item = (kind: string, tier = "consequential", project = "alpha", id = kind) =>
  ({ project, stage: "s1", id, tier, kind, title: `${kind} title`, anchor: `a-${id}`, step_title: "Draft the article" });

test("the four groups are in the spec's order, with their words and tones", () => {
  assert.deepEqual(GROUPS.map((g) => g.label), ["Needs your approval", "Needs an answer", "Failed or stopped", "Finished, not reviewed"]);
  assert.deepEqual(GROUPS.map((g) => g.tone), ["warn", "info", "bad", "quiet"]);
});

test("every kind the core sends lands in one group, with a word and a glyph", () => {
  const want: Record<string, string> = {
    plan: "approve", action: "approve", code: "approve", write: "approve", proposal: "approve",
    questions: "answer", finding: "answer", escalation: "answer", details: "answer",
    failure: "failed", stopped: "failed", finished: "review",
  };
  for (const [kind, group] of Object.entries(want)) {
    const k = kindOf(kind);
    assert.equal(k.group, group, kind);
    assert.ok(k.word && k.glyph, kind);
  }
  assert.equal(kindOf("stopped").word, "Stopped by you");
});

test("an unknown kind is shown, never dropped, and asks for an answer", () => {
  const k = kindOf("something-new");
  assert.equal(k.group, "answer");
  assert.equal(k.word, "Decision");
});

test("items are grouped and ordered by risk, destructive first; empty groups are left out", () => {
  const g = boardGroups([
    item("finding", "reversible"), item("write"), item("code", "destructive"), item("failure"), item("plan", "consequential", "beta"),
  ]);
  assert.deepEqual(g.map((x) => x.id), ["approve", "answer", "failed"]);
  assert.deepEqual(g[0].items.map((i) => i.kind), ["code", "write", "plan"]);
});

test("within one risk the core's order is kept", () => {
  const g = boardGroups([item("plan", "consequential", "beta"), item("write", "consequential", "alpha")]);
  assert.deepEqual(g[0].items.map((i) => i.project), ["beta", "alpha"]);
});

test("the first group with something in it opens first, and its first item is picked", () => {
  const g = boardGroups([item("failure"), item("questions")]);
  assert.equal(firstOpen(g)?.kind, "questions");
  assert.equal(firstOpen([]), null);
});

test("J and K move through the items in board order and stop at the ends", () => {
  const g = boardGroups([item("failure"), item("questions"), item("plan")]);
  const keys = g.flatMap((x) => x.items).map((i) => i.kind);
  assert.deepEqual(keys, ["plan", "questions", "failure"]);
  const at = (k: string) => g.flatMap((x) => x.items).find((i) => i.kind === k)!;
  assert.equal(moveFrom(g, at("plan"), 1)?.kind, "questions");
  assert.equal(moveFrom(g, at("failure"), 1)?.kind, "failure");
  assert.equal(moveFrom(g, at("plan"), -1)?.kind, "plan");
  assert.equal(moveFrom(g, null, 1)?.kind, "plan");
});
