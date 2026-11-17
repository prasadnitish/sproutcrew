import type { NextDraft } from "./handoff";
import { KeyboardEvent, ReactNode } from "react";
import { choiceIndex } from "./navigation";

export const text = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));

/** A status chip. The word carries the meaning, the color only reinforces it. */
export function Chip({ tone, children }: { tone: "ok" | "warn" | "bad" | "info" | "quiet"; children: ReactNode }) {
  return <span className={`chip ${tone}`}>{children}</span>;
}

/** The one status pill (docs/DESIGN.md §5): the same six states and the same words everywhere. */
export type PillState = "queued" | "running" | "approval" | "done" | "failed" | "cancelled";
export const PILL_WORD: Record<PillState, string> = { queued: "Queued", running: "Running", approval: "Needs approval", done: "Done", failed: "Failed", cancelled: "Cancelled" };
export function StatusPill({ state, title }: { state: PillState; title?: string }) {
  return <span className={`status-pill ${state}`} title={title}>{PILL_WORD[state]}</span>;
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
