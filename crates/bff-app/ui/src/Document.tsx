import { Fragment, ReactNode, useMemo } from "react";
import { Block, Inline, parse } from "./markdown";

const In = ({ c }: { c: Inline[] }) => (
  <>
    {c.map((i, k) => {
      switch (i.t) {
        case "text": return <Fragment key={k}>{i.v}</Fragment>;
        case "code": return <code key={k}>{i.v}</code>;
        case "strong": return <strong key={k}><In c={i.c} /></strong>;
        case "em": return <em key={k}><In c={i.c} /></em>;
        // A link is shown as text with its address on hover. Clicking it must not move this window.
        case "link": return <span key={k} className="ext" title={i.href}>{i.v}</span>;
      }
    })}
  </>
);

function Blocks({ b }: { b: Block[] }): ReactNode {
  return b.map((x, k) => {
    switch (x.t) {
      case "h": {
        const H = (`h${Math.min(6, x.n + 1)}`) as "h2";
        return <H key={k}><In c={x.c} /></H>;
      }
      case "p": return <p key={k}><In c={x.c} /></p>;
      case "code": return <pre key={k} className="code"><code>{x.v}</code></pre>;
      case "quote": return <blockquote key={k}><Blocks b={x.c} /></blockquote>;
      case "hr": return <hr key={k} />;
      case "ul":
      case "ol": {
        const L = x.t;
        return <L key={k}>{x.items.map((it, j) => <li key={j}><Blocks b={it} /></li>)}</L>;
      }
      case "table":
        return (
          <div className="table-wrap" key={k}>
            <table>
              <thead><tr>{x.head.map((c, j) => <th key={j}><In c={c} /></th>)}</tr></thead>
              <tbody>{x.rows.map((r, j) => <tr key={j}>{r.map((c, n) => <td key={n}><In c={c} /></td>)}</tr>)}</tbody>
            </table>
          </div>
        );
    }
  });
}

/** A result, read as a document. Nothing in it can become markup. */
export default function Markdown({ text, skipTitle }: { text: string; skipTitle?: string }) {
  const blocks = useMemo(() => {
    const b = parse(text);
    // A result that starts by repeating the step's own title does not need to say it twice.
    const first = b[0];
    const plain = (c: Inline[]): string => c.map((i) => (i.t === "text" || i.t === "code" ? i.v : i.t === "link" ? i.v : plain(i.c))).join("").trim().toLowerCase();
    return skipTitle && first && first.t === "h" && first.n === 1 && plain(first.c) === skipTitle.trim().toLowerCase() ? b.slice(1) : b;
  }, [text, skipTitle]);
  return <div className="doc-text"><Blocks b={blocks} /></div>;
}
