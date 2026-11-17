import { test } from "node:test";
import assert from "node:assert/strict";
import { parse, type Block, type Inline } from "../src/markdown.ts";

const text = (c: Inline[]): string => c.map((i) => (i.t === "text" || i.t === "code" ? i.v : i.t === "link" ? i.v : text(i.c))).join("");

test("headings, paragraphs, and blank lines", () => {
  const b = parse("# Title\n\nFirst line\nsecond line\n\n## Next");
  assert.equal(b.length, 3);
  assert.deepEqual([b[0].t, (b[0] as any).n, text((b[0] as any).c)], ["h", 1, "Title"]);
  assert.equal(text((b[1] as any).c), "First line second line");
  assert.equal((b[2] as any).n, 2);
});

test("inline bold, italic, code, and links", () => {
  const [p] = parse("A **bold** and *soft* and `code` and [site](https://example.com).") as Block[];
  const kinds = (p as any).c.map((i: Inline) => i.t);
  assert.deepEqual(kinds, ["text", "strong", "text", "em", "text", "code", "text", "link", "text"]);
  const link = (p as any).c.find((i: Inline) => i.t === "link");
  assert.deepEqual([link.v, link.href], ["site", "https://example.com"]);
});

test("a link that is not http, https, or mailto stays as plain text", () => {
  const [p] = parse("[x](javascript:alert(1)) and [y](file:///etc/passwd)") as Block[];
  assert.ok(!(p as any).c.some((i: Inline) => i.t === "link"), JSON.stringify(p));
  assert.match(text((p as any).c), /javascript:alert/);
});

test("html is text and never markup", () => {
  const [p] = parse("<script>alert(1)</script> and <img src=x onerror=y>") as Block[];
  assert.equal(text((p as any).c), "<script>alert(1)</script> and <img src=x onerror=y>");
});

test("lists, ordered lists, and one level of nesting", () => {
  const [ul, ol] = parse("- one\n- two\n  - inner\n- three\n\n1. first\n2. second") as any[];
  assert.equal(ul.t, "ul");
  assert.equal(ul.items.length, 3);
  assert.equal(ul.items[1].length, 2, "item text plus a nested list");
  assert.equal(ul.items[1][1].t, "ul");
  assert.equal(ol.t, "ol");
  assert.equal(ol.items.length, 2);
});

test("a table keeps its header and rows", () => {
  const [t] = parse("| team | heads |\n|---|---:|\n| intake | 40 |\n| adjusters | 120 |") as any[];
  assert.equal(t.t, "table");
  assert.deepEqual(t.head.map(text), ["team", "heads"]);
  assert.deepEqual(t.rows.map((r: Inline[][]) => r.map(text)), [["intake", "40"], ["adjusters", "120"]]);
});

test("a code fence keeps its text exactly, even when it looks like markdown, and an unclosed fence runs to the end", () => {
  const [c] = parse("```js\n# not a heading\n**not bold**\n```") as any[];
  assert.deepEqual([c.t, c.lang, c.v], ["code", "js", "# not a heading\n**not bold**"]);
  const [open] = parse("```\nno end") as any[];
  assert.equal(open.v, "no end");
});

test("quotes and rules", () => {
  const [q, hr] = parse("> a quoted line\n\n---") as any[];
  assert.equal(q.t, "quote");
  assert.equal(hr.t, "hr");
});

test("empty and odd input does not throw", () => {
  assert.deepEqual(parse(""), []);
  assert.deepEqual(parse("\n\n  \n"), []);
  parse("**unclosed and `unclosed and [no](");
  parse("|".repeat(50_000));
  parse("*".repeat(50_000));
  parse("a\n".repeat(20_000));
});
