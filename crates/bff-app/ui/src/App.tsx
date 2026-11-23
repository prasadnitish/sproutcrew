import Memory from "./Memory";
import PolicyPage from "./PolicyPage";
import UsagePage, { todayLine } from "./UsagePage";
import { groupOf, Overview as PolicyOverview } from "./policyFit";
import { total, dollarsLine, projectSpendLine, UsageRow } from "./usage";
import Board from "./Board";
import { kindOf, type DecisionItem, type Inbox } from "./queue";
import { headline, audience } from "./positioning";
import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, AttentionStatus, EngineMode, Info, Plan, RunEvent, Spend, SpendStatus, State } from "./api";
import { Hud } from "./Gauge";
import { GuardStrip } from "./Policy";
import Gate, { Skeleton } from "./Gate";
import CostTab from "./Cost";
import NewProject, { RestartDraft } from "./NewProject";
import CommandPalette, { PaletteItem } from "./CommandPalette";
import Settings from "./Settings";
import ShareTab from "./Share";
import Splash from "./Splash";
import BrandMark from "./BrandMark";
import Stress from "./Stress";
import TeamTab, { MaterialTab } from "./Team";
import { Ctx, PillState, Section, setRoleLabels, shortHash, Tabs, text, WindowChrome } from "./ui";
import { LinkedLine } from "./Continue";
import type { NextDraft } from "./handoff";
import { filterProjects, lastActivity, restartName } from "./projects";
import WorkTab from "./Work";
import { runner, stopFlag } from "./runner";
import { missing } from "./gaps";
import { runningIds } from "./graph";
import { RunTracker } from "./runTracker";
import { idleLine, nextAction, noteLine, RUN_GOING, spentSince, workingLine } from "./status";
import { count, needInput } from "./plural";

type TabId = "work" | "material" | "team" | "share" | "usage";
const TABS: { id: TabId; label: string }[] = [
  { id: "work", label: "Plan & work" }, { id: "material", label: "Sources" }, { id: "team", label: "Your crew" }, { id: "share", label: "Results & sharing" }, { id: "usage", label: "Usage & limits" },
];

type Overview = { name: string; brief: string; steps_done: number; steps_total: number; steps_failed?: number; details_missing?: number; approved: boolean; reviewed?: boolean; archived: boolean; outcome?: string | null; last_at?: number | null };

/** The sidebar shows one project's state as the same pill words a step uses. */
function projectState(o: Overview, open: boolean, busy: boolean): { state: PillState; word: string; pill?: string } {
  if (open && busy) return { state: "running", word: "Running" };
  if (o.steps_total === 0) return { state: "queued", word: "No plan yet" };
  if (o.steps_failed) return { state: "failed", word: `${o.steps_failed} step${o.steps_failed === 1 ? "" : "s"} failed` };
  // A finished draft that still lacks details only the person can supply is a draft, and the sidebar says so.
  if (o.steps_done === o.steps_total && o.details_missing) return { state: "approval", word: `Draft · ${missing(o.details_missing)}`, pill: "Draft" };
  // Finished work waits for the person to review it (DESIGN_SPEC.md §4: Not reviewed is its own state).
  if (o.steps_done === o.steps_total) return o.reviewed === false ? { state: "done", word: "Not reviewed" } : { state: "done", word: "Finished" };
  if (!o.approved) return { state: "approval", word: "Needs approval" };
  return { state: "queued", word: `${o.steps_done} of ${o.steps_total} steps done` };
}

/** The run that is going, for the page bar of every screen but its own project's (DESIGN_SPEC.md §9, Run pill). */
type RunInfo = { name: string; done: number; total: number; open: () => void; stop: () => void } | null;
const RunContext = createContext<RunInfo>(null);

function RunPill() {
  const run = useContext(RunContext);
  if (!run) return null;
  return (
    <div className="run-pill-group">
      <button className="run-pill" onClick={run.open} title={`Open ${run.name}`}><span aria-hidden="true">● </span>{run.name}{run.total ? `, step ${Math.min(run.done + 1, run.total)} of ${run.total}` : ""}<span className="vh"> is running</span></button>
      <button onClick={run.stop} title="Stop the run (⌘.)">Stop <kbd>⌘.</kbd></button>
    </div>
  );
}

function Toolbar({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <header className="toolbar" data-tauri-drag-region>
      <button className="quiet show-sidebar" onClick={() => window.dispatchEvent(new Event("bff:sidebar"))} title="Show the sidebar (⌘\)" aria-label="Show the sidebar">Show sidebar</button>
      <button className="quiet mobile-home" onClick={() => window.dispatchEvent(new Event("bff:home"))}>Projects</button>
      <h1 data-tauri-drag-region>{title}</h1>
      <RunPill />
      {children && <div className="toolbar-actions">{children}</div>}
    </header>
  );
}

const GLYPH: Record<PillState, string> = { running: "●", approval: "◆", failed: "×", done: "✓", queued: "○", cancelled: "■" } as Record<PillState, string>;

/** The sidebar plane (DESIGN_SPEC.md §10): Board with its count, Projects, then Recent and More, both closed at first.
 *  It can be hidden with the button in the toolbar or ⌘\, and the choice is remembered. Its width is too. */
