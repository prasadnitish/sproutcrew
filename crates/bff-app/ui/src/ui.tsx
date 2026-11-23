import type { NextDraft } from "./handoff";
import { KeyboardEvent, ReactNode, useEffect, useId, useRef, useState } from "react";
import { choiceIndex } from "./navigation";

export const text = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));

/** A status chip. The word carries the meaning, the color only reinforces it. */
export function Chip({ tone, children }: { tone: "ok" | "warn" | "bad" | "info" | "quiet"; children: ReactNode }) {
  return <span className={`chip ${tone}`}>{children}</span>;
}

/** The one status pill (docs/DESIGN.md §5): the same six states and the same words everywhere. */
export type PillState = "queued" | "running" | "approval" | "done" | "failed" | "cancelled";
export const PILL_WORD: Record<PillState, string> = { queued: "Queued", running: "Running", approval: "Needs approval", done: "Done", failed: "Failed", cancelled: "Cancelled" };
export function StatusPill({ state, title, word }: { state: PillState; title?: string; word?: string }) {
  return <span className={`status-pill ${state}`} title={title}>{word ?? PILL_WORD[state]}</span>;
}

/** A colored monogram chip for who does a step: a muted tint, never the accent and never a status color. */
export function Monogram({ side, name }: { side: "writing" | "checking"; name: string }) {
  return <span className={`monogram ${side}`} aria-hidden="true" title={name}>{name.charAt(0).toUpperCase()}</span>;
}

export function Tabs<T extends string>({ items, value, onChange, label, panelId }: { items: { id: T; label: string; badge?: string }[]; value: T; onChange: (v: T) => void; label: string; panelId?: string }) {
  return (
    <div className="tabs" role="tablist" aria-label={label}>
      {items.map((t, index) => (
        <button key={t.id} id={panelId ? `${panelId}-${t.id}` : undefined} role="tab" aria-controls={value === t.id ? panelId : undefined} aria-selected={value === t.id} tabIndex={value === t.id ? 0 : -1} className="tab" onClick={() => onChange(t.id)} onKeyDown={(e) => {
          const next = choiceIndex(e.key, index, items.length);
          if (next < 0) return;
          e.preventDefault();
          onChange(items[next].id);
          (e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next])?.focus();
        }}>
          {t.label}
          {t.badge && <span className="tab-badge">{t.badge}</span>}
        </button>
      ))}
    </div>
  );
}

export function radioKeys(event: KeyboardEvent<HTMLDivElement>) {
  const options = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]:not(:disabled)')];
  const next = choiceIndex(event.key, options.indexOf(event.target as HTMLButtonElement), options.length);
  if (next < 0) return;
  event.preventDefault();
  options[next].click();
  options[next].focus();
}

/** An empty native drag area keeps all content below macOS window controls. */
export function WindowChrome() {
  return <div className="window-chrome" data-tauri-drag-region aria-hidden="true" />;
}

export function SectionIntro({ title, children }: { title: string; children: ReactNode }) {
  return <header className="section-intro"><h2>{title}</h2><p className="muted">{children}</p></header>;
}

