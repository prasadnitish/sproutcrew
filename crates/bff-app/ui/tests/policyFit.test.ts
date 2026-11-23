import test from "node:test";
import assert from "node:assert/strict";
import { defaultsLine, fitLine, groupOf, looserThan, phrase, type PolicyRow } from "../src/policyFit.ts";

const row = (fit: { looser: string[]; stricter: string[] }, g = { send_outside: "ask_each_time", delete_files: "ask_each_time", install_software: "ask_each_time" } as const): PolicyRow =>
  ({ name: "case", plan: true, guardrails: g, privacy: "standard", fit, approved: true, hash: "9f3c 21ab" });

test("each field reads as a phrase", () => {
  assert.equal(phrase("delete_files", "never"), "never deletes");
  assert.equal(phrase("send_outside", "ask_each_time"), "asks before sending outside");
  assert.equal(phrase("send_outside", "varies_by_role"), "asks before sending outside for some roles");
  assert.equal(phrase("privacy", "no_retention"), "providers keep nothing");
  assert.equal(defaultsLine({ send_outside: "ask", delete_files: "never", install_software: "never", privacy: "standard" }),
    "asks before sending outside; never deletes; never installs; providers follow their own policy");
});

test("looser wins over stricter, and a project with no difference is the same", () => {
  assert.equal(groupOf(row({ looser: ["send_outside"], stricter: ["delete_files"] })), "looser");
  assert.equal(groupOf(row({ looser: [], stricter: ["delete_files"] })), "stricter");
  assert.equal(groupOf(row({ looser: [], stricter: [] })), "same");
  assert.equal(fitLine(row({ looser: [], stricter: [] })), "same as your defaults");
  assert.equal(fitLine(row({ looser: ["send_outside"], stricter: [] })), "asks before sending outside");
  assert.equal(fitLine({ name: "x", plan: false }), "no plan yet");
});

test("saving stricter defaults offers only the projects that allow more", () => {
  const open = row({ looser: [], stricter: [] });
  const shut = row({ looser: [], stricter: [] }, { send_outside: "never", delete_files: "ask_each_time", install_software: "ask_each_time" });
  const next = { send_outside: "never", delete_files: "ask", install_software: "ask", privacy: "standard" } as const;
  assert.deepEqual(looserThan([open, shut, { name: "empty", plan: false }], next), [open]);
});
