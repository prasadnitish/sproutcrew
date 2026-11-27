import CodeReview from "./CodeReview";
import { capStop, heldStop, plainWhy } from "./failures";
import ActionPolicy from "./Policy";
import { useEffect, useState } from "react";
import { runner } from "./runner";
import { api, Artifact, Finding, ReviewSummary, Stage } from "./api";
import Coordinator from "./Coordinator";
import Diagram from "./WorkflowDiagram";
import { detailLine, runsOf } from "./diagram";
import { InjectionNotice, RequirementsNotice } from "./Files";
import Markdown from "./Document";
import { checksOf, modelOf, needsOf, readySteps, runningIds } from "./graph";
import StepTalk from "./Talk";
import FinalFiles, { SecondCheckResult } from "./FinalFiles";
import { Skeleton } from "./Gate";
import Shape from "./Shape";
import StepModels from "./StepModels";
import ModelPicker from "./ModelPicker";
import { useEstimate } from "./Preflight";
import ReviewBoard from "./ReviewBoard";
import Questions from "./Questions";
import { asking } from "./facts";
import { usePop } from "./spring";
import { secondCheckOf } from "./secondcheck";
import { headline, ticked, Check } from "./verdict";
import { stoppedLine } from "./status";
import { Chip, Ctx, effort, Panel, provider, roleName, secs, StatusPill, text } from "./ui";

function Findings({ ctx, name, r }: { ctx: Ctx; name: string; r: ReviewSummary }) {
  const [rejecting, setRejecting] = useState<string | null>(null);
  const [escalating, setEscalating] = useState<string | null>(null);
  const [escalationReason, setEscalationReason] = useState("");
  const [reason, setReason] = useState("");
  const states = Object.values(r.reviewers);
  // After a revision every review is out of date, and the findings below describe the old version.
  const stale = states.length > 0 && states.every((v) => v === "invalidated");
  const accepted = r.findings.filter((f) => f.decision === "accept").length;
  const undecided = r.findings.filter((f) => !f.decision).length;
  const decide = (f: Finding, decision: "accept" | "reject", why = "") =>
    ctx.guard(async () => {
      await api.call("decide", { project: ctx.s.name, artifact: name, finding: f.id, decision, reason: why });
      setRejecting(null);
      setReason("");
    });
  return (
    <div className="findings">
      {r.findings.map((f) => (
        <div className={`finding${stale ? " old" : ""}`} key={f.id} id={`finding-${name}-${f.id}`}>
          <p className="finding-claim">{f.claim}</p>
          <p className="muted">{f.evidence}</p>
          {f.escalated && !f.decision && <Chip tone="warn">Escalated for your judgment</Chip>}
          {!stale && !f.decision && !f.escalated && <button className="ghost" disabled={!!ctx.busy} onClick={() => setEscalating(f.id)}>Escalate this finding</button>}
          {escalating === f.id && <div className="reject"><label>What needs your judgment?<input value={escalationReason} onChange={e => setEscalationReason(e.target.value)} maxLength={2000} /></label><button disabled={!!ctx.busy || !escalationReason.trim()} onClick={() => ctx.guard(async () => { await api.call("escalate_finding", { project: ctx.s.name, artifact: name, finding: f.id, base: r.head, reason: escalationReason }); setEscalating(null); setEscalationReason(""); })}>Keep in decision inbox</button><button className="ghost" onClick={() => setEscalating(null)}>Cancel escalation</button></div>}
          {f.decision === "accept" && <Chip tone="ok">Accepted</Chip>}
          {f.decision === "reject" && <><Chip tone="quiet">Rejected</Chip> <span className="muted">Reason: {f.reason}</span></>}
          {!stale && !f.decision && rejecting !== f.id && (
            <div className="actions tight">
              <button onClick={() => decide(f, "accept")} disabled={!!ctx.busy}>Accept</button>
              <button className="ghost" onClick={() => setRejecting(f.id)}>Reject</button>
            </div>
          )}
          {!stale && !f.decision && rejecting === f.id && (
            <div className="reject">
              <label>Why are you rejecting it?
                <input value={reason} onChange={(e) => setReason(e.target.value)} placeholder="A reason is kept with the decision" autoFocus />
              </label>
              <div className="actions tight">
                <button disabled={!reason.trim()} onClick={() => decide(f, "reject", reason)}>Reject this point</button>
                <button className="ghost" onClick={() => setRejecting(null)}>Cancel</button>
              </div>
            </div>
          )}
        </div>
      ))}
      {stale && r.findings.length > 0 && <p className="muted">These points are about an earlier version. Version {r.version} has been revised, so ask for another check.</p>}
      {!stale && r.findings.length > 0 && undecided === 0 && accepted > 0 && (
        <div className="actions">
          <button className="primary" disabled={!!ctx.busy} onClick={() => ctx.guard(() => api.run("revise", { engine: ctx.engine, artifact: name, by: r.owner, base: r.head }, ctx.track))}>
            Have {roleName(r.owner)} address {accepted} accepted {accepted === 1 ? "finding" : "findings"}
          </button>
        </div>
      )}
      {!stale && undecided > 0 && <p className="muted">{undecided} to decide. Accepted points go back to {roleName(r.owner)}. A rejected one keeps its reason.</p>}
    </div>
  );
}

