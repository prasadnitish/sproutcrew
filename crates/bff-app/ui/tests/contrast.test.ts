import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// docs/DESIGN.md §11: text is 4.5:1, fields/focus 3:1, status text 4.5:1 on its own 15% fill.
const css = readFileSync(new URL("../src/tokens.css", import.meta.url), "utf8");
const light = css.slice(css.indexOf(":root {"), css.indexOf("@media (prefers-color-scheme: dark)"));
const darkStart = css.indexOf("@media (prefers-color-scheme: dark)");
const dark = css.slice(darkStart, css.indexOf("/* Color schemes"));

const hex = (block: string, name: string): [number, number, number] => {
  const resolve = (key: string, seen = new Set<string>()): string => {
    assert.ok(!seen.has(key), `Circular token: ${key}`);
    seen.add(key);
    const expression = new RegExp(`--${key}:\\s*([^;]+);`, "i");
    const value = (block.match(expression) ?? light.match(expression))?.[1].trim();
    assert.ok(value, `Missing token: ${key}`);
    const alias = value.match(/^var\(--([\w-]+)\)$/);
    return alias ? resolve(alias[1], seen) : value;
  };
  const value = resolve(name);
  assert.match(value, /^#[0-9a-f]{6}$/i);
  const v = value.slice(1);
  return [parseInt(v.slice(0, 2), 16), parseInt(v.slice(2, 4), 16), parseInt(v.slice(4, 6), 16)];
};
const lum = ([r, g, b]: number[]) => {
  const f = (c: number) => { const s = c / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
};
const ratio = (a: number[], b: number[]) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
const mix = (fg: number[], bg: number[], a: number) => fg.map((c, i) => Math.round(a * c + (1 - a) * bg[i]));

for (const [mode, block, inherit] of [["light", light, light], ["dark", dark, light]] as const) {
  const get = (n: string) => { try { return hex(block, n); } catch { return hex(inherit, n); } };
  test(`${mode}: body text, meta text, links and button labels have enough contrast`, () => {
    for (const plane of ["base", "plane-sidebar", "plane-raised", "plane-overlay"]) {
      assert.ok(ratio(get("ink"), get(plane)) >= 4.5, `ink on ${plane}`);
      assert.ok(ratio(get("ink-muted"), get(plane)) >= 4.5, `ink-muted on ${plane}`);
    }
    for (const plane of ["base", "plane-raised"]) assert.ok(ratio(get("ink-faint"), get(plane)) >= 4.5, `ink-faint on ${plane}`);
    assert.ok(ratio(get("accent-ink"), get("accent")) >= 4.5, "label on the accent button");
    assert.ok(ratio(get("accent-ink"), get("accent-hover")) >= 4.5, "label on the hovered button");
    assert.ok(ratio(get("accent-ink"), get("accent-press")) >= 4.5, "label on the pressed button");
    for (const plane of ["base", "plane-raised", "plane-overlay"]) assert.ok(ratio(get("accent-text"), get(plane)) >= 4.5, `accent text on ${plane}`);
  });
  test(`${mode}: field boundaries and focus rings are visible`, () => {
    assert.ok(ratio(get("input-border"), get("plane-raised")) >= 3, "field border");
    for (const plane of ["base", "plane-sidebar", "plane-raised", "plane-overlay"]) assert.ok(ratio(get("focus"), get(plane)) >= 3, `focus on ${plane}`);
  });
  test(`${mode}: status text is readable on its own 15% fill`, () => {
    for (const plane of ["base", "plane-sidebar", "plane-raised", "plane-overlay"]) {
      for (const s of ["live", "warn", "danger"]) {
        const fill = mix(get(s), get(plane), 0.15);
        assert.ok(ratio(get(`${s}-ink`), fill) >= 4.5, `${s}-ink on ${plane}: ${ratio(get(`${s}-ink`), fill).toFixed(2)}`);
      }
    }
  });
  test(`${mode}: the muted identity tints are readable on their fills`, () => {
    for (const side of ["write", "check"]) assert.ok(ratio(get(`side-${side}`), get(`side-${side}-fill`)) >= 4.5, `${side}`);
  });
}
