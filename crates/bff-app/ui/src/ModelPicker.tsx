import { KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { Catalog, CatalogModel, contextLine, filterModels, groups, makers, priceLine } from "./models";
import { text } from "./ui";

let cached: Promise<Catalog> | null = null;
/** OpenRouter's model list, read once per launch. The app keeps its own copy and refreshes it in the background. */
export function loadCatalog(): Promise<Catalog> {
  cached ??= api.call<Catalog>("model_catalog").catch((e) => { cached = null; throw e; });
  return cached;
}

/** The model's readable name, from the list when it has one. */
export function useModelName(id: string): string {
  const [name, setName] = useState(id);
  useEffect(() => {
    let live = true;
    loadCatalog().then((c) => live && setName(c.models.find((m) => m.id === id)?.name ?? id)).catch(() => undefined);
    return () => { live = false; };
  }, [id]);
  return name;
}

/** A searchable list of every OpenRouter model, grouped Recommended, Free and All models, with prices per
 *  million tokens and context length. An expert can type any model ID instead. Choosing a model only changes the
 *  plan; the plan still has to be approved before anything runs. */
export default function ModelPicker({ side, value, onPick, onClose }: { side: "writing" | "checking"; value: string; onPick: (id: string) => void; onClose: () => void }) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [err, setErr] = useState("");
  const [query, setQuery] = useState("");
  const [maker, setMaker] = useState("");
  const [typed, setTyped] = useState("");
  const [typedErr, setTypedErr] = useState("");
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => { loadCatalog().then(setCatalog).catch((e) => setErr(text(e))); }, []);
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    box.current?.querySelector<HTMLInputElement>("input")?.focus();
    return () => before?.focus?.();
  }, []);
  const shown = useMemo(() => (catalog ? groups(filterModels(catalog.models, query, maker), catalog.recommended[side]) : []), [catalog, query, maker, side]);
  const useTyped = () => {
    const id = typed.trim();
    api.call("check_assignment", { route: "R12", model: id, effort: "medium" }).then(() => onPick(id)).catch((e) => setTypedErr(text(e)));
  };
  const keys = (e: KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onClose(); }
    else if (e.key === "Tab" && box.current) {
      const f = [...box.current.querySelectorAll<HTMLElement>("input, select, button:not(:disabled)")];
      const first = f[0], last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    }
  };
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={box} className="gate model-picker" role="dialog" aria-modal="true" aria-labelledby="picker-title" onKeyDown={keys}>
        <h2 id="picker-title" className="gate-title">Choose a model for the {side} side</h2>
        <div className="picker-filters">
          <label className="grow">Search
            <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Name or ID, for example haiku or mistral" />
          </label>
          <label>Maker
            <select value={maker} onChange={(e) => setMaker(e.target.value)}>
              <option value="">All makers</option>
              {catalog && makers(catalog.models).map((m) => <option key={m} value={m}>{m}</option>)}
            </select>
          </label>
        </div>
        {catalog?.offline && <p className="notice warn">OpenRouter could not be reached, so only the usual choices are listed and their prices are unknown.</p>}
        {err && <p className="field-error" role="alert">{err}</p>}
        <div className="picker-list" role="listbox" aria-label="Models">
          {!catalog && !err && <p className="muted">Loading models</p>}
          {catalog && shown.length === 0 && <p className="muted">No model matches. Try fewer words, or type the exact ID below.</p>}
          {shown.map((g) => (
            <section key={g.title} aria-label={g.title}>
              <h3>{g.title}</h3>
              {g.title === "Free" && <p className="muted small">Free models allow 20 requests a minute and 50 a day, or 1,000 a day once the account has bought $10 of credit. A crew makes one request per step.</p>}
              {g.models.map((m) => <Row key={m.id} m={m} on={m.id === value} onPick={onPick} />)}
            </section>
          ))}
        </div>
        <details className="picker-typed">
          <summary>Type a model ID</summary>
          <div className="path-row first">
            <label>Model ID
              <input value={typed} onChange={(e) => { setTyped(e.target.value); setTypedErr(""); }} onKeyDown={(e) => e.key === "Enter" && typed.trim() && useTyped()} placeholder="vendor/model" />
            </label>
            <button disabled={!typed.trim()} onClick={useTyped}>Use this ID</button>
          </div>
          {typedErr && <p className="field-error" role="alert">{typedErr}</p>}
        </details>
        <p className="muted small">The model you choose goes into the plan. Nothing runs, and nothing is charged, until you approve the plan. SproutCrew never moves a step to a paid model on its own.</p>
        <div className="gate-actions">
          <button className="ghost" onClick={onClose}>Cancel <kbd>Esc</kbd></button>
        </div>
      </div>
    </div>
  );
}

function Row({ m, on, onPick }: { m: CatalogModel; on: boolean; onPick: (id: string) => void }) {
  return (
    <button role="option" aria-selected={on} className={`model-row${on ? " on" : ""}`} onClick={() => onPick(m.id)}>
      <span className="model-name"><strong>{m.name}</strong><span className="meta mono">{m.id}</span></span>
      <span className="model-facts">
        <span className={m.free ? "free" : ""}>{priceLine(m)}</span>
        <span className="meta">{[contextLine(m.context_length), m.sees_images ? "Reads images" : ""].filter(Boolean).join(" · ")}</span>
      </span>
    </button>
  );
}
