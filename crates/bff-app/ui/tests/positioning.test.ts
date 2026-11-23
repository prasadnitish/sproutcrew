import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { audience, headline } from "../src/positioning.ts";

// The first screen says what the website's first screen says.
const hero = readFileSync(new URL("../../../../website/index.html", import.meta.url), "utf8");
const text = (html: string) => html.replace(/<br\s*\/?>/g, " ").replace(/<[^>]+>/g, "").replace(/\s+/g, " ").trim();

test("the first screen's headline and line are the website hero's", () => {
  const h1 = text(/<h1>([\s\S]*?)<\/h1>/.exec(hero)![1]);
  const line = text(/<\/h1><p>([\s\S]*?)<\/p>/.exec(hero)![1]);
  assert.equal(headline, h1);
  assert.equal(audience, line);
});
