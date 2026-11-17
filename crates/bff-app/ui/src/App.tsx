import Memory from "./Memory";
import { total, dollarsLine, UsageRow } from "./usage";
import DecisionInbox from "./DecisionInbox";
import { headline, audience } from "./positioning";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
import { Ctx, PillState, setRoleLabels, StatusPill, Tabs, text, WindowChrome } from "./ui";
import { LinkedLine } from "./Continue";
import type { NextDraft } from "./handoff";
import { filterProjects, lastActivity, restartName } from "./projects";
import WorkTab from "./Work";

type TabId = "work" | "material" | "team" | "share" | "usage";
const TABS: { id: TabId; label: string }[] = [
  { id: "work", label: "Plan & work" }, { id: "material", label: "Sources" }, { id: "team", label: "Your crew" }, { id: "share", label: "Results & sharing" }, { id: "usage", label: "Usage & limits" },
];

type Overview = { name: string; brief: string; steps_done: number; steps_total: number; steps_failed?: number; approved: boolean; archived: boolean; outcome?: string | null; last_at?: number | null };

/** The sidebar shows one project's state as the same pill words a step uses. */
function projectState(o: Overview, open: boolean, busy: boolean): { state: PillState; word: string } {
  if (open && busy) return { state: "running", word: "Running" };
  if (o.steps_total === 0) return { state: "queued", word: "No plan yet" };
  if (o.steps_failed) return { state: "failed", word: `${o.steps_failed} step${o.steps_failed === 1 ? "" : "s"} failed` };
  if (o.steps_done === o.steps_total) return { state: "done", word: "Finished" };
  if (!o.approved) return { state: "approval", word: "Needs approval" };
  return { state: "queued", word: `${o.steps_done} of ${o.steps_total} steps done` };
}

function Toolbar({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <header className="toolbar" data-tauri-drag-region>
      <button className="quiet show-sidebar" onClick={() => window.dispatchEvent(new Event("bff:sidebar"))} title="Show the sidebar (⌘\)" aria-label="Show the sidebar">Show sidebar</button>
      <button className="quiet mobile-home" onClick={() => window.dispatchEvent(new Event("bff:home"))}>Projects</button>
      <h1 data-tauri-drag-region>{title}</h1>
      {children && <div className="toolbar-actions">{children}</div>}
    </header>
  );
}

/** The sidebar plane (§2): the open project, and the ways out of it. It does not list every project: "All projects"
 *  does that. It can be hidden with the button in the toolbar or ⌘\, and the choice is remembered. Its width is too. */
