import { test } from "node:test";
import assert from "node:assert/strict";
import { PALETTES, paletteFromStored } from "../src/palette.ts";

test("a stored name that is a palette is kept", () => {
  for (const p of PALETTES) assert.equal(paletteFromStored(p.id), p.id);
});

test("anything else falls back to the first palette", () => {
  for (const v of [null, "", "nope", "CLASSIC ", "<script>"]) assert.equal(paletteFromStored(v), PALETTES[0].id);
});

test("every palette has a name, a line about it, and three different swatches", () => {
  const ids = new Set<string>();
  for (const p of PALETTES) {
    assert.ok(p.name && p.note);
    assert.equal(new Set(p.swatches).size, 3, p.id);
    assert.ok(!ids.has(p.id));
    ids.add(p.id);
  }
  assert.ok(PALETTES.length >= 5);
});