function Sidebar({ rows, current, busy, view, decisions, looser, today, onBoard, onHome, onPolicy, onUsage, onOpen, onNew, onMemory, onSettings, onHide, width, setWidth }: {
  rows: Overview[] | null; current: string | null; busy: boolean; view: string; decisions: number | null; looser: number; today: string; onBoard: () => void; onHome: () => void; onPolicy: () => void; onUsage: () => void; onOpen: (n: string) => void; onNew: () => void; onMemory: () => void; onSettings: () => void; onHide: () => void; width: number; setWidth: (w: number) => void;
}) {
  const drag = useRef(false);
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    const move = (e: PointerEvent) => { if (drag.current) setWidth(Math.max(200, Math.min(360, e.clientX))); };
    const up = () => { drag.current = false; setDragging(false); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); };
  }, [setWidth]);
  // The open project first, then the most recently active, four in all.
  const recent = [...(rows ?? [])].sort((x, y) => (x.name === current ? -1 : y.name === current ? 1 : (y.last_at ?? 0) - (x.last_at ?? 0))).slice(0, 4);
  const here = (v: string) => (view === v ? "page" : undefined);
  return (
    <>
      <nav className="sidebar" aria-label="Main">
        <div className="sidebar-top" data-tauri-drag-region>
          <span className="sidebar-title brand-lockup" data-tauri-drag-region><BrandMark />SproutCrew</span>
          <button className="quiet" onClick={onHide} title="Hide the sidebar (⌘\)" aria-label="Hide the sidebar">Hide</button>
        </div>
        <ul className="nav-list">
          <li><button className="nav-item" aria-current={here("board")} onClick={onBoard}>Board{decisions !== null && <span className="nav-count" aria-label={needInput(decisions)}>{decisions}</span>}</button></li>
          <li><button className="nav-item" aria-current={here("home")} onClick={onHome}>Projects</button></li>
          <li><button className="nav-item" aria-current={here("policy")} onClick={onPolicy}>Policy{looser > 0 && <span className="nav-count warn" aria-label={`${looser} looser than your defaults`}>! {looser}</span>}</button></li>
        </ul>
        <div className="sidebar-list">
          {recent.length > 0 && <details className="nav-group">
            <summary>Recent</summary>
            <ul className="nav-list">{recent.map((r) => {
              const st = projectState(r, r.name === current, busy && r.name === current);
              return <li key={r.name}><button className="nav-item" aria-current={view === "project" && r.name === current ? "page" : undefined} title={`${r.name}: ${st.word}`} onClick={() => onOpen(r.name)}><span className="nav-glyph" aria-hidden="true">{GLYPH[st.state] ?? "○"}</span><span className="nav-name">{r.name}</span><span className="vh">, {st.word}</span></button></li>;
            })}</ul>
          </details>}
          <details className="nav-group">
            <summary>More</summary>
            <ul className="nav-list">
              <li><button className="nav-item" aria-current={here("new")} onClick={onNew}>New project <kbd>⌘N</kbd></button></li>
              <li><button className="nav-item" aria-current={here("memory")} onClick={onMemory}>Memory</button></li>
              <li><button className="nav-item" aria-current={here("settings")} onClick={onSettings}>Settings</button></li>
            </ul>
          </details>
        </div>
        <button className="nav-item nav-foot" aria-current={here("usage")} onClick={onUsage}>Usage{today && <span className="nav-today">{today}</span>}</button>
      </nav>
      <div className={`sash${dragging ? " drag" : ""}`} role="separator" aria-orientation="vertical" aria-label="Resize the sidebar" aria-valuenow={width} tabIndex={0}
        onPointerDown={(e) => { e.preventDefault(); drag.current = true; setDragging(true); }}
        onKeyDown={(e) => { if (e.key === "ArrowLeft") setWidth(Math.max(200, width - 16)); if (e.key === "ArrowRight") setWidth(Math.min(360, width + 16)); }} />
    </>
  );
}


