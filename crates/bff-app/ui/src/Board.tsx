import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { boardGroups, DecisionItem, firstOpen, Group, Inbox, keyOf, kindOf, moveFrom } from "./queue";
import { shortHash, text } from "./ui";
import BoardDecision from "./BoardDecision";
import { lastActivity } from "./projects";

type Decided = { project: string; at: number; kind: string; what: string; by: string | null; version: string | null };

/** The first screen (DESIGN_SPEC.md §10, A6): every decision across projects in a queue, and the open one beside it,
 *  decided here with the version the person read (§14). */
export default function Board({ onOpen, onNew, onProjects, empty }: { onOpen: (item: DecisionItem, tab?: "work" | "material" | "team" | "share" | "usage") => void; onNew: () => void; onProjects: () => void; empty: boolean }) {
  const [data, setData] = useState<Inbox | null>(null);
  const [error, setError] = useState("");
  const [pick, setPick] = useState<string | null>(null);
  const [shut, setShut] = useState<Record<string, boolean>>({});
  const [queue, setQueue] = useState(true);
  // At narrow widths the queue and the detail take turns; this is the detail's turn.
  const [reading, setReading] = useState(false);
  // Waiting is the queue; Decided is what was decided, read back from each project's journal.
  const [list, setList] = useState<"waiting" | "decided">("waiting");
  const [decided, setDecided] = useState<{ items: Decided[]; errors: { project: string; error: string }[] } | null>(null);
  const [pickedDone, setPickedDone] = useState(0);
  useEffect(() => {
    if (list !== "decided") return;
    let live = true;
    api.call<typeof decided>("decided_list").then((d) => { if (live) setDecided(d); }).catch((e) => live && setError(text(e)));
    return () => { live = false; };
  }, [list]);
  const detail = useRef<HTMLDivElement>(null);
  const [said, setSaid] = useState("");
  const refresh = useCallback(async () => {
    try { setData(await api.call<Inbox>("decision_inbox")); setError(""); }
    catch (e) { setError(text(e)); }
  }, []);
  useEffect(() => {
    void refresh(); const timer = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(timer);
  }, [refresh]);
  const groups = useMemo(() => boardGroups(data?.items ?? []), [data]);
  const all = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const open = all.find((i) => keyOf(i) === pick) ?? firstOpen(groups);
  // A row picked from the keyboard takes focus once it has rendered as the current one.
  const focusNext = useRef(false);
  const choose = useCallback((item: DecisionItem | null, focus = false) => {
    if (!item) return;
    focusNext.current = focus;
    setPick(keyOf(item)); setReading(true);
    setShut((s) => ({ ...s, [kindOf(item.kind).group]: false }));
  }, []);
  useEffect(() => {
    if (!focusNext.current || !pick) return;
    focusNext.current = false;
    document.getElementById(`bd-${pick}`)?.focus();
  }, [pick]);
  // J and K move, O opens the item's project, [ hides the queue, E opens or closes every section. Single keys do
  // nothing while a field, a select or a dialog has focus.
  const now = useRef({ groups, open, queue, list });
  now.current = { groups, open, queue, list };
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || document.querySelector("dialog[open]")) return;
      const t = e.target as HTMLElement;
      if (/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || t.isContentEditable) return;
      const { groups: g, open: o, queue: q, list: l } = now.current;
      if (l !== "waiting" && (e.key.toLowerCase() === "j" || e.key.toLowerCase() === "k" || e.key.toLowerCase() === "o")) return;
      const key = e.key.toLowerCase();
      if (key === "j" || key === "k") { e.preventDefault(); choose(moveFrom(g, o, key === "j" ? 1 : -1), true); }
      else if (key === "o" && o) { e.preventDefault(); onOpen(o); }
      else if (e.key === "[") { e.preventDefault(); setQueue(!q); }
      else if (key === "e") {
        const sections = [...(detail.current?.querySelectorAll<HTMLDetailsElement>("details.sect") ?? [])];
        const expand = sections.some((d) => !d.open);
        sections.forEach((d) => { d.open = expand; });
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [choose, onOpen]);

  // First launch: no project yet, and so nothing can be waiting.
  if (empty && data && data.items.length === 0 && data.errors.length === 0) return (
    <div className="bd-empty">
      <h2>Nothing runs until you approve it</h2>
      <p className="lede">Every plan, change, write and finished result waits for you here.</p>
      <button className="primary" onClick={onNew}>Start a project <kbd>⌘N</kbd></button>
    </div>
  );
  const unread = data?.errors ?? [];
  return (
    <div className={`bd${queue ? "" : " bd-noqueue"}${reading ? " bd-reading" : ""}`}>
      {queue && <section className="bd-queue" aria-label={list === "waiting" ? "Waiting for you" : "Decided"}>
        <div className="bd-tools"><div className="seg" role="group" aria-label="Which list">
          <button aria-pressed={list === "waiting"} onClick={() => { setList("waiting"); setReading(false); }}>Waiting <span className="nav-count">{all.length}</span></button>
          <button aria-pressed={list === "decided"} onClick={() => { setList("decided"); setReading(false); setPickedDone(0); }}>Decided</button>
        </div></div>
        {list === "decided" ? <DecidedList data={decided} at={pickedDone} pick={(i) => { setPickedDone(i); setReading(true); }} /> : <>
        {api.mocked && !new URLSearchParams(window.location.search).has("board") && <p className="bd-note muted">Browser preview. Add ?board to the address to see a sample queue.</p>}
        {error && <p className="bd-note" role="alert">{error}</p>}
        {unread.length > 0 && <p className="bd-note" role="status">Not on the board, because {unread.length === 1 ? "it" : "they"} could not be read: {unread.map((u) => `${u.project} (${u.error})`).join(", ")}. The list may be missing decisions.</p>}
        {!data && !error && <p className="bd-note muted" role="status">Reading project decisions…</p>}
        {data && all.length === 0 && <div className="bd-note"><strong>Nothing is waiting for you.</strong><p className="muted">New plans, changes and results show up here.</p><button className="link" onClick={onProjects}>Open your projects</button></div>}
        {groups.map((g, gi) => <QueueGroup key={g.id} g={g} open={shut[g.id] === undefined ? gi === 0 || (!!open && kindOf(open.kind).group === g.id) : !shut[g.id]} toggle={(o) => setShut((s) => ({ ...s, [g.id]: !o }))} current={open} choose={choose} />)}
        </>}
      </section>}
      <section className="bd-detail" aria-label="The open decision" ref={detail}>
        {list === "decided"
          ? (decided?.items[pickedDone] ? <DecidedDetail d={decided.items[pickedDone]} back={() => setReading(false)} /> : <div className="bd-scroll"><p className="muted">Nothing has been decided yet.</p></div>)
          : open ? <BoardDecision key={keyOf(open)} item={open} onOpen={onOpen} back={() => setReading(false)} queue={queue} showQueue={() => setQueue(true)}
              onDecided={(line) => {
                // After a decision the next item is chosen, and the status message names what was done and what is next.
                const next = moveFrom(groups, open, 1);
                const after = next && keyOf(next) !== keyOf(open) ? next : moveFrom(groups, open, -1);
                if (after && keyOf(after) !== keyOf(open)) choose(after, true);
                if (line) setSaid(`${line}${after && keyOf(after) !== keyOf(open) ? ` Next: ${after.title}.` : ""}`);
                void refresh();
              }} /> : data && <div className="bd-scroll"><p className="muted">Nothing is open.</p></div>}
      </section>
      <p className="vh" role="status" aria-live="polite">{said}</p>
    </div>
  );
}

