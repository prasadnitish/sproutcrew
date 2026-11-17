// A small markdown reader for the results models write. It makes a tree, and the screen turns the tree into
// elements, so nothing here ever becomes HTML. Raw HTML in a result stays text. A link keeps its address only
// if it is http, https, or mailto. It reads what results use: headings, paragraphs, lists, tables, code,
// quotes, and bold, italic, code, and link text.

export type Inline =
  | { t: "text"; v: string }
  | { t: "code"; v: string }
  | { t: "strong"; c: Inline[] }
  | { t: "em"; c: Inline[] }
  | { t: "link"; v: string; href: string };

export type Block =
  | { t: "h"; n: number; c: Inline[] }
  | { t: "p"; c: Inline[] }
  | { t: "code"; v: string; lang: string }
  | { t: "ul" | "ol"; items: Block[][] }
  | { t: "quote"; c: Block[] }
  | { t: "hr" }
  | { t: "table"; head: Inline[][]; rows: Inline[][][] };

const SAFE = /^(https?:\/\/|mailto:)/i;
const TOKEN = /(`[^`\n]+`|\*\*[^*\n]+\*\*|__[^_\n]+__|\*[^*\s][^*\n]*\*|_[^_\s][^_\n]*_|\[[^\]\n]+\]\([^)\s]+\))/;

export function inline(src: string): Inline[] {
  const out: Inline[] = [];
  for (const part of src.split(TOKEN)) {
    if (!part) continue;
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) out.push({ t: "code", v: part.slice(1, -1) });
    else if ((part.startsWith("**") && part.endsWith("**") && part.length > 4) || (part.startsWith("__") && part.endsWith("__") && part.length > 4)) out.push({ t: "strong", c: inline(part.slice(2, -2)) });
    else if ((part.startsWith("*") && part.endsWith("*") && part.length > 2) || (part.startsWith("_") && part.endsWith("_") && part.length > 2)) out.push({ t: "em", c: inline(part.slice(1, -1)) });
    else {
      const m = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(part);
      if (m && SAFE.test(m[2])) out.push({ t: "link", v: m[1], href: m[2] });
      else out.push({ t: "text", v: part });
    }
  }
  return out;
}

const indentOf = (l: string) => l.length - l.trimStart().length;
const LIST = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
const cells = (l: string) => l.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
const isRule = (l: string) => /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(l) && l.includes("-");

export function parse(md: string): Block[] {
  const lines = md.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    const fence = /^\s*```(\S*)\s*$/.exec(line);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```\s*$/.test(lines[i])) body.push(lines[i++]);
      i++;
      out.push({ t: "code", v: body.join("\n"), lang: fence[1] });
      continue;
    }
    const h = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(line);
    if (h) { out.push({ t: "h", n: h[1].length, c: inline(h[2]) }); i++; continue; }
    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) { out.push({ t: "hr" }); i++; continue; }
    if (/^\s*>/.test(line)) {
      const body: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) body.push(lines[i++].replace(/^\s*>\s?/, ""));
      out.push({ t: "quote", c: parse(body.join("\n")) });
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && isRule(lines[i + 1])) {
      const head = cells(line).map(inline);
      i += 2;
      const rows: Inline[][][] = [];
      while (i < lines.length && lines[i].includes("|") && lines[i].trim()) rows.push(cells(lines[i++]).map(inline));
      out.push({ t: "table", head, rows });
      continue;
    }
    const li = LIST.exec(line);
    if (li) {
      const base = indentOf(line);
      const ordered = /\d/.test(li[2]);
      const items: Block[][] = [];
      while (i < lines.length) {
        const m = LIST.exec(lines[i]);
        if (!m || indentOf(lines[i]) < base) break;
        if (indentOf(lines[i]) > base) {
          // A deeper list belongs to the item before it.
          const sub: string[] = [];
          while (i < lines.length && lines[i].trim() && indentOf(lines[i]) > base) sub.push(lines[i++]);
          const last = items[items.length - 1];
          if (last) last.push(...parse(sub.join("\n")));
          continue;
        }
        items.push([{ t: "p", c: inline(m[3]) }]);
        i++;
      }
      out.push({ t: ordered ? "ol" : "ul", items });
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^\s*(```|#{1,6}\s|>|-{3,}\s*$)/.test(lines[i]) && !LIST.test(lines[i]) && !(lines[i].includes("|") && i + 1 < lines.length && isRule(lines[i + 1]))) para.push(lines[i++].trim());
    if (para.length === 0) { i++; continue; }
    out.push({ t: "p", c: inline(para.join(" ")) });
  }
  return out;
}