function Project({ onChanged, onRestart, onContinue, openProject, onBoard, onPolicy }: { onChanged: (busy: boolean) => void; onRestart: (state: State) => void; onContinue: (draft: NextDraft) => void; openProject: (name: string) => void; onBoard: () => void; onPolicy: () => void }) {
  const root = useRef<HTMLDivElement>(null);
  const [s, setS] = useState<State | null>(null);
  const [draft, setDraft] = useState<Plan | null>(null);
  // Another screen can ask for a section to open first, once.
  const [tab, setTab] = useState<TabId>(() => {
    let t: string | null = null;
    try { t = sessionStorage.getItem("sproutcrew:tab"); sessionStorage.removeItem("sproutcrew:tab"); } catch { /* no storage */ }
    return TABS.find((x) => x.id === t)?.id ?? "work";
  });
  const [busy, setBusy] = useState("");
  // The last progress line of the run, such as a wait for a provider. The status line is made from `busy`.
  const [note, setNote] = useState("");
  // The person pressed Stop and the step that was running has not returned yet: it finishes, and nothing new starts.
  const [stopping, setStopping] = useState(false);
  const [msg, setMsg] = useState("");
  const [confirmRestart, setConfirmRestart] = useState(false);
  // The run of the plan's steps, wherever it was started: this page, or the board.
  const [autoRunning, setAutoRunning] = useState(runner.project !== null);
  const [tokens, setTokens] = useState<Spend | null>(null);
  // Each tab comes back where it was left. A tab not opened yet starts at its top, just under the tabs.
  const scrolled = useRef<Partial<Record<TabId, number>>>({});
  const showing = useRef(tab);
  const tabsBar = useRef<HTMLDivElement>(null);
  const tabBody = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Scrolling does not bubble, and the pane may not exist yet while the project loads, so this listens from the top.
    const note = (e: Event) => { if (e.target instanceof Element && e.target.matches(".content")) scrolled.current[showing.current] = e.target.scrollTop; };
    document.addEventListener("scroll", note, { capture: true, passive: true });
    return () => document.removeEventListener("scroll", note, { capture: true });
  }, []);
  useLayoutEffect(() => {
    showing.current = tab;
    const pane = root.current?.closest(".content");
    if (!pane) return;
    const kept = scrolled.current[tab];
    if (kept !== undefined) { pane.scrollTop = kept; return; }
    const top = (tabBody.current?.getBoundingClientRect().top ?? 0) - pane.getBoundingClientRect().top + pane.scrollTop;
    pane.scrollTop = Math.min(pane.scrollTop, Math.max(0, top - (tabsBar.current?.getBoundingClientRect().bottom ?? 0) + pane.getBoundingClientRect().top));
  }, [tab]);

  const refresh = useCallback(async () => {
    const st = await api.call<State>("get_state", { engine });
    setRoleLabels(st.role_info);
    setS(st);
    api.call<SpendStatus>("spend_status").then((x) => setTokens(x.tokens)).catch(() => setTokens(null));
    setDraft(st.plan ? structuredClone(st.plan) : null);
    onChanged(false);
  }, [engine]);
  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => { onChanged(!!busy || autoRunning || stopping); }, [busy, autoRunning, stopping]);
  // A run is going that this page did not start: the project was left mid-run and opened again. The page shows it and
  // keeps every control that starts work shut, so the same step is not run twice.
  const elsewhere = !!s?.run_active && !busy && !autoRunning && !stopping;
  // The run cannot report back to this page, so the project is read again until it is over.
  useEffect(() => {
    if (!elsewhere) return;
    const t = setInterval(() => void refresh(), 2000);
    return () => clearInterval(t);
  }, [elsewhere, refresh]);
  // What the Stop handler needs, kept current without re-adding the handler.
  const live = useRef({ busy: "", auto: false, elsewhere: false });
  live.current = { busy, auto: autoRunning, elsewhere };
  // Each piece of work in flight, so a run that was stopped is finished only when its own work has returned.
  const work = useRef(new RunTracker());
  const since = useRef(0);
  // The command palette goes to a section of this project.
  useEffect(() => {
    const go = (e: Event) => { const t = (e as CustomEvent<TabId>).detail; if (TABS.some((x) => x.id === t)) setTab(t); };
    const key = (e: KeyboardEvent) => { if (e.metaKey && e.key === ".") { e.preventDefault(); window.dispatchEvent(new Event("bff:stop")); } };
    const stop = () => {
      const { busy: what, auto, elsewhere: left } = live.current;
      // A run this page did not start, because the project was left and opened again: its loop reads the same flag.
      if (left) { stopFlag.current = true; void api.call("stop_run", { steps: true }).catch(() => undefined); return; }
      if ((!what && !auto) || work.current.stopping) return;
      // A run of the plan's steps is left at once: it stops being "running", and the step in flight finishes quietly.
      const steps = auto || runningIds(what).length > 0;
      if (steps) {
        stopFlag.current = true;
        if (work.current.stop()) { setStopping(true); setBusy(""); setNote(""); }
      }
      void api.call("stop_run", { steps }).catch(() => undefined);
    };
    window.addEventListener("bff:tab", go);
    window.addEventListener("bff:stop", stop);
    window.addEventListener("keydown", key);
    return () => { window.removeEventListener("bff:tab", go); window.removeEventListener("bff:stop", stop); window.removeEventListener("keydown", key); };
  }, []);

  const track = (e: RunEvent) => {
    // The step that was stopped may still report as it ends. That must not bring the status line back.
    if (work.current.stopping && e.event !== "started") return;
    if (e.event === "started") {
      if (!since.current) since.current = Math.floor(Date.now() / 1000);
      setBusy(`${e.what} on ${e.engine}`);
      setNote("");
    } else if (e.event === "progress") {
      setNote(e.line);
      // A rate-limit wait while SproutCrew is in the background gets one notification; the app limits how often.
      if (e.line.includes("Retrying in")) void api.call("notify_run", { outcome: "waiting" }).catch(() => undefined);
    } else {
      setBusy("");
      setNote("");
    }
  };
  const guard = async (f: () => Promise<unknown>) => {
    const mine = work.current.start();
    setMsg("");
    try { await f(); } catch (e) { setMsg(text(e)); } finally {
      const { idle, released } = work.current.finish(mine);
      if (idle) { setBusy(""); setNote(""); since.current = 0; }
      if (released) setStopping(false);
      await refresh();
    }
  };

  // The run is followed here while this page is open: its events make the status line, and the project is read
  // again after each step. A page opened partway through a run picks up the step in flight.
  const follow = useRef({ track, refresh });
  follow.current = { track, refresh };
  useEffect(() => {
    if (runner.started) follow.current.track(runner.started);
    return runner.subscribe({
      event: (e) => follow.current.track(e),
      change: (p) => setAutoRunning(p !== null),
      wave: () => follow.current.refresh(),
    });
  }, []);

  // This project's items on the board, read again when its approval or results change.
  const [mine, setMine] = useState<DecisionItem[]>([]);
  useEffect(() => {
    if (!s?.name) return;
    let live = true;
    api.call<Inbox>("decision_inbox").then((v) => live && setMine(v.items.filter((i) => i.project === s.name))).catch(() => live && setMine([]));
    return () => { live = false; };
  }, [s?.name, s?.gate, s?.artifacts.length, s?.policy?.pending.length]);

  if (!s) return <div className="content-inner"><Toolbar title="Opening project" /><Skeleton label="Opening the project" lines={4} /></div>;
  const dirty = !!draft && !!s.plan && JSON.stringify(draft) !== JSON.stringify(s.plan);
  const running = !!busy || autoRunning || stopping || elsewhere;
  // While the stopped step is still in flight every control that starts work or changes the plan stays shut. Only the
  // boxes that ask for a change open at once, and they read `askBusy`.
  const ctx: Ctx = { s, engine, busy: busy || (stopping ? "Finishing the step that was running" : elsewhere ? RUN_GOING : ""), askBusy: busy, running, stopping, stopFlag, guard, track, refresh, dirty, goTo: setTab, onContinue, openProject };
  const stepCount = s.plan?.stages.length ?? 0;
  const doneCount = s.plan ? s.plan.stages.filter((st) => s.artifacts.some((a) => a.name === st.id)).length : 0;
  const approved = s.gate === "allowed";
  const failedAny = !!s.plan && s.plan.stages.some((st) => s.failures?.[st.id]);
  const stopped = !!s.stopped && approved && doneCount < stepCount && !running;
  const next = nextAction({ steps: stepCount, done: doneCount, approved });
  const stateWord = running ? "Running" : failedAny ? "A step failed" : stopped ? "Stopped by you" : stepCount === 0 ? "No plan yet" : doneCount === stepCount ? (s.result_review?.reviewed ? "Finished" : "Finished, not reviewed") : approved ? "Approved" : s.gate === "plan_changed" ? "Changed since approval" : "Needs approval";
  const open = s.review ? Object.values(s.review).reduce((k, r) => k + r.findings.filter((f) => !f.decision).length, 0) : 0;

  return (
    <div className="content-inner project" ref={root}>
      <Toolbar title={s.name}>
        <button className="quiet" disabled={running} title={running ? "Finish or stop the current work before starting over" : "Return to setup with your brief and sources"} onClick={() => setConfirmRestart(true)}>Start over</button>
      </Toolbar>

      {confirmRestart && <Gate title="Start over with this brief?" approveLabel="Open fresh setup" rejectLabel="Keep working" busy={running}
        what={<><p>Go back to the first setup screen with your brief and {s.sources.length} {s.sources.length === 1 ? "source" : "sources"}. You can change the outcome, edit the brief, and leave out sources.</p><p>Your original project and all its results stay available. The fresh project has no previous results or approvals. Shared API usage and limits still apply; AI-assisted planning also uses provider allowance.</p><p className="muted">Nothing is created until you finish setup. Cancelling leaves the original project unchanged.</p></>}
        onReject={() => setConfirmRestart(false)} onApprove={() => { setConfirmRestart(false); onRestart(s); }} />}

      <div className="project-status">
        {busy ? (
          <div className="busy-block" role="status" aria-live="polite">
            <p className="busy"><span className="spinner" aria-hidden="true" />{workingLine(busy, runningIds(busy), s.plan?.stages ?? [], spentSince(s.usage_rows, since.current))}</p>
            {noteLine(note) && <p className="muted small">{noteLine(note)}</p>}
            <div className="stream-strip" aria-hidden="true"><span /></div>
          </div>
        ) : elsewhere ? (
          <div className="busy-block" role="status" aria-live="polite">
            <p className="busy"><span className="spinner" aria-hidden="true" />{RUN_GOING} It carries on while you are here, and this page follows it.</p>
          </div>
        ) : tab !== "work" && (
          // The Plan tab says this itself in its run bar, so the line here is for the other tabs.
          <p className="status-line">
            <span role="status">{stopping ? "Finishing the step that was running…" : idleLine({ steps: stepCount, done: doneCount, approved, failed: failedAny, stopped })}</span>
            {!stopping && !running && <button className="link" onClick={() => setTab(next.tab)}>{next.label}</button>}
          </p>
        )}
      </div>
      {msg && (
        <div className="notice error" role="alert">
          <p>{msg}</p>
          <button className="ghost dismiss" onClick={() => setMsg("")}>Dismiss</button>
        </div>
      )}

      <LinkedLine ctx={ctx} />

      {/* What this project has on the board, then its state and limit (DESIGN_SPEC.md §10, Project). */}
      <div className="project-head">
        {mine.length > 0 && <Section title="On the board" flag={`${mine.length} waiting`}>
          <ul className="plain">{mine.map((i) => <li key={`${i.kind}:${i.id}`}><span aria-hidden="true">{kindOf(i.kind).glyph} </span>{i.title} <span className="muted">· {kindOf(i.kind).word}</span></li>)}</ul>
          <button className="link" onClick={onBoard}>Decide on the board</button>
        </Section>}
        <Section title={stateWord} value={`${doneCount} of ${stepCount} ${stepCount === 1 ? "step" : "steps"}${s.project_limit ? ` · ${projectSpendLine(s.project_limit).replace(/^This project: /, "")}` : ""}`}>
          <p>Approval <span className="mono">{approved && s.approval_hash ? shortHash(s.approval_hash) : "none"}</span>. <button className="link" onClick={onPolicy}>Compare with your policy defaults</button></p>
          <GuardStrip ctx={ctx} />
          <Hud s={s} tokens={tokens} onOpen={() => setTab("usage")} />
        </Section>
      </div>

      <div className="project-tabs" ref={tabsBar}>
        <Tabs label="Project sections" panelId="project-section" value={tab} onChange={setTab} items={TABS.map((t) => ({
          ...t,
          badge: t.id === "work" ? (open ? String(open) : undefined) : t.id === "material" ? (s.sources.length ? String(s.sources.length) : undefined) : undefined,
        }))} />
      </div>

      <div className="tab-body" ref={tabBody} id="project-section" role="tabpanel" aria-labelledby={`project-section-${tab}`} tabIndex={0}>
        {tab === "work" && <WorkTab ctx={ctx} />}
        {tab === "material" && <MaterialTab ctx={ctx} />}
        {tab === "team" && <TeamTab ctx={ctx} draft={draft} setDraft={setDraft} />}
        {tab === "share" && <ShareTab ctx={ctx} />}
        {tab === "usage" && <CostTab ctx={ctx} />}
      </div>
    </div>
  );
}