function QueueGroup({ g, open, toggle, current, choose }: { g: Group; open: boolean; toggle: (open: boolean) => void; current: DecisionItem | null; choose: (i: DecisionItem) => void }) {
  return (
    <details className="bd-group" open={open} onToggle={(e) => { const o = (e.currentTarget as HTMLDetailsElement).open; if (o !== open) toggle(o); }}>
      <summary><span className="sect-title">{g.label}</span>{!open && <span className="sect-value">{g.items[0].project}</span>}<span className={`bd-count ${g.tone}`} aria-label={`${g.items.length} waiting`}>{g.items.length}</span></summary>
      <ul>{g.items.map((i) => {
        const k = kindOf(i.kind);
        const on = !!current && keyOf(current) === keyOf(i);
        return <li key={keyOf(i)}><button id={`bd-${keyOf(i)}`} className="bd-row" aria-current={on ? "true" : undefined} onClick={() => choose(i)}>
          <span className={`bd-tile ${g.tone}`} aria-hidden="true">{k.glyph}</span>
          <span className="bd-title">{i.title}</span>
          <span className="bd-meta">{k.word} · {i.project}{i.step_title ? ` · ${i.step_title}` : ""}</span>
        </button></li>;
      })}</ul>
    </details>
  );
}

function DecidedList({ data, at, pick }: { data: { items: Decided[]; errors: { project: string; error: string }[] } | null; at: number; pick: (i: number) => void }) {
  if (!data) return <p className="bd-note muted" role="status">Reading what was decided…</p>;
  return (
    <>
      {data.errors.length > 0 && <p className="bd-note" role="status">Not listed, because {data.errors.length === 1 ? "it" : "they"} could not be read: {data.errors.map((u) => u.project).join(", ")}.</p>}
      {data.items.length === 0 ? <p className="bd-note muted">Nothing has been decided yet.</p> : <ul className="bd-decided">{data.items.map((d, i) => (
        <li key={`${d.project}:${d.kind}:${d.at}:${i}`}><button className="bd-row" aria-current={i === at ? "true" : undefined} onClick={() => pick(i)}>
          <span className="bd-tile quiet" aria-hidden="true">✓</span>
          <span className="bd-title">{d.what}</span>
          <span className="bd-meta">{d.project} · {lastActivity(d.at)}</span>
        </button></li>
      ))}</ul>}
    </>
  );
}

function DecidedDetail({ d, back }: { d: Decided; back: () => void }) {
  return (
    <div className="bd-scroll">
      <div className="bd-crumb"><button className="link bd-back" onClick={back}>Back to the list</button><span>{d.project}</span></div>
      <h2 className="bd-h">{d.what}</h2>
      <dl className="bd-facts">
        <dt>Project</dt><dd>{d.project}</dd>
        <dt>When</dt><dd>{new Date(d.at * 1000).toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short" })}</dd>
        <dt>By</dt><dd>{d.by ?? "Not recorded"}</dd>
        {d.version && <><dt>Covered</dt><dd><span className="mono" title={d.version}>{shortHash(d.version)}</span></dd></>}
      </dl>
    </div>
  );
}