function ReviewBlock({ ctx, name, r }: { ctx: Ctx; name: string; r: ReviewSummary }) {
  const roles = (ctx.s.plan?.team ?? []).map((t) => t.role).filter((x) => x !== r.owner);
  const [who, setWho] = useState(roles[0] ?? "");
  const states = Object.entries(r.reviewers);
  return (
    <div className="review">
      <div className="review-head">
        <span className="muted">Written by {roleName(r.owner)}, version {r.version}</span>
        {states.map(([role, st]) => (
          <span key={role}>
            {st === "valid" && r.findings.length === 0 && <Chip tone="ok">{roleName(role)} found nothing to change</Chip>}
            {st === "valid" && r.findings.length > 0 && <Chip tone="warn">{roleName(role)} raised points</Chip>}
            {st === "invalidated" && <Chip tone="warn">{roleName(role)}: check is out of date</Chip>}
          </span>
        ))}
      </div>
      {roles.length > 0 ? (
        <div className="actions tight">
          <select aria-label="Reviewer" value={who} onChange={(e) => setWho(e.target.value)}>
            {roles.map((x) => <option key={x} value={x}>{roleName(x)}</option>)}
          </select>
          <button disabled={!!ctx.busy || !who} onClick={() => ctx.guard(() => api.run("review", { engine: ctx.engine, artifact: name, reviewer: who }, ctx.track))}>
            {states.length ? "Check again" : "Ask for a second opinion"}
          </button>
        </div>
      ) : (
        <p className="muted">Add a second role to the team to get a second opinion.</p>
      )}
      <Findings ctx={ctx} name={name} r={r} />
    </div>
  );
}

/** `attention` is a finished check whose verdict is not Ready, so it has no green tick. */
type Status = "done" | "attention" | "next" | "waiting" | "running" | "failed";

/** Which side a role is on: the ones that write, and the ones that check what was written. */
const sideOf = (ctx: Ctx, role: string): "writing" | "checking" =>
  ctx.s.role_info?.[role]?.side ?? (role === "reviewer" ? "checking" : "writing");