type Reliability = { outcome: string; writer: string; checker: string; projects: number; finished: number; stopped: number; findings: number; accepted: number; stand_ins: number };

/** How each crew has done on this Mac, from the projects' own records. Shown once there is a project with a plan. */
function CrewRecord() {
  const [rows, setRows] = useState<Reliability[] | null>(null);
  useEffect(() => { api.call<Reliability[]>("reliability").then(setRows).catch(() => setRows(null)); }, []);
  if (!rows || rows.length === 0) return null;
  const short = (m: string) => m.split("/").pop() ?? m;
  return (
    <details className="sheet crew-record">
      <summary><strong>How your crews have done</strong> <span className="muted">from {count(rows.reduce((n, r) => n + r.projects, 0), "project")} on this Mac</span></summary>
      <p className="muted">Finished means every step has a result and none failed; a project still in progress counts as not finished. Safety stops are pauses such as a charge that could not be confirmed. Nothing here leaves your Mac.</p>
      <table className="table">
        <thead><tr><th>Outcome</th><th>Writer · Second check</th><th className="num">Projects</th><th className="num">Finished</th><th className="num">Safety stops</th><th className="num">Findings accepted</th></tr></thead>
        <tbody>{rows.map((r) => (
          <tr key={`${r.outcome}-${r.writer}-${r.checker}`}>
            <td>{r.outcome}</td><td>{short(r.writer)} · {short(r.checker)}</td>
            <td className="num">{r.projects}</td><td className="num">{r.finished} of {r.projects}</td><td className="num">{r.stopped}</td>
            <td className="num">{r.accepted} of {r.findings}</td>
          </tr>
        ))}</tbody>
      </table>
    </details>
  );
}

