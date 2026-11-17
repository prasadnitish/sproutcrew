import test from "node:test";
import assert from "node:assert/strict";
import { dollarsLine, group, total, type UsageRow } from "../src/usage.ts";

const at = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h).getTime() / 1000;
const row = (o: Partial<UsageRow>): UsageRow => ({ at: at(2026, 11, 10), task: "t1", stage: "frame", role: "writer", route: "R12", model: "anthropic/claude-haiku-5.5", tokens: 100, dollars: 0.01, dollars_status: "reported", ...o });

const rows = [
  row({ task: "plan", stage: null, role: null, tokens: 50, dollars: 0.001 }),
  row({ task: "t1" }),
  row({ task: "t2", role: "checker", model: "mistralai/mistral-small-2603", tokens: null, dollars: null, dollars_status: "unknown" }),
  row({ task: "t3", at: at(2026, 11, 11), dollars: 0.2 }),
  row({ task: "t4", at: at(2026, 10, 31), tokens: 1000, dollars: 0.05 }),
];

test("days and months are local, newest first, and unknown figures are counted beside the sums", () => {
  const days = group(rows, "day");
  assert.deepEqual(days.map((d) => d.key), ["2026-11-11", "2026-11-10", "2026-10-31"]);
  const nov10 = days[1];
  assert.equal(nov10.requests, 3);
  assert.equal(nov10.tokens, 150);
  assert.equal(nov10.unknownTokens, 1);
  assert.equal(nov10.unknownDollars, 1);
  assert.ok(Math.abs(nov10.dollars - 0.011) < 1e-9);
  assert.deepEqual(group(rows, "month").map((m) => [m.key, m.requests]), [["2026-11", 4], ["2026-10", 1]]);
});

test("roles use the given names, the plan has its own group, and the costliest comes first", () => {
  const roles = group(rows, "role", (r) => (r === "writer" ? "Writer" : "Checker"));
  assert.deepEqual(roles.map((r) => r.label), ["Writer", "Coordinator plan", "Checker"]);
  assert.equal(roles[2].unknownDollars, 1);
});

test("a total keeps unknown apart, and a line never shows unknown as zero", () => {
  const t = total(rows);
  assert.equal(t.requests, 5);
  assert.equal(t.unknownDollars, 1);
  assert.equal(dollarsLine(t.dollars, t.unknownDollars, t.requests), "$0.26 + 1 unknown");
  assert.equal(dollarsLine(0, 2, 2), "unknown");
  assert.equal(dollarsLine(0.004, 0, 1), "under $0.01");
  assert.equal(dollarsLine(0, 0, 0), "$0");
});