/** The status mark of a step row. A step that finishes while you watch gets the one playful moment of this view. */
function Mark({ status }: { status: Status }) {
  const k = usePop(status === "done");
  if (status === "done") {
    return (
      <svg className="mark done" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" style={{ transform: `scale(${k})` }}>
        <path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  }
  return <span className={`mark ${status === "running" ? "running" : status === "failed" ? "failed" : status === "attention" ? "attention" : "queued"}`} aria-hidden="true" />;
}

const STATUS_WORD: Record<Status, string> = { done: "Done", attention: "Needs a look", running: "Running", next: "Next", waiting: "Waiting", failed: "Failed" };

/** Which later steps read this result on a checking side, on what model, and whether they have run. */
function Checks({ ctx, st, onPick }: { ctx: Ctx; st: Stage; onPick: (id: string) => void }) {
  const plan = ctx.s.plan;
  if (!plan) return null;
  const checks = checksOf(plan.stages, st.id, (role) => sideOf(ctx, role) === "checking");
  if (checks.length === 0) return null;
  return (
    <p className="muted step-checks">
      Checked by {checks.map((c, i) => <span key={c.id}>{i > 0 ? (i === checks.length - 1 ? " and " : ", ") : ""}<button className="link" onClick={() => onPick(c.id)}>{c.title}</button>{ctx.s.artifacts.some((x) => x.name === c.id) ? " (done)" : ""}</span>)}.
    </p>
  );
}

/** Why a step stopped, in one or two sentences, with the moves that fit: another model for this step, credit for the
 *  account, or another try once a short wait is over. Choosing a model changes the plan, so it is approved again. */
function StepFailed({ ctx, st, why, kind, earlier, onTalk }: { ctx: Ctx; st: Stage; why: string; kind?: string; earlier: boolean; onTalk: () => void }) {
  const [picking, setPicking] = useState(false);
  const wait = kind === "per_minute" || kind === "upstream_busy" ? 60 : 0;
  const [left, setLeft] = useState(wait);
  useEffect(() => {
    setLeft(wait);
    if (!wait) return;
    const t = setInterval(() => setLeft((n) => Math.max(0, n - 1)), 1000);
    return () => clearInterval(t);
  }, [why, wait]);
  const plan = ctx.s.plan;
  const current = plan ? modelOf(plan.team, st) : undefined;
  const openrouter = (current?.route ?? "R12") === "R12";
  const limit = !!kind;
  const choose = (model: string) => {
    setPicking(false);
    if (!plan || !current) return;
    const stages = plan.stages.map((x) => (x.id === st.id ? { ...x, uses: { route: current.route, model, effort: current.effort } } : x));
    void ctx.guard(() => api.call("save_plan", { plan: { ...plan, stages } }));
  };
  const retry = () => ctx.guard(() => api.run("run_stage", { engine: ctx.engine, stageId: st.id }, ctx.track));
  if (heldStop(why) && capStop(why)) {
    // Approving a higher cap in Usage lifts this hold at once; no restart is needed (Q2-B10).
    return (
      <div className="notice error step-failed" role="alert">
        <p><strong>This step did not finish, and all AI work is on hold until a higher cap is approved.</strong> {plainWhy(why)}</p>
        <div className="actions tight">
          <button className="ghost" disabled={ctx.s.gate !== "allowed" || !!ctx.busy} onClick={retry}>Try again</button>
        </div>
        <p className="muted small">Approve a higher cap in Usage, then try this step again.</p>
      </div>
    );
  }
  if (heldStop(why)) {
    // Every step is held until the app is reopened, so another model or another try cannot help here.
    return (
      <div className="notice error step-failed" role="alert">
        <p><strong>This step did not finish, and all AI work is on hold.</strong> {plainWhy(why)}</p>
        <div className="actions tight">
          {openrouter && <button onClick={() => void api.call("open_link", { link: "R12-activity" }).catch(() => undefined)}>Check usage on OpenRouter ↗</button>}
        </div>
        <p className="muted small">After checking, quit SproutCrew and open it again, then run this step again.</p>
      </div>
    );
  }
  return (
    <div className="notice error step-failed" role="alert">
      <p>{limit ? why : `This step did not finish. ${plainWhy(why)}`}{earlier ? " The result below is from an earlier run." : ""}</p>
      <div className="actions tight">
        {openrouter && <button onClick={() => setPicking(true)}>Choose another model</button>}
        {openrouter && (kind === "out_of_credits" || kind === "daily_cap") && <button className="ghost" onClick={() => void api.call("open_link", { link: "R12-credits" }).catch(() => undefined)}>Add credit ↗</button>}
        <button className="ghost" disabled={left > 0 || ctx.s.gate !== "allowed" || !!ctx.busy} onClick={retry}>{left > 0 ? `Try again in ${left}s` : "Try again"}</button>
        {!limit && <button className="ghost" onClick={onTalk}>Ask for a change</button>}
      </div>
      {openrouter && <p className="muted small">A new model changes the plan, so you approve it again before the step runs.</p>}
      {picking && current && <ModelPicker side={(ctx.s.role_info?.[st.owner_role]?.side ?? "writing") === "checking" ? "checking" : "writing"} value={current.model} onClose={() => setPicking(false)} onPick={choose} />}
    </div>
  );
}

/** The pages a web search used, offered as sources. Nothing is fetched until the person asks, and adding sources
 *  means the plan is approved again before the next step, so the new pages are seen before they are used. */
function CitedPages({ ctx, stage }: { ctx: Ctx; stage: string }) {
  const [urls, setUrls] = useState<string[]>([]);
  const [done, setDone] = useState("");
  useEffect(() => { api.call<string[]>("cited_pages", { stage }).then(setUrls).catch(() => setUrls([])); }, [stage, ctx.s.artifacts.length]);
  const kept = new Set(ctx.s.sources.map((x) => x.origin));
  const fresh = urls.filter((u) => !kept.has(u));
  if (fresh.length === 0) return done ? <p className="muted" role="status">{done}</p> : null;
  const add = () => ctx.guard(async () => {
    let n = 0;
    for (const url of fresh) { const r = await api.call<{ added: unknown[] }>("add_source_url", { url }); n += r.added.length; }
    setDone(`${n} of ${fresh.length} pages added to Sources. Approve the plan again to continue.`);
  });
  return (
    <div className="notice cited-pages">
      <p><strong>The search used {fresh.length} page{fresh.length === 1 ? "" : "s"}.</strong> Add them to Sources so later steps can read them, not just the summary above.</p>
      <ul className="plain">{fresh.map((u) => <li key={u} className="mono small">{u}</li>)}</ul>
      <div className="actions tight"><button disabled={!!ctx.busy} onClick={() => void add()}>Add as sources</button></div>
      <p className="muted small">Adding sources changes what the plan reads, so you approve it again before the next step runs.</p>
    </div>
  );
}

/** What a second check said, before its text: the verdict and what it listed. A check that gave no verdict says so in
 *  amber, because it is not a pass. Its own text is folded away below, since it may be a rewrite and not a check. */
function CheckCard({ ctx, st, check }: { ctx: Ctx; st: Stage; check: Check }) {
  const plan = ctx.s.plan;
  const by = plan ? secondCheckOf(plan.team, plan.stages, (role) => sideOf(ctx, role) === "checking", st).replace(/^Second check by /, "") : "";
  const tone = check.verdict === "ready" ? "ok" : check.verdict === "do_not_send" ? "error" : "warn";
  return (
    <div className={`notice verdict ${tone}`} role="group" aria-label="What the second check said">
      <p className="verdict-by">Second check of the draft{by ? ` · ${by}` : ""}</p>
      <p className="verdict-head"><strong>{headline(check)}</strong></p>
      {check.verdict === null && <p className="muted">It did not end with a verdict, so SproutCrew cannot say whether the draft is ready. Run this step again, or read what it returned before you go on.</p>}
      {check.verdict && check.verdict !== "ready" && check.flagged.length > 0 && <ul>{check.flagged.map((f, i) => <li key={i}>{f}</li>)}</ul>}
    </div>
  );
}

function Focus({ ctx, st, status, a, onPick, onTalk }: { ctx: Ctx; st: Stage; status: Status; a?: Artifact; onPick: (id: string) => void; onTalk: () => void }) {
  const [body, setBody] = useState<{ file: string; text: string } | null>(null);
  const ready = ctx.s.gate === "allowed";
  const t = modelOf(ctx.s.plan?.team ?? [], st);
  const side = sideOf(ctx, st.owner_role);
  const r = a ? ctx.s.review?.[a.name] : undefined;
  const moved = ctx.s.stand_ins?.[st.id];
  const failed = ctx.s.failures?.[st.id];
  const check = a ? ctx.s.checks?.[st.id] : undefined;
  const held = ctx.s.blocked?.[st.id];
  useEffect(() => {
    if (!a || body?.file === a.file) return;
    let live = true;
    api.call<string>("read_artifact", { file: a.file }).then((text) => live && setBody({ file: a.file, text })).catch(() => undefined);
    return () => { live = false; };
  }, [a?.file]);
  return (
    <section className="focus" key={st.id} aria-live="polite">
      <header className="focus-head">
        <div>
          <h2>{st.title}</h2>
          <p className="muted who">
            <span className={`side ${side}`}>{side === "writing" ? "Writes" : "Checks"}</span>
            {roleName(st.owner_role)}
          </p>
          {t && <details className="model-details"><summary>AI details</summary><p className="muted">{provider(t.route)} · {t.model} · {effort(t.effort)} effort</p><button className="link" onClick={() => ctx.goTo("team")}>Change the model for this step</button></details>}
        </div>
        {!ctx.running && (
          <button className="ghost" disabled={!ready} onClick={() => ctx.guard(() => api.run("run_stage", { engine: ctx.engine, stageId: st.id }, ctx.track))}>
            {status === "done" || status === "attention" ? "Run again" : "Run this step"}
          </button>
        )}
      </header>
      {failed && <StepFailed ctx={ctx} st={st} why={failed} kind={ctx.s.failure_kinds?.[st.id]} earlier={!!a} onTalk={onTalk} />}
      {moved && !failed && <p className="notice">{provider(moved.from_route)} {moved.from_model} {moved.why}, so this step ran on {provider(moved.to_route)} {moved.to_model} instead.</p>}
      {st.guide && (
        <details className="ask">
          <summary>What this step asks for</summary>
          <p className="muted">{st.guide}</p>
        </details>
      )}
      {a ? (
        <>
          {body?.file === a.file && (
            <p className="muted source-note">{body.text.includes("came from the replay engine") ? "A practice result. No model wrote this." : "Written by a model. A person has not checked it yet."} <span className="tabular">{body.text.trim().split(/\s+/).filter(Boolean).length.toLocaleString("en-US")} words.</span></p>
          )}
          {check && <CheckCard ctx={ctx} st={st} check={check} />}
          {body?.file !== a.file ? <Skeleton label="Opening the result" lines={4} /> : check ? (
            <details className="check-text"><summary>{check.verdict ? "Read the full check" : "Read what it returned"}</summary><Markdown text={body.text} skipTitle={st.title} /></details>
          ) : <Markdown text={body.text} skipTitle={st.title} />}
          {st.id === "web_research" && <CitedPages ctx={ctx} stage={st.id} />}
          {r && <ReviewBlock ctx={ctx} name={a.name} r={r} />}
        </>
      ) : (
        <p className="empty-note">{held ? `Not started. ${held}.` : status === "running" ? "This step is being written now." : failed ? "No result yet." : ready ? "Not run yet. Its result will appear here." : "Approve the plan to start."}</p>
      )}
      <Checks ctx={ctx} st={st} onPick={onPick} />
      <StepTalk ctx={ctx} st={st} a={a} />
    </section>
  );
}

export default function WorkTab({ ctx }: { ctx: Ctx }) {
  const { s, engine, busy, guard, track } = ctx;
  const [picked, setPicked] = useState<string | null>(null);
  useEffect(() => {
    const raw = sessionStorage.getItem("sproutcrew:decision");
    if (!raw) return;
    try {
      const item = JSON.parse(raw) as { project: string; stage: string; anchor: string; kind: string };
      if (item.project !== s.name) return;
      sessionStorage.removeItem("sproutcrew:decision");
      setPicked(item.stage);
      if (item.kind === "proposal") setTalking(true);
      if (item.kind === "write") { sessionStorage.setItem("sproutcrew:write-decision", JSON.stringify(item)); ctx.goTo("share"); }
      requestAnimationFrame(() => requestAnimationFrame(() => document.getElementById(item.anchor)?.scrollIntoView({ block: "start" })));
    } catch { sessionStorage.removeItem("sproutcrew:decision"); }
  }, [s.name]);
  const plan = s.plan;
  const runningNow = runningIds(busy);
  const running = runningNow[0];
  // A step that starts running takes the focus, so the result appears where the eyes already are, and it keeps
  // the focus when the run ends, so the result stays on screen instead of jumping to another step.
  useEffect(() => { if (running) setPicked(running); }, [running]);
  const [talking, setTalking] = useState(false);
  const [gate, setGate] = useState(false);
  const estimate = useEstimate(gate ? JSON.stringify(s.plan) : null);
  const [view, setView] = useState<"list" | "diagram">("list");

  if (!plan) {
    return (
      <Panel title="Let’s turn your brief into a plan" help="Your crew will suggest the steps and who handles each one. You review and approve them before work starts.">
        <div className="actions">
          <button className="primary" disabled={!!busy} onClick={() => guard(() => api.run("propose_plan", { engine }, track))}>Suggest a plan</button>
        </div>
        <p className="muted">The crew reads your brief and proposes the steps and the team. You approve the plan before anything runs.</p>
      </Panel>
    );
  }

  const latest = (id: string) => [...s.artifacts].reverse().find((x) => x.name === id);
  const firstOpen = plan.stages.findIndex((st) => !latest(st.id));
  const done = plan.stages.filter((st) => latest(st.id)).length;
  const allDone = done === plan.stages.length;
  const approved = s.gate === "allowed";
  const doneIds = plan.stages.filter((st) => latest(st.id)).map((st) => st.id);
  const readyNow = readySteps(plan.stages, doneIds);
  const statusOf = (st: Stage, i: number): Status => (runningNow.includes(st.id) ? "running" : s.failures?.[st.id] ? "failed" : latest(st.id) ? (ticked(s.checks?.[st.id]) ? "done" : "attention") : readyNow.includes(st.id) || i === firstOpen ? "next" : "waiting");
  const focusId = picked ?? running ?? (firstOpen >= 0 ? plan.stages[firstOpen].id : plan.stages[plan.stages.length - 1].id);
  const focusIx = Math.max(0, plan.stages.findIndex((x) => x.id === focusId));
  // The run itself is in `runner`, so the board can start it too. This page follows it while it is open.
  const runAll = () => guard(async () => {
    try { await runner.start(s.name, engine); }
    // The run is over, so the focus goes to what comes next, or to the last step, which is the finished work.
    finally { setPicked(null); }
  });

  // After approval the crew asks for facts only the person knows, and the run waits here until they answer or skip.
  if (approved && asking(s.blocked)) return <Questions ctx={ctx} onAnswered={() => void runAll()} />;

  // What the run bar says. Stop is the only thing that reads "Stopped by you": a run held for answers or on a
  // "Do not send" verdict is shown as the steps done so far, with what is holding it.
  const stopped = !!s.stopped && approved && !allDone && !ctx.running;
  const failedAny = plan.stages.some((st) => s.failures?.[st.id]);
  const heading = ctx.stopping ? "Finishing the step that was running…" : stopped ? stoppedLine(done) : allDone ? "All steps have results" : approved ? `${done} of ${plan.stages.length} steps done` : "Ready when you are";
  const pill = ctx.stopping ? <StatusPill state="cancelled" word="Stopping" /> : stopped ? <StatusPill state="cancelled" word="Stopped" /> : !approved ? <StatusPill state="approval" /> : busy ? <StatusPill state="running" /> : failedAny ? <StatusPill state="failed" /> : allDone ? <StatusPill state="done" /> : <StatusPill state="queued" />;
  const sub = ctx.stopping ? "You pressed Stop. The step that was running finishes and is kept. Nothing new starts."
    : stopped ? `Nothing is running. ${done} of ${plan.stages.length} steps have results. Run the rest when you are ready.`
    : allDone ? "Your results are ready to review. Choose what to keep, improve, or export."
    : approved ? "Approved. Your crew uses the sources and earlier results as it works."
    : "Review the suggested steps below. Nothing starts until you approve the plan.";
  // Run the rest does something only when a step can start. When every step left is held, it stays out of the way.
  const canRunRest = readyNow.some((id) => !s.blocked?.[id]);

  return (
    <>
      <div className="runbar" id="plan-approval">
        <div className="runbar-text">
          <h2 id="run-status" tabIndex={-1}>{heading} {pill}</h2>
          <p className="muted">
            {sub}
            {s.gate === "plan_changed" && " The approved plan, permissions, or source context changed."}
            {ctx.dirty && " There are unsaved edits to the plan. Save them in Your crew first."}
          </p>
        </div>
        <div className="runbar-actions">
          {!approved && <button className="primary big" disabled={ctx.dirty || !!busy || s.problems.length > 0} onClick={() => setGate(true)}>Review the plan</button>}
          {approved && !allDone && !ctx.running && canRunRest && <button className="primary big" disabled={!!busy} onClick={runAll}>{done === 0 ? "Run all steps" : "Run the rest"}</button>}
          <button className="ghost quiet" aria-expanded={talking} onClick={() => setTalking(!talking)}>Ask for a change</button>
          {ctx.running && !ctx.stopping && <button onClick={() => window.dispatchEvent(new Event("bff:stop"))} title="Stop the run (⌘.)">Stop <kbd>⌘.</kbd></button>}
          {s.app?.opens && <button className={allDone ? "ghost" : ""} disabled={!!busy} onClick={() => guard(() => api.call("start_preview"))}>Open the app</button>}
          {allDone && <button className="primary big" onClick={() => ctx.goTo("share")}>View results & export</button>}
        </div>
      </div>
      <RequirementsNotice sources={s.sources} onMaterial={() => ctx.goTo("material")} />
      <InjectionNotice sources={s.sources} />
      <ActionPolicy ctx={ctx} />
      <CodeReview ctx={ctx} />
      {gate && plan && (
        <ReviewBoard ctx={ctx} plan={plan} estimate={estimate} onClose={() => setGate(false)}
          onApprove={(start) => {
            setGate(false);
            // The opener is gone once the plan is approved, so the focus goes to the run's status line.
            // The version the card showed goes back with the approval; a plan that changed since is refused.
            void guard(() => api.call("approve", { project: s.name, hash: s.approval_hash ?? "" })).then(() => {
              requestAnimationFrame(() => {
                // The run bar is brought below the sticky tabs first, so focusing its status line does not scroll it back under them.
                document.getElementById("plan-approval")?.scrollIntoView({ block: "start" });
                document.getElementById("run-status")?.focus({ preventScroll: true });
              });
              if (start) void runAll();
            });
          }} />
      )}
      {allDone ? <FinalFiles ctx={ctx} /> : <SecondCheckResult ctx={ctx} />}
      {!approved && <details className="plan-customize"><summary>Adjust the suggested steps</summary><Shape ctx={ctx} plan={plan} onTalk={() => setTalking(true)} /><StepModels ctx={ctx} plan={plan} /></details>}
      {talking && <Coordinator ctx={ctx} onClose={() => setTalking(false)} />}
      {s.problems.length > 0 && <ul className="field-error">{s.problems.map((p) => <li key={p}>{p}</li>)}</ul>}

      <div className="viewbar">
        <h3>Your steps <span className="meta">{done} of {plan.stages.length} complete</span></h3>
        <div className="seg" role="group" aria-label="How to show the steps">
          <button aria-pressed={view === "list"} onClick={() => setView("list")}>List</button>
          <button aria-pressed={view === "diagram"} onClick={() => setView("diagram")}>Diagram</button>
        </div>
      </div>
      {view === "diagram" && (
        <Diagram
          steps={plan.stages.map((st, i) => ({ id: st.id, title: st.title, side: sideOf(ctx, st.owner_role), needs: needsOf(plan.stages, i) }))}
          status={(id) => { const i = plan.stages.findIndex((x) => x.id === id); return statusOf(plan.stages[i], i); }}
          runs={(id) => runsOf(ctx.s.artifacts, id)}
          detail={(id) => {
            const rows = (ctx.s.usage_rows ?? []).filter((r) => r.stage === id);
            const last = rows[rows.length - 1];
            if (!last) return "";
            return detailLine(last.model, ctx.s.timings[last.task]?.seconds ?? null, last.dollars, last.dollars !== null);
          }}
          who={(id) => roleName(plan.stages.find((x) => x.id === id)!.owner_role)}
          focus={focusId}
          onPick={setPicked}
        />
      )}
      <div className={`split${view === "diagram" ? " single" : ""}`}>
        {view === "list" && <ol className="crew" aria-label="Steps">
          {plan.stages.map((st, i) => {
            const status = statusOf(st, i);
            const side = sideOf(ctx, st.owner_role);
            const flowing = (status === "done" || status === "attention") && i + 1 < plan.stages.length && statusOf(plan.stages[i + 1], i + 1) === "running";
            return (
              <li key={st.id} id={`step-${st.id}`} className={`node ${status} ${side}${st.id === focusId ? " sel" : ""}${flowing ? " flowing" : ""}${i + 1 === plan.stages.length ? " last" : ""}`}>
                <button className="node-btn" aria-current={st.id === focusId ? "step" : undefined} onClick={() => setPicked(st.id)} title={`${st.title}. ${STATUS_WORD[status]}`}>
                  <Mark status={status} />
                  <span className="node-text">
                    <strong>{st.title}</strong>
                    <span className="meta">{roleName(st.owner_role)} · {STATUS_WORD[status].toLowerCase()}</span>
                  </span>
                  <span className="node-time">{(() => { const a = latest(st.id); const t = a && s.timings[a.from_task]; return t ? secs(t.seconds) : ""; })()}</span>
                </button>
              </li>
            );
          })}
        </ol>}
        <Focus onPick={setPicked} onTalk={() => setTalking(true)} ctx={ctx} st={plan.stages[focusIx]} status={statusOf(plan.stages[focusIx], focusIx)} a={latest(plan.stages[focusIx].id)} />
      </div>
    </>
  );
}

export { text };