function Sidebar({ current, busy, decisions, onHome, onDecisions, onNew, onMemory, onSettings, onHide, width, setWidth }: {
  current: Overview | null; busy: boolean; decisions: number | null; onHome: () => void; onDecisions: () => void; onNew: () => void; onMemory: () => void; onSettings: () => void; onHide: () => void; width: number; setWidth: (w: number) => void;
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
  const st = current ? projectState(current, true, busy) : null;
  return (
    <>
      <nav className="sidebar" aria-label="Project">
        <div className="sidebar-top" data-tauri-drag-region>
          <span className="sidebar-title brand-lockup" data-tauri-drag-region><BrandMark />SproutCrew</span>
          <button className="quiet" onClick={onHide} title="Hide the sidebar (⌘\)" aria-label="Hide the sidebar">Hide</button>
        </div>
        <div className="sidebar-list">
          {current && st && (
            <>
              <div className="eyebrow sidebar-head">Open project</div>
              <div className="run-row sel" role="group" aria-label={`Open project ${current.name}`}>
                <span className={`mark ${st.state === "done" ? "queued" : st.state}`} aria-hidden="true" style={st.state === "done" ? { background: "var(--live)", border: 0 } : undefined} />
                <span className="run-text">
                  <strong>{current.name}</strong>
                  <span className="meta">{st.word}</span>
                </span>
                <span className="run-count">{current.steps_total ? `${current.steps_done}/${current.steps_total}` : ""}</span>
              </div>
            </>
          )}
        </div>
        <div className="sidebar-bottom">
          <button className="quiet start" onClick={onHome}>All projects</button>
          <button className="quiet start" onClick={onDecisions}>Decision inbox{decisions !== null && <span className="tab-badge" aria-live="polite" aria-label={`${decisions} items need input`}>{decisions}</span>}</button>
          <button className="quiet start" onClick={onNew}>New project <kbd>⌘N</kbd></button>
          <button className="quiet start" onClick={onMemory}>Memory</button>
          <button className="quiet start" onClick={onSettings}>Settings</button>
        </div>
      </nav>
      <div className={`sash${dragging ? " drag" : ""}`} role="separator" aria-orientation="vertical" aria-label="Resize the sidebar" aria-valuenow={width} tabIndex={0}
        onPointerDown={(e) => { e.preventDefault(); drag.current = true; setDragging(true); }}
        onKeyDown={(e) => { if (e.key === "ArrowLeft") setWidth(Math.max(200, width - 16)); if (e.key === "ArrowRight") setWidth(Math.min(360, width + 16)); }} />
    </>
  );
}

function Project({ onChanged, onRestart, onContinue, openProject }: { onChanged: (busy: boolean) => void; onRestart: (state: State) => void; onContinue: (draft: NextDraft) => void; openProject: (name: string) => void }) {
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
  const [msg, setMsg] = useState("");
  const [confirmRestart, setConfirmRestart] = useState(false);
  const [autoRunning, setAutoRunning] = useState(false);
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
  useEffect(() => { onChanged(!!busy || autoRunning); }, [busy, autoRunning]);
  // The command palette goes to a section of this project.
  useEffect(() => {
    const go = (e: Event) => { const t = (e as CustomEvent<TabId>).detail; if (TABS.some((x) => x.id === t)) setTab(t); };
    const key = (e: KeyboardEvent) => { if (e.metaKey && e.key === ".") { e.preventDefault(); window.dispatchEvent(new Event("bff:stop")); } };
    const stop = () => void api.call("stop_run").catch(() => undefined);
    window.addEventListener("bff:tab", go);
    window.addEventListener("bff:stop", stop);
    window.addEventListener("keydown", key);
    return () => { window.removeEventListener("bff:tab", go); window.removeEventListener("bff:stop", stop); window.removeEventListener("keydown", key); };
  }, []);

  const track = (e: RunEvent) => {
    setBusy(e.event === "started" ? `${e.what} on ${e.engine}` : e.event === "progress" ? e.line : "");
    // A rate-limit wait while SproutCrew is in the background gets one notification; the app limits how often.
    if (e.event === "progress" && e.line.includes("Retrying in")) void api.call("notify_run", { outcome: "waiting" }).catch(() => undefined);
  };
  const guard = async (f: () => Promise<unknown>) => {
    setMsg("");
    try { await f(); } catch (e) { setMsg(text(e)); } finally { setBusy(""); await refresh(); }
  };

  if (!s) return <div className="content-inner"><Toolbar title="Opening project" /><Skeleton label="Opening the project" lines={4} /></div>;
  const dirty = !!draft && !!s.plan && JSON.stringify(draft) !== JSON.stringify(s.plan);
  const ctx: Ctx = { s, engine, busy, guard, track, refresh, dirty, goTo: setTab, onContinue, openProject };
  const open = s.review ? Object.values(s.review).reduce((k, r) => k + r.findings.filter((f) => !f.decision).length, 0) : 0;

  return (
    <div className="content-inner project" ref={root}>
      <Toolbar title={s.name}>
        <button className="quiet" disabled={!!busy || autoRunning} title={busy || autoRunning ? "Finish or stop the current work before starting over" : "Return to setup with your brief and sources"} onClick={() => setConfirmRestart(true)}>Start over</button>
        {busy && <button className="ghost" onClick={() => window.dispatchEvent(new Event("bff:stop"))} title="Stop the run (⌘.)">Stop <kbd>⌘.</kbd></button>}
      </Toolbar>

      {confirmRestart && <Gate title="Start over with this brief?" approveLabel="Open fresh setup" rejectLabel="Keep working" busy={!!busy || autoRunning}
        what={<><p>Go back to the first setup screen with your brief and {s.sources.length} {s.sources.length === 1 ? "source" : "sources"}. You can change the outcome, edit the brief, and leave out sources.</p><p>Your original project and all its results stay available. The fresh project has no previous results or approvals. Shared API usage and limits still apply; AI-assisted planning also uses provider allowance.</p><p className="muted">Nothing is created until you finish setup. Cancelling leaves the original project unchanged.</p></>}
        onReject={() => setConfirmRestart(false)} onApprove={() => { setConfirmRestart(false); onRestart(s); }} />}

      {busy && (
        <div className="busy-block" role="status" aria-live="polite">
          <p className="busy"><span className="spinner" aria-hidden="true" />Working: {busy}</p>
          <div className="stream-strip" aria-hidden="true"><span /></div>
        </div>
      )}
      {msg && (
        <div className="notice error" role="alert">
          <p>{msg}</p>
          <button className="ghost dismiss" onClick={() => setMsg("")}>Dismiss</button>
        </div>
      )}

      <LinkedLine ctx={ctx} />

      <GuardStrip ctx={ctx} />
      <Hud s={s} tokens={tokens} onOpen={() => setTab("usage")} />

      <div className="project-tabs" ref={tabsBar}>
        <Tabs label="Project sections" panelId="project-section" value={tab} onChange={setTab} items={TABS.map((t) => ({
          ...t,
          badge: t.id === "work" ? (open ? String(open) : undefined) : t.id === "material" ? (s.sources.length ? String(s.sources.length) : undefined) : undefined,
        }))} />
      </div>

      <div className="tab-body" ref={tabBody} id="project-section" role="tabpanel" aria-labelledby={`project-section-${tab}`} tabIndex={0}>
        {tab === "work" && <WorkTab ctx={ctx} onAutoChange={setAutoRunning} />}
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
      <summary><strong>How your crews have done</strong> <span className="muted">from {rows.reduce((n, r) => n + r.projects, 0)} projects on this Mac</span></summary>
      <p className="muted">Finished means every step has a result and none failed; a project still in progress counts as not finished. Safety stops are pauses such as a charge that could not be confirmed. Nothing here leaves your Mac.</p>
      <table className="table">
        <thead><tr><th>Outcome</th><th>Writer · Checker</th><th className="num">Projects</th><th className="num">Finished</th><th className="num">Safety stops</th><th className="num">Findings accepted</th></tr></thead>
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
      Across all projects: {dollarsLine(m.dollars, m.unknownDollars, m.requests)} this month, {dollarsLine(t.dollars, t.unknownDollars, t.requests)} in all, from {t.requests} request{t.requests === 1 ? "" : "s"}.
      {u.unreadable.length > 0 && ` Not counted, because they could not be read: ${u.unreadable.join(", ")}.`}
    </p>
  );
}

function Home({ rows, archived, err, note, info, running, onNew, onOpen, onArchive, onRestore }: { rows: Overview[] | null; archived: Overview[]; err: string; note: string; info: Info | null; running: boolean; onNew: () => void; onOpen: (n: string) => void; onArchive: (n: string) => void; onRestore: (n: string) => void }) {
  const [query, setQuery] = useState("");
  const shown = filterProjects(rows ?? [], query);
  return (
    <div className="content-inner home">
      <Toolbar title="All projects" />
      {api.mocked && <p className="notice preview-note">Design preview · Sample projects and results. No real AI calls.</p>}
      {info && info.swept_orphans > 0 && <p className="muted">Ended {info.swept_orphans} leftover process(es) from a previous session.</p>}
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
        <div className="project-list-heading"><h2>Your projects <span className="meta">{rows.length}</span></h2><label className="project-search">Find a project<input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search names or briefs" /></label></div>
        <ul className="project-list" aria-label="Projects">
          {shown.map((r) => {
            const i = rows.indexOf(r);
            const st = projectState(r, false, false);
            return (
              <li key={r.name} className="project-row">
                <button className="run-row wide" onClick={() => onOpen(r.name)} title={i < 9 ? `Open ${r.name} (⌘${i + 1})` : `Open ${r.name}`}>
                  <span className={`mark ${st.state === "done" ? "queued" : st.state}`} aria-hidden="true" style={st.state === "done" ? { background: "var(--live)", border: 0 } : undefined} />
                  <span className="run-text"><strong>{r.name}</strong><span className="meta">{[r.outcome ?? (r.steps_total ? "Your own plan" : ""), lastActivity(r.last_at)].filter(Boolean).join(" · ")}</span></span>
                  <span className="home-state"><StatusPill state={st.state} title={st.word} /><span className="run-count">{r.steps_total ? `${r.steps_done} of ${r.steps_total} steps` : "No plan yet"}</span></span>
                </button>
                <details className="row-menu">
                  <summary aria-label={`More for ${r.name}`} title="More">⋯</summary>
                  <div className="row-menu-items">
                    <button className="quiet" disabled={running} title={running ? "Finish or stop the current work first" : undefined} onClick={(e) => { (e.currentTarget.closest("details") as HTMLDetailsElement).open = false; onArchive(r.name); }}>Archive <span className="meta">Hidden here; work is kept</span></button>
                  </div>
                </details>
              </li>
            );
          })}
        </ul>
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

type View = { k: "decisions" } | { k: "memory"; back: View } | { k: "home" } | { k: "new"; next?: NextDraft; nextName?: string } | { k: "restart"; seed: RestartDraft } | { k: "settings"; back: View } | { k: "project"; name: string };

const WIDTH_KEY = "bff-sidebar-w";
const HIDDEN_KEY = "bff-sidebar-hidden";
const savedHidden = () => { try { return localStorage.getItem(HIDDEN_KEY) === "1"; } catch { return false; } };
const savedWidth = () => { try { const v = Number(localStorage.getItem(WIDTH_KEY)); return v >= 200 && v <= 360 ? v : 248; } catch { return 248; } };

export default function App() {
  // The stress view is reached by ?stress=1 in a browser, or by Cmd+Shift+S inside the app.
  const [stress, setStress] = useState(new URLSearchParams(window.location.search).has("stress"));
  useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.metaKey && e.shiftKey && e.key.toLowerCase() === "s") setStress(true); };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);
  useEffect(() => {
    api.call<Info>("app_info").then((i) => { if (i.stress_mode) setStress(true); setInfo(i); }).catch(() => undefined);
  }, []);
  const [splash, setSplash] = useState(!stress);
  const [view, setView] = useState<View>({ k: "home" });
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
          if (next.open_inbox) setView({ k: "decisions" });
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
  const [width, setWidthState] = useState(savedWidth());
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

  const reload = useCallback(() => {
    api.call<Overview[]>("projects_overview").then((r) => { setRows(r); setErr(""); }).catch((e) => setErr(text(e)));
  }, []);
  useEffect(() => { setNote(""); reload(); }, [view.k, reload]);
  const onChanged = useCallback((busy: boolean) => { setRunning(busy); if (!busy) reload(); }, [reload]);

  const openProject = useCallback(async (n: string) => {
    await api.call("open_project", { name: n }).catch(() => undefined);
    setView({ k: "project", name: n });
  }, []);
  const current = view.k === "project" ? view.name : null;
  const setArchived = useCallback(async (command: "archive_project" | "restore_project", name: string, done: string) => {
    try { await api.call(command, { name }); setNote(done); reload(); }
    catch (e) { setNote(""); setErr(text(e)); }
  }, [reload]);

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
    <div className={`shell${hidden ? " no-sidebar" : ""}`} style={{ "--sidebar-w": `${width}px` } as React.CSSProperties}>
      <WindowChrome />
      {!hidden && <Sidebar decisions={attention?.count ?? null} current={rows?.find((r) => r.name === current) ?? null} busy={running} onHome={() => setView({ k: "home" })} onDecisions={() => setView({ k: "decisions" })} onNew={() => setView({ k: "new" })} onMemory={() => setView({ k: "memory", back: view.k === "memory" ? { k: "home" } : view })} onSettings={() => setView({ k: "settings", back: view.k === "settings" ? { k: "home" } : view })} onHide={() => setHidden(true)} width={width} setWidth={setWidth} />}
      <main className="content" id="main-content" tabIndex={-1} ref={content}>
        {view.k !== "decisions" && (attentionError || attention?.last_error || attention?.errors || (hidden && !!attention?.count)) ? <div className="attention-strip" role="status"><span>{attentionError || attention?.last_error || (attention?.errors ? "Some project decisions could not be read." : `${attention?.count} item(s) need your input.`)}</span><button className="ghost" onClick={() => setView({ k: "decisions" })}>Review decisions</button></div> : null}
        {view.k === "decisions" && <div className="content-inner"><Toolbar title="Decision inbox" /><DecisionInbox busy={running} onOpen={(item) => { sessionStorage.setItem("sproutcrew:decision", JSON.stringify(item)); void openProject(item.project); }} onProjects={() => setView({ k: "home" })} /></div>}
        {view.k === "home" && <Home rows={rows} archived={archived} err={err} note={note} info={info} running={running} onNew={() => setView({ k: "new" })} onOpen={openProject} onArchive={(n) => void setArchived("archive_project", n, `${n} is archived. Its work is kept, and you can restore it below.`)} onRestore={(n) => void setArchived("restore_project", n, `${n} is back in your projects.`)} />}
        {view.k === "new" && <div className="content-inner"><Toolbar title="New project" /><NewProject key={view.next?.from ?? "new"} next={view.next} nextName={view.nextName} onOpen={openProject} onBack={() => { if (view.next) void openProject(view.next.from); else setView({ k: "home" }); }} /></div>}
        {view.k === "restart" && <div className="content-inner"><Toolbar title="Start over" /><NewProject key={view.seed.from} restart={view.seed} onOpen={openProject} onBack={() => void openProject(view.seed.from)} /></div>}
        {view.k === "memory" && <div className="content-inner"><Toolbar title="Memory" /><Memory project={view.back.k === "project" ? view.back.name : null} onProject={() => { if (view.back.k === "project") { try { sessionStorage.setItem("sproutcrew:tab", "material"); } catch { /* no storage */ } setView(view.back); } }} /></div>}
        {view.k === "settings" && <div className="content-inner"><Toolbar title="Settings" /><Settings busy="" guard={quiet} presenting={presenting} setPresenting={setPresenting} onBack={() => setView(view.back)} /></div>}
        {view.k === "project" && <Project key={view.name} onChanged={onChanged} openProject={(n) => void openProject(n)} onContinue={(next) => setView({ k: "new", next, nextName: restartName(next.from, (rows ?? []).map((r) => r.name), next.kind === "app_development" ? "app" : "next") })} onRestart={(s) => { setView({ k: "restart", seed: { from: s.name, name: restartName(s.name, (allRows ?? []).map((row) => row.name)), brief: s.brief, sources: s.sources } }); }} />}
      </main>
      {palette && <CommandPalette items={items} onClose={() => setPalette(false)} />}
    </div>
  );
}
