import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DEFAULT_SCHEME, SCHEMES, schemeFromStored } from "../src/scheme.ts";

const tokens = JSON.parse(readFileSync(new URL("../src/brand/tokens.json", import.meta.url), "utf8"));

test("Electric is the default and comes first", () => {
  assert.equal(DEFAULT_SCHEME, "electric");
  assert.equal(SCHEMES[0].id, "electric");
});

test("the list is the five schemes in the token source", () => {
  assert.deepEqual(SCHEMES.map((s) => s.id), ["electric", ...Object.keys(tokens.schemes)]);
});

test("a stored scheme is kept, and anything else gives Electric", () => {
  for (const s of SCHEMES) assert.equal(schemeFromStored(s.id), s.id);
  for (const v of [null, "", "classic", "graphite", "ELECTRIC", " mono", "<script>"]) assert.equal(schemeFromStored(v), "electric");
});

test("each scheme has a name, its line from the spec, and swatches from its own light values", () => {
  for (const s of SCHEMES) {
    assert.ok(s.name && s.blurb.endsWith("."), s.id);
    const light = s.id === "electric" ? tokens.semantic.light : tokens.schemes[s.id].light;
    assert.deepEqual(s.swatches, [light.base, light.ink, light.accent, light.warn, light.danger, light.live], s.id);
  }
});

test("only a scheme other than Electric is stored, the old palette key goes, and blocked storage still applies it", async () => {
  const { applyScheme, loadScheme } = await import("../src/scheme.ts");
  const store = new Map<string, string>([["bff-palette", "harbor"]]);
  const dataset: Record<string, string> = {};
  const oldDocument = Object.getOwnPropertyDescriptor(globalThis, "document");
  const oldStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  const storage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v), removeItem: (k: string) => void store.delete(k) };
  try {
    Object.defineProperty(globalThis, "document", { configurable: true, value: { documentElement: { dataset } } });
    Object.defineProperty(globalThis, "localStorage", { configurable: true, value: storage });
    assert.equal(loadScheme(), "electric");
    assert.equal(store.has("bff-palette"), false);
    applyScheme("pop");
    assert.equal(dataset.scheme, "pop");
    assert.equal(store.get("sproutcrew-scheme"), "pop");
    assert.equal(loadScheme(), "pop");
    applyScheme("electric");
    assert.equal(dataset.scheme, "electric");
    assert.equal(store.has("sproutcrew-scheme"), false);
    Object.defineProperty(globalThis, "localStorage", { configurable: true, get() { throw new Error("Storage blocked"); } });
    assert.equal(loadScheme(), "electric");
    assert.doesNotThrow(() => applyScheme("lagoon"));
    assert.equal(dataset.scheme, "lagoon");
  } finally {
    if (oldDocument) Object.defineProperty(globalThis, "document", oldDocument); else Reflect.deleteProperty(globalThis, "document");
    if (oldStorage) Object.defineProperty(globalThis, "localStorage", oldStorage); else Reflect.deleteProperty(globalThis, "localStorage");
  }
});