/** What every project has used this month and in all, in one line. A project that could not be read is named. */
function AllProjectsCost() {
  const [u, setU] = useState<{ projects: { project: string; rows: UsageRow[] }[]; unreadable: string[] } | null>(null);
  useEffect(() => { api.call<typeof u>("usage_everywhere").then(setU).catch(() => setU(null)); }, []);
  if (!u) return null;
  const all = u.projects.flatMap((p) => p.rows);
  if (all.length === 0) return null;
  const now = new Date();
  const month = all.filter((r) => { const d = new Date(r.at * 1000); return d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth(); });
  const t = total(all);
  const m = total(month);
  return (
    <p className="muted all-projects-cost">
      Across all projects: {dollarsLine(m.dollars, m.unknownDollars, m.requests)} this month, {dollarsLine(t.dollars, t.unknownDollars, t.requests)} in all, from {count(t.requests, "request")}.
      {u.unreadable.length > 0 && ` Not counted, because they could not be read: ${u.unreadable.join(", ")}.`}
    </p>
  );
}

function Home({ rows, archived, err, note, info, running, onNew, onOpen, onArchive, onRestore }: { rows: Overview[] | null; archived: Overview[]; err: string; note: string; info: Info | null; running: boolean; onNew: () => void; onOpen: (n: string) => void; onArchive: (n: string) => void; onRestore: (n: string) => void }) {
  const [query, setQuery] = useState("");
  const shown = filterProjects(rows ?? [], query);
  return (
    <div className="content-inner home">
      <Toolbar title="Projects" />
      {api.mocked && <p className="notice preview-note">Design preview · Sample projects and results. No real AI calls.</p>}
      {info && info.swept_orphans > 0 && <p className="muted">Ended {count(info.swept_orphans, "leftover process", "leftover processes")} from a previous session.</p>}
      {err && <p className="notice error" role="alert">{err}</p>}
      {note && <p className="notice" role="status">{note}</p>}
      {rows === null && !err && <Skeleton label="Loading projects" lines={3} />}
      {rows && rows.length === 0 && (
        <section className="hero">
          <BrandMark className="welcome-mark" />
          <h2 className="hero-title">{headline}</h2>
          <p className="lede">{audience}</p>
          <ul className="plain hero-points">
            <li>A second AI checks the work before you see it.</li>
            <li>Nothing is posted, deleted or installed without asking you first.</li>
            <li>You see the plan, the models and the cost limit before anything runs.</li>
          </ul>
          <button className="primary" onClick={onNew}>Start a project <kbd>⌘N</kbd></button>
        </section>
      )}
      {rows && rows.length > 0 && (
        <>
        <AllProjectsCost />
        <div className="project-list-heading"><h2 className="vh">Your projects</h2><label className="project-search">Find a project<input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search names or briefs" /></label></div>
        {(() => {
          // Need you: waiting for an approval, an answer, a review, or failed (DESIGN_SPEC.md §10, Projects).
          const need = shown.filter((r) => { const st = projectState(r, false, false); return st.state === "approval" || st.state === "failed" || st.word === "Not reviewed"; });
          const rest = shown.filter((r) => !need.includes(r));
          const row = (r: Overview) => {
            const i = rows.indexOf(r);
            const st = projectState(r, false, false);
            return (
              <Section key={r.name} title={<><span className="nav-glyph" aria-hidden="true">{GLYPH[st.state] ?? "○"}</span> {r.name}</>} value={`${st.word}${r.steps_total ? `, ${r.steps_done} of ${r.steps_total} ${r.steps_total === 1 ? "step" : "steps"}` : ""}`}>
                <p className="muted">{[r.outcome ?? (r.steps_total ? "Your own plan" : "No plan yet"), r.approved ? "approved" : r.steps_total ? "not approved" : "", lastActivity(r.last_at) ? `last activity ${lastActivity(r.last_at)}` : ""].filter(Boolean).join(" · ")}</p>
                {r.brief && <p className="prose">{r.brief}</p>}
                <div className="actions tight">
                  <button onClick={() => onOpen(r.name)} title={i < 9 ? `Open ${r.name} (⌘${i + 1})` : undefined}>Open</button>
                  <button className="quiet" disabled={running} title={running ? "Finish or stop the current work first" : undefined} onClick={() => onArchive(r.name)}>Archive</button>
                </div>
              </Section>
            );
          };
          return (
            <div className="page-sections"><div>
              {need.length > 0 && <Section title="Need you" value={`${need.length} ${need.length === 1 ? "project" : "projects"}`} flag={String(need.length)} open>{need.map(row)}</Section>}
              {rest.length > 0 && <Section title="Going or done" value={need.length ? `${rest.length} ${rest.length === 1 ? "project" : "projects"}, nothing needed` : `${rest.length} ${rest.length === 1 ? "project" : "projects"}`} open={need.length === 0}>{rest.map(row)}</Section>}
            </div></div>
          );
        })()}
        {shown.length === 0 && <div className="empty-state"><h3>No projects match “{query}”</h3><button className="link" onClick={() => setQuery("")}>Clear search</button></div>}
        </>
      )}
      <CrewRecord />
        {archived.length > 0 && (
        <details className="archived-projects">
          <summary>Archived <span className="meta">{archived.length}</span></summary>
          <ul className="project-list" aria-label="Archived projects">
            {archived.map((r) => (
              <li key={r.name} className="project-row archived-row">
                <span className="run-text"><strong>{r.name}</strong><span className="meta">{r.brief || "No brief"}</span></span>
                <button className="ghost" aria-label={`Restore ${r.name}`} onClick={() => onRestore(r.name)}>Restore</button>
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

/** The public beta runs only on connected API providers. */
const engine: EngineMode = "live";

type View = { k: "board" } | { k: "policy" } | { k: "usage" } | { k: "memory"; back: View } | { k: "home" } | { k: "new"; next?: NextDraft; nextName?: string } | { k: "restart"; seed: RestartDraft } | { k: "settings"; back: View } | { k: "project"; name: string };

const WIDTH_KEY = "bff-sidebar-w";
const HIDDEN_KEY = "bff-sidebar-hidden";
const savedHidden = () => { try { return localStorage.getItem(HIDDEN_KEY) === "1"; } catch { return false; } };
/** The width the person dragged to, or null: then the token's default holds, 248px, or 200px at 1100px and under. */
const savedWidth = (): number | null => { try { const v = Number(localStorage.getItem(WIDTH_KEY)); return v >= 200 && v <= 360 ? v : null; } catch { return null; } };

export default function App() {
  // The stress view is reached by ?stress=1 in a browser, or by Cmd+Shift+S inside the app.
  const [stress, setStress] = useState(new URLSearchParams(window.location.search).has("stress"));
  useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.metaKey && e.shiftKey && e.key.toLowerCase() === "s") setStress(true); };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);
  // A run that was going when this window loaded cannot still be driven from it, so its hold on the projects is let go.
  useEffect(() => { void api.call("end_run").catch(() => undefined); }, []);
  useEffect(() => {
    api.call<Info>("app_info").then((i) => { if (i.stress_mode) setStress(true); setInfo(i); }).catch(() => undefined);
  }, []);
  const [splash, setSplash] = useState(!stress);
  const [view, setView] = useState<View>({ k: "board" });
  const [attention, setAttention] = useState<AttentionStatus | null>(null);
  const [attentionError, setAttentionError] = useState("");
  useEffect(() => {
    let disposed = false, checking = false;
    const check = async () => {
      if (checking) return;
      checking = true;
      try {
        const next = await api.call<AttentionStatus>("attention_status", { takeOpen: true });
        if (!disposed) {
          setAttention(next); setAttentionError("");
          if (next.open_inbox) setView({ k: "board" });
        }
      } catch { if (!disposed) setAttentionError("Decision alerts could not be checked. Open the inbox to review pending work."); }
      finally { checking = false; }
    };
    void check(); const timer = window.setInterval(() => void check(), 3000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, []);
  const content = useRef<HTMLElement>(null);
  useLayoutEffect(() => { content.current?.scrollTo(0, 0); }, [view]);
  const [presenting, setPresenting] = useState(false);
  const [allRows, setRows] = useState<Overview[] | null>(null);
  const rows = useMemo(() => allRows?.filter((r) => !r.archived) ?? null, [allRows]);
  const archived = useMemo(() => allRows?.filter((r) => r.archived) ?? [], [allRows]);
  const [err, setErr] = useState("");
  const [note, setNote] = useState("");
  const [info, setInfo] = useState<Info | null>(null);
  const [running, setRunning] = useState(false);
  const [palette, setPalette] = useState(false);
  const [width, setWidthState] = useState<number | null>(savedWidth());
  const setWidth = useCallback((w: number) => { setWidthState(w); try { localStorage.setItem(WIDTH_KEY, String(w)); } catch { /* not kept */ } }, []);
  const [hidden, setHiddenState] = useState(savedHidden());
  const setHidden = useCallback((h: boolean) => { setHiddenState(h); try { localStorage.setItem(HIDDEN_KEY, h ? "1" : "0"); } catch { /* not kept */ } }, []);
  useEffect(() => {
    const toggle = () => setHidden(!savedHidden());
    const home = () => setView({ k: "home" });
    const key = (e: KeyboardEvent) => { if (e.metaKey && e.key === "\\") { e.preventDefault(); toggle(); } };
    window.addEventListener("bff:sidebar", toggle);
    window.addEventListener("bff:home", home);
    window.addEventListener("keydown", key);
    return () => { window.removeEventListener("bff:sidebar", toggle); window.removeEventListener("bff:home", home); window.removeEventListener("keydown", key); };
  }, [setHidden]);
  const done = useCallback(() => setSplash(false), []);

  // The sidebar's Policy count and today's cost at its foot, read with the project list.
  const [looser, setLooser] = useState(0);
  const [today, setToday] = useState("");
  const reload = useCallback(() => {
    api.call<Overview[]>("projects_overview").then((r) => { setRows(r); setErr(""); }).catch((e) => setErr(text(e)));
    api.call<PolicyOverview>("policy_overview").then((v) => setLooser(v.projects.filter((x) => x.plan && groupOf(x) === "looser").length)).catch(() => setLooser(0));
    api.call<Parameters<typeof todayLine>[0]>("usage_everywhere").then((u) => setToday(todayLine(u))).catch(() => setToday(""));
  }, []);
  useEffect(() => { setNote(""); reload(); }, [view.k, reload]);
  const onChanged = useCallback((busy: boolean) => { setRunning(busy); if (!busy) reload(); }, [reload]);
  // Which project the run belongs to, so the page bar can name it from any screen. Only one runs at a time.
  const [runName, setRunName] = useState("");
  // A run started from the board has no project page to report to, so the shell follows the runner itself.
  const [runnerOn, setRunnerOn] = useState(runner.project);
  useEffect(() => runner.subscribe({ change: (p) => { setRunnerOn(p); if (p) setRunName(p); else reload(); }, wave: () => reload() }), [reload]);
  const viewNow = useRef(view);
  viewNow.current = view;
  useEffect(() => { if (running && view.k === "project") setRunName(view.name); }, [running, view]);
  // Away from its project, the run cannot report back, so the app asks whether it is still going.
  useEffect(() => {
    if (!running || view.k === "project") return;
    const t = setInterval(() => {
      api.call<State>("get_state", { engine }).then((st) => { if (!st.run_active) { setRunning(false); reload(); } else reload(); }).catch(() => undefined);
    }, 2000);
    return () => clearInterval(t);
  }, [running, view.k, reload]);
  // Stop from the page bar of another screen reaches the run the same way its own page does when it was left.
  useEffect(() => {
    const stop = () => { if (viewNow.current.k === "project") return; stopFlag.current = true; void api.call("stop_run", { steps: true }).catch(() => undefined); };
    const key = (e: KeyboardEvent) => { if (e.metaKey && e.key === "." && viewNow.current.k !== "project") { e.preventDefault(); stop(); } };
    window.addEventListener("bff:stop", stop);
    window.addEventListener("keydown", key);
    return () => { window.removeEventListener("bff:stop", stop); window.removeEventListener("keydown", key); };
  }, []);

  // Why another project could not be opened. It shows where the person is, so nothing about the open project changes.
  const [refusal, setRefusal] = useState("");
  const openProject = useCallback(async (n: string): Promise<boolean> => {
    try { await api.call("open_project", { name: n }); }
    catch (e) {
      // Another project's step is still finishing. Opening this one now would put that step's result in it, so the
      // person stays on the project they are in and is told why.
      setRefusal(text(e));
      return false;
    }
    setRefusal("");
    setView({ k: "project", name: n });
    return true;
  }, []);
  const current = view.k === "project" ? view.name : null;
  const setArchived = useCallback(async (command: "archive_project" | "restore_project", name: string, done: string) => {
    try { await api.call(command, { name }); setNote(done); reload(); }
    catch (e) { setNote(""); setErr(text(e)); }
  }, [reload]);

  // G then B, P, O or U goes to Board, Projects, Policy or Usage (DESIGN_SPEC.md §11). Not while typing.
  useEffect(() => {
    let g = 0;
    const k = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || document.querySelector("dialog[open]")) return;
      const t = e.target as HTMLElement;
      if (/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || t.isContentEditable) return;
      const key = e.key.toLowerCase();
      if (Date.now() - g < 900) {
        g = 0;
        const to = ({ b: { k: "board" }, p: { k: "home" }, o: { k: "policy" }, u: { k: "usage" } } as Record<string, View>)[key];
        if (to) { e.preventDefault(); e.stopImmediatePropagation(); setView(to); }
        return;
      }
      if (key === "g") g = Date.now();
    };
    window.addEventListener("keydown", k, { capture: true });
    return () => window.removeEventListener("keydown", k, { capture: true });
  }, []);
  // Shortcuts (§6): ⌘K the palette, ⌘N a new project, ⌘1 to ⌘9 the projects in the sidebar.
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (!e.metaKey || e.shiftKey || e.altKey) return;
      if (e.key.toLowerCase() === "k") { e.preventDefault(); setPalette((p) => !p); }
      else if (e.key.toLowerCase() === "n") { e.preventDefault(); setView({ k: "new" }); }
      else if (/^[1-9]$/.test(e.key) && rows && rows[Number(e.key) - 1]) { e.preventDefault(); void openProject(rows[Number(e.key) - 1].name); }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [rows, openProject]);

  const items = useMemo<PaletteItem[]>(() => {
    const list: PaletteItem[] = [
      { id: "board", group: "Actions", label: "Board", run: () => setView({ k: "board" }) },
      { id: "projects", group: "Actions", label: "Projects", run: () => setView({ k: "home" }) },
      { id: "policy", group: "Actions", label: "Policy", run: () => setView({ k: "policy" }) },
      { id: "usage", group: "Actions", label: "Usage", run: () => setView({ k: "usage" }) },
      { id: "new", group: "Actions", label: "New project", hint: "⌘N", run: () => setView({ k: "new" }) },
      { id: "settings", group: "Actions", label: "Open settings", run: () => setView((v) => ({ k: "settings", back: v })) },
      ...(rows ?? []).map((r, i) => ({ id: `p-${r.name}`, group: "Projects" as const, label: r.name, hint: i < 9 ? `⌘${i + 1}` : undefined, run: () => void openProject(r.name) })),
    ];
    if (current) for (const t of TABS) list.push({ id: `t-${t.id}`, group: "Sections", label: `${t.label} of ${current}`, run: () => window.dispatchEvent(new CustomEvent("bff:tab", { detail: t.id })) });
    return list;
  }, [rows, current, openProject]);

  if (stress) return <div className="content"><main className="content-inner"><Stress /></main></div>;
  if (splash) return <Splash onDone={done} />;
  const quiet = async (f: () => Promise<unknown>) => { try { await f(); } catch { /* the panel shows its own error */ } };
  return (
    <div className={`shell${hidden ? " no-sidebar" : ""}`} style={width === null ? undefined : { "--sidebar-w": `${width}px` } as React.CSSProperties}>
      <WindowChrome />
      {!hidden && <Sidebar rows={rows} view={view.k} decisions={attention?.count ?? null} looser={looser} today={today} onPolicy={() => setView({ k: "policy" })} onUsage={() => setView({ k: "usage" })} current={current ?? (runName || null)} busy={running} onBoard={() => setView({ k: "board" })} onHome={() => setView({ k: "home" })} onOpen={(n) => void openProject(n)} onNew={() => setView({ k: "new" })} onMemory={() => setView({ k: "memory", back: view.k === "memory" ? { k: "home" } : view })} onSettings={() => setView({ k: "settings", back: view.k === "settings" ? { k: "home" } : view })} onHide={() => setHidden(true)} width={width ?? (window.innerWidth <= 1100 ? 200 : 248)} setWidth={setWidth} />}
      <RunContext.Provider value={(running || !!runnerOn) && runName && !(view.k === "project" && view.name === runName) ? { name: runName, done: rows?.find((r) => r.name === runName)?.steps_done ?? 0, total: rows?.find((r) => r.name === runName)?.steps_total ?? 0, open: () => void openProject(runName), stop: () => window.dispatchEvent(new Event("bff:stop")) } : null}>
      <main className={`content${view.k === "board" ? " content-board" : ""}`} id="main-content" tabIndex={-1} ref={content}>
        {refusal && <div className="attention-strip" role="alert"><span>{refusal}</span><button className="ghost" onClick={() => setRefusal("")}>Dismiss</button></div>}
        {view.k !== "board" && (attentionError || attention?.last_error || attention?.errors || (hidden && !!attention?.count)) ? <div className="attention-strip" role="status"><span>{attentionError || attention?.last_error || (attention?.errors ? "Some project decisions could not be read." : `${needInput(attention?.count ?? 0)}.`)}</span><button className="ghost" onClick={() => setView({ k: "board" })}>Open the board</button></div> : null}
        {view.k === "board" && <div className="board-page"><Toolbar title="Board" /><Board empty={rows !== null && rows.length === 0} onNew={() => setView({ k: "new" })} onProjects={() => setView({ k: "home" })} onOpen={(item: DecisionItem, tab) => { try { sessionStorage.setItem("sproutcrew:decision", JSON.stringify(item)); if (tab) sessionStorage.setItem("sproutcrew:tab", tab); } catch { /* no storage */ } void openProject(item.project).then((opened) => { if (!opened) try { sessionStorage.removeItem("sproutcrew:decision"); } catch { /* no storage */ } }); }} /></div>}
        {view.k === "policy" && <div className="content-inner"><Toolbar title="Policy" /><PolicyPage running={runnerOn} onOpen={(n) => void openProject(n)} onSaved={reload} /></div>}
        {view.k === "usage" && <div className="content-inner"><Toolbar title="Usage" /><UsagePage onOpen={(n) => void openProject(n)} /></div>}
        {view.k === "home" && <Home rows={rows} archived={archived} err={err} note={note} info={info} running={running} onNew={() => setView({ k: "new" })} onOpen={openProject} onArchive={(n) => void setArchived("archive_project", n, `${n} is archived. Its work is kept, and you can restore it below.`)} onRestore={(n) => void setArchived("restore_project", n, `${n} is back in your projects.`)} />}
        {view.k === "new" && <div className="content-inner"><Toolbar title="New project" /><NewProject key={view.next?.from ?? "new"} next={view.next} nextName={view.nextName} taken={(allRows ?? []).map((r) => r.name)} onOpen={openProject} onBack={() => { if (view.next) void openProject(view.next.from); else setView({ k: "home" }); }} /></div>}
        {view.k === "restart" && <div className="content-inner"><Toolbar title="Start over" /><NewProject key={view.seed.from} restart={view.seed} taken={(allRows ?? []).map((r) => r.name)} onOpen={openProject} onBack={() => void openProject(view.seed.from)} /></div>}
        {view.k === "memory" && <div className="content-inner"><Toolbar title="Memory" /><Memory project={view.back.k === "project" ? view.back.name : null} onProject={() => { if (view.back.k === "project") { try { sessionStorage.setItem("sproutcrew:tab", "material"); } catch { /* no storage */ } setView(view.back); } }} /></div>}
        {view.k === "settings" && <div className="content-inner"><Toolbar title="Settings" /><Settings busy="" guard={quiet} presenting={presenting} setPresenting={setPresenting} onBack={() => setView(view.back)} /></div>}
        {view.k === "project" && <Project key={view.name} onBoard={() => setView({ k: "board" })} onPolicy={() => setView({ k: "policy" })} onChanged={onChanged} openProject={(n) => void openProject(n)} onContinue={(next) => setView({ k: "new", next, nextName: restartName(next.from, (rows ?? []).map((r) => r.name), next.kind === "app_development" ? "app" : "next") })} onRestart={(s) => { setView({ k: "restart", seed: { from: s.name, name: restartName(s.name, (allRows ?? []).map((row) => row.name)), brief: s.brief, sources: s.sources } }); }} />}
      </main>
      </RunContext.Provider>
      {palette && <CommandPalette items={items} onClose={() => setPalette(false)} />}
    </div>
  );
}
