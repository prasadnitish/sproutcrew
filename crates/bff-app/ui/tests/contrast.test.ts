import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// DESIGN_SPEC.md §3, §4 and §13, in every scheme and both modes: text is 4.5:1; field edges and focus 3:1.
// The pairs are the ones docs/plan/wireframes/schemes.py checks for the locked schemes.
type Values = Record<string, string>;
const tokens = JSON.parse(readFileSync(new URL("../src/brand/tokens.json", import.meta.url), "utf8"));
const css = readFileSync(new URL("../src/tokens.css", import.meta.url), "utf8");

/** The 33 roles every scheme sets, in light and in dark (DESIGN_SPEC.md §3.2). */
const ROLES = (
  "base plane-sidebar plane-raised plane-overlay ink ink-muted ink-faint field-border accent accent-ink accent-text " +
  "accent-hover accent-press brand-ink focus mark-leaf warn warn-ink warn-fill danger danger-ink danger-fill live live-ink live-fill " +
  "accent-fill side-write side-write-fill side-check side-check-fill line hover press"
).split(" ");
const SCHEMES = ["electric", ...Object.keys(tokens.schemes)];
const PLANES = ["base", "plane-sidebar", "plane-raised", "plane-overlay"];

/** What `:root` resolves to for a scheme and mode, in cascade order. Electric is plain `:root`. */
function values(scheme: string, mode: "light" | "dark"): Values {
  const s = tokens.schemes[scheme] ?? {};
  return { ...tokens.component, ...tokens.semantic.light, ...(mode === "dark" ? tokens.semantic.dark : {}), ...(s.light ?? {}), ...(mode === "dark" ? s.dark ?? {} : {}) };
}

type RGB = [number, number, number];
const parseHex = (v: string): RGB => {
  assert.match(v, /^#[0-9a-f]{6}$/i, `not a solid color: ${v}`);
  return [0, 2, 4].map((i) => parseInt(v.slice(1 + i, 3 + i), 16)) as RGB;
};
const over = (fg: RGB, bg: RGB, a: number): RGB => fg.map((c, i) => Math.round(a * c + (1 - a) * bg[i])) as RGB;

/** A role as a solid color. `transparent` in a color-mix is laid over `ground`, the plane it sits on. */
function color(v: Values, name: string, ground?: RGB, seen = new Set<string>()): RGB {
  assert.ok(!seen.has(name), `circular token: ${name}`);
  seen.add(name);
  const raw = (v[name] ?? "").trim();
  assert.ok(raw, `missing token: ${name}`);
  const ref = (expr: string): RGB => {
    const m = expr.trim().match(/^var\(--([\w-]+)\)$/);
    if (!m) return parseHex(expr.trim());
    const prim = m[1].match(/^sc-color-(.+)$/);
    return prim ? parseHex(tokens.primitive.color[prim[1]]) : color(v, m[1], ground, seen);
  };
  const mix = raw.match(/^color-mix\(in srgb, (.+?) (\d+)%, (.+)\)$/);
  if (mix) {
    const back = mix[3].trim() === "transparent" ? ground : ref(mix[3]);
    assert.ok(back, `${name} needs a plane to sit on`);
    return over(ref(mix[1]), back, Number(mix[2]) / 100);
  }
  return ref(raw);
}

const lum = (c: RGB) => {
  const f = (x: number) => { const s = x / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
};
const ratio = (a: RGB, b: RGB) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

test("there are five schemes and Electric is the default", () => {
  assert.deepEqual(SCHEMES, ["electric", "mono", "pastel", "pop", "lagoon"]);
  assert.match(css, /^\/\*[^]*?\*\/\n:root \{/, "Electric is plain :root");
});

for (const scheme of SCHEMES) {
  if (scheme === "electric") continue;
  test(`${scheme} sets every role in light and dark, and the cascade reaches it`, () => {
    for (const mode of ["light", "dark"] as const) {
      const keys = Object.keys(tokens.schemes[scheme][mode]).sort();
      assert.deepEqual(keys, [...ROLES].sort(), `${scheme} ${mode}`);
    }
    const q = `:root[data-scheme="${scheme}"]`;
    assert.ok(css.includes(`${q} {`), "light block");
    assert.ok(css.includes(`${q}:not([data-theme="light"]) {`), "dark block for Match this Mac");
    assert.ok(css.includes(`${q}[data-theme="dark"] {`), "dark block for Dark");
  });
}

test("Electric sets every role in light and dark", () => {
  for (const mode of ["light", "dark"] as const) for (const r of ROLES) assert.ok(values("electric", mode)[r], `${mode} ${r}`);
});

test("the splash and reference colors are fixed and no scheme changes them", () => {
  for (const s of Object.keys(tokens.schemes)) for (const m of ["light", "dark"]) for (const k of ["brand-apricot", "splash-ink", "splash-base", "splash-muted"]) {
    assert.ok(!(k in tokens.schemes[s][m]), `${s} ${m} sets ${k}`);
  }
  assert.equal(tokens.primitive.color.apricot, "#F0A76A");
  assert.equal(tokens.primitive.color.blue, "#233F80");
});

for (const scheme of SCHEMES) {
  for (const mode of ["light", "dark"] as const) {
    const v = values(scheme, mode);
    const c = (n: string, g?: RGB) => color(v, n, g);
    const need = (fg: string, bg: RGB, min: number, why: string) => {
      const r = ratio(c(fg, bg), bg);
      assert.ok(r >= min, `${why}: ${r.toFixed(2)} < ${min}`);
    };
    const tag = `${scheme} ${mode}`;

    test(`${tag}: text, muted text and links are readable on every plane and control`, () => {
      for (const p of PLANES) {
        const plane = c(p);
        for (const bg of [plane, c("control-bg", plane)]) {
          need("ink", bg, 4.5, `ink on ${p}`);
          need("ink-muted", bg, 4.5, `ink-muted on ${p}`);
          need("accent-text", bg, 4.5, `accent-text on ${p}`);
        }
        need("ink-faint", plane, 4.5, `ink-faint on ${p}`);
        // A disabled primary button: its label is ink-faint on the control fill (DESIGN_SPEC.md §9, §13). Primary buttons
        // sit on content, raised planes and dialogs, never in the sidebar.
        if (p !== "plane-sidebar") need("ink-faint", c("control-bg", plane), 4.5, `disabled label on control-bg over ${p}`);
      }
    });

    test(`${tag}: field edges and focus are visible`, () => {
      for (const p of PLANES) {
        need("field-border", c(p), 3, `field-border on ${p}`);
        need("focus", c(p), 3, `focus on ${p}`);
      }
    });

    test(`${tag}: status words are readable on their fills and planes`, () => {
      for (const s of ["warn", "danger", "live"]) {
        need(`${s}-ink`, c(`${s}-fill`), 4.5, `${s}-ink on ${s}-fill`);
        need("ink", c(`${s}-fill`), 4.5, `ink on ${s}-fill (diff lines)`);
        for (const p of PLANES) need(`${s}-ink`, c(p), 4.5, `${s}-ink on ${p}`);
      }
    });

    test(`${tag}: the primary button label and the selected row are readable`, () => {
      for (const k of ["accent", "accent-hover", "accent-press"]) need("accent-ink", c(k), 4.5, `label on ${k}`);
      for (const k of ["accent-text", "ink", "ink-muted"]) need(k, c("accent-fill"), 4.5, `${k} on accent-fill`);
    });

    test(`${tag}: writer and checker tints are readable on their fills`, () => {
      for (const k of ["side-write", "side-check"]) need(k, c(`${k}-fill`), 4.5, k);
    });
  }
}