/** A titled group. One heading size, with an optional line of help under it. */
export function Panel({ title, help, children, aside }: { title: string; help?: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <section className="panel">
      <div className="panel-head">
        <div>
          <h2>{title}</h2>
          {help && <p className="muted">{help}</p>}
        </div>
        {aside}
      </div>
      {children}
    </section>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty">{children}</p>;
}

/** Plain names for the providers. The route ids stay in the plan file and are not shown. */
export const PROVIDERS: [string, string][] = [
  ["R1", "Codex"],
  ["R2", "Claude"],
  ["R3", "Anthropic API"],
  ["R5", "OpenAI API"],
  ["R8", "xAI API"],
  ["R9", "Gemini API"],
  ["R10", "DeepSeek API"],
  ["R11", "Kimi API"],
  ["R12", "OpenRouter API"],
  ["R13", "Your own AI service"],
];
export const provider = (route: string) => PROVIDERS.find(([id]) => id === route)?.[1] ?? route;
export const EFFORTS: [string, string][] = [["low", "Quick"], ["medium", "Balanced"], ["high", "Thorough"]];
export const effort = (e: string) => EFFORTS.find(([id]) => id === e)?.[1] ?? e;
const ROLES: Record<string, string> = { author: "Writer", builder: "Builder", reviewer: "Reviewer", writer: "Writer" };
// The program names its own roles. State sends the names for the roles in the open plan, and they win over the
// fallbacks above, so a team of specialists reads as names and not ids.
let LABELS: Record<string, string> = {};
export const setRoleLabels = (info: Record<string, { label: string }> | undefined) => {
  LABELS = Object.fromEntries(Object.entries(info ?? {}).map(([k, v]) => [k, v.label]));
};
export const roleName = (r: string) => LABELS[r] ?? ROLES[r] ?? r.charAt(0).toUpperCase() + r.slice(1);

export const n = (v: number) => v.toLocaleString("en-US");

export function secs(s: number) {
  return s < 60 ? `${s.toFixed(s < 10 ? 1 : 0)}s` : `${Math.floor(s / 60)}m ${String(Math.round(s % 60)).padStart(2, "0")}s`;
}

import type { EngineMode, RunEvent, State } from "./api";

/** What every tab gets: the current state, the engine, and how to run something with a busy label and refresh. */
export type Ctx = {
  s: State;
  engine: EngineMode;
  busy: string;
  /** What the run bar is doing right now. Empty once Stop is pressed, though the stopped step may still be in
   *  flight, so the boxes that ask for a change open at once while every other control keeps to `busy`. */
  askBusy: string;
  /** Set by Stop and read by the run, so Stop works from any tab. */
  stopFlag: { current: boolean };
  /** A run of steps is going or finishing: busy, between two steps, or stopped with a step still in flight. */
  running: boolean;
  /** The person pressed Stop and the step that was running has not returned yet. */
  stopping: boolean;
  guard: (f: () => Promise<unknown>) => Promise<void>;
  track: (e: RunEvent) => void;
  /** Reads the project again, so progress is current while a long run is still going. */
  refresh: () => Promise<void>;
  /** The plan has edits that are not saved yet. */
  dirty: boolean;
  goTo: (tab: "work" | "material" | "team" | "share" | "usage") => void;
  /** Opens the new project screen, ready to continue from this project. */
  onContinue: (draft: NextDraft) => void;
  /** Opens another project. */
  openProject: (name: string) => void;
};

/** A mark that something needs the person (DESIGN_SPEC.md §9): it starts with "!", and its tone agrees with its word. */
export function Flag({ tone = "warn", children }: { tone?: "warn" | "bad" | "info"; children: ReactNode }) {
  return <b className={`flag ${tone}`}><span aria-hidden="true">! </span>{children}</b>;
}

/** One section (DESIGN_SPEC.md §9). Closed, it still says its value; a flagged section opens by itself. The summary
 *  holds no heading, so screen readers keep it as one button. */
export function Section({ title, value, flag, tone, open, children }: { title: ReactNode; value?: ReactNode; flag?: string; tone?: "warn" | "bad" | "info"; open?: boolean; children: ReactNode }) {
  return (
    <details className="sect" open={open || !!flag}>
      <summary><span className="sect-title">{title}</span>{flag && <Flag tone={tone}>{flag}</Flag>}{value !== undefined && <span className="sect-value">{value}</span>}</summary>
      <div className="sect-body">{children}</div>
    </details>
  );
}

/** A hash as the screens show it: two groups of four (DESIGN_SPEC.md §5). The full value goes in a title or a section. */
export function shortHash(h: string): string {
  const x = h.replace(/[^0-9a-f]/gi, "").slice(0, 8);
  return x.length < 8 ? x : `${x.slice(0, 4)} ${x.slice(4)}`;
}

/** A native modal dialog (DESIGN_SPEC.md §9). It opens with showModal, so the page behind is inert, Esc closes it and
 *  decides nothing, and focus returns to the control that opened it. The safe choice inside takes `data-autofocus`. */
export function Dialog({ title, onClose, children, describe }: { title: string; onClose: () => void; children: ReactNode; describe?: string }) {
  const ref = useRef<HTMLDialogElement>(null);
  const id = useId();
  const closing = useRef(onClose);
  closing.current = onClose;
  // Opened on mount and closed on unmount. A close that is followed at once by opening again (React runs effects
  // twice in development) is not the person closing it, so it is ignored above.
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
    // React does not write `autofocus` to the page, so the safe choice is marked `data-autofocus` and focused here.
    d?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    return () => { if (d?.open) d.close(); };
  }, []);
  return (
    <dialog ref={ref} className="dlg" aria-labelledby={id} aria-describedby={describe} onClose={() => { if (!ref.current?.open) closing.current(); }}>
      <h2 id={id} className="dlg-title">{title}</h2>
      {children}
    </dialog>
  );
}

/** Confirm (DESIGN_SPEC.md §9): every remove and delete asks first, with focus on Keep it. Esc keeps it too. */
export function Confirm({ title, line, action, onKeep, onConfirm }: { title: string; line: string; action: string; onKeep: () => void; onConfirm: () => void }) {
  return (
    <Dialog title={title} onClose={onKeep}>
      <p>{line}</p>
      <div className="dlg-foot">
        <button data-autofocus onClick={onKeep}>Keep it</button>
        <span className="sp" />
        <button onClick={onConfirm}>{action}</button>
      </div>
    </Dialog>
  );
}

type Asked = { title: string; line: string; action: string; run: () => void };
/** A Confirm a screen opens with `ask(...)` and renders where it likes. */
export function useConfirm(): [(a: Asked) => void, ReactNode] {
  const [a, setA] = useState<Asked | null>(null);
  const el = a ? <Confirm title={a.title} line={a.line} action={a.action} onKeep={() => setA(null)} onConfirm={() => { setA(null); a.run(); }} /> : null;
  return [setA, el];
}
