import { test } from "node:test";
import assert from "node:assert/strict";
import { destOrder, mainResult, nextBrief } from "../src/handoff.ts";
import { restartName } from "../src/projects.ts";

test("an app project leads with the app, and documents stay available at the end", () => {
  const order = destOrder(true);
  assert.equal(order[0], "app");
  assert.ok(order.indexOf("github") < order.indexOf("client"));
  assert.ok(order.includes("client") && order.includes("next"));
});

test("a project with no code has no app destination and leads with documents", () => {
  const order = destOrder(false);
  assert.equal(order[0], "client");
  assert.ok(!order.includes("app"));
  assert.ok(order.includes("next"));
});

test("the result to carry over is the finished document when there is one", () => {
  assert.equal(mainResult([{ id: "frame" }, { id: "document" }, { id: "polish_document" }]), "polish_document");
  assert.equal(mainResult([{ id: "frame" }, { id: "recommend" }]), "recommend");
  assert.equal(mainResult([{ id: "first" }, { id: "last" }]), "last", "otherwise the last finished step");
  assert.equal(mainResult([]), "");
});

test("the suggested brief names the project it continues from", () => {
  assert.match(nextBrief("app_development", "Case-3"), /prototype of what the project "Case-3" recommends/);
  assert.match(nextBrief("proposal", "Case-3"), /Continue from the project "Case-3"/);
});

test("a follow-on project gets a name of its own, within the limit", () => {
  assert.equal(restartName("case", [], "app"), "case-app");
  assert.equal(restartName("case", ["CASE-app"], "app"), "case-app-2");
  assert.equal(restartName("x".repeat(70), [], "app").length, 64);
  assert.equal(restartName("case", []), "case-restart", "starting over is unchanged");
});
