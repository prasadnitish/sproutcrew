import { useEffect, useMemo, useRef, useState } from "react";
import { fuzzy } from "./fuzzy";

export type PaletteItem = { id: string; group: "Actions" | "Projects" | "Sections"; label: string; hint?: string; run: () => void };

/** The command palette (⌘K, docs/DESIGN.md §5): the overlay plane, centered in the top third, fuzzy results grouped
 *  by kind, keyboard first, with the shortcuts in the footer. It only goes places and runs things the screens
 *  already do. */
export default function CommandPalette({ items, onClose }: { items: PaletteItem[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [at, setAt] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { input.current?.focus(); }, []);
  const shown = useMemo(() => {
    return items
      .map((it) => ({ it, s: fuzzy(q, it.label) }))
      .filter((x): x is { it: PaletteItem; s: number } => x.s !== null)
      .sort((a, b) => b.s - a.s)
      .map((x) => x.it);
  }, [items, q]);
  const groups = (["Actions", "Projects", "Sections"] as const).map((g) => [g, shown.filter((i) => i.group === g)] as const).filter(([, l]) => l.length > 0);
  const flat = groups.flatMap(([, l]) => l);
  const go = (it?: PaletteItem) => { if (it) { onClose(); it.run(); } };
  const key = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); onClose(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setAt((a) => Math.min(a + 1, flat.length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setAt((a) => Math.max(a - 1, 0)); }
    else if (e.key === "Enter") { e.preventDefault(); go(flat[at]); }
  };
  return (
    <div className="scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="palette-box" role="dialog" aria-modal="true" aria-label="Command palette" onKeyDown={key}>
        <input ref={input} className="palette-input" value={q} placeholder="Go to a project or section, or run an action" aria-label="Search" spellCheck={false}
          onChange={(e) => { setQ(e.target.value); setAt(0); }} />
        <div className="palette-results" role="listbox">
          {flat.length === 0 && <p className="muted palette-empty">Nothing matches. Clear the search to see everything.</p>}
          {groups.map(([g, list]) => (
            <div key={g}>
              <div className="eyebrow palette-group">{g}</div>
              {list.map((it) => (
                <button key={it.id} role="option" aria-selected={flat[at] === it} className={`palette-item${flat[at] === it ? " on" : ""}`} onMouseMove={() => setAt(flat.indexOf(it))} onClick={() => go(it)}>
                  <span>{it.label}</span>
                  {it.hint && <kbd>{it.hint}</kbd>}
                </button>
              ))}
            </div>
          ))}
        </div>
        <div className="palette-foot meta"><span><kbd>↑</kbd> <kbd>↓</kbd> move</span><span><kbd>⏎</kbd> open</span><span><kbd>Esc</kbd> close</span></div>
      </div>
    </div>
  );
}
