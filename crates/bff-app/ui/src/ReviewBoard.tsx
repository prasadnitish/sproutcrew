import { KeyboardEvent, useEffect, useRef, useState } from "react";
import { api, ApiRoute, Permission, Plan, ProjectLimit } from "./api";
import { LimitForm } from "./Cost";
import { modelOf } from "./graph";
import { replaceModel, scopeOf, sideModels } from "./modelChange";
import { secondCheckOf } from "./secondcheck";
import ModelPicker, { useModelName } from "./ModelPicker";
import { planSentence } from "./planSentence";
import Preflight, { Estimate } from "./Preflight";
import { Ctx, roleName, shortHash, text } from "./ui";

export type Guard = "ask_each_time" | "never" | "varies_by_role";
export type Guardrails = { send_outside: Guard; delete_files: Guard; install_software: Guard; run_commands: Guard; checkers_read_only: boolean };

/** The columns of the per-role table in plain words. */
const TOOL_WORD: Record<string, string> = {
  read: "Read sources", artifact: "Return results", write: "Change files", shell: "Run commands",
  external_send: "Send outside", delete: "Delete", dependency_install: "Install",
};

const sideOf = (ctx: Ctx, role: string) => ctx.s.role_info?.[role]?.side ?? (role === "reviewer" ? "checking" : "writing");

/** "Claude Haiku 5.5", or "Claude Haiku 5.5 and 2 others", for the models one side of the crew uses. */
function Models({ ids }: { ids: string[] }) {
  const first = useModelName(ids[0] ?? "");
  if (ids.length === 0) return <>no model</>;
  return <>{first.replace(/^[^:]+:\s*/, "")}{ids.length > 1 ? ` and ${ids.length - 1} other${ids.length > 2 ? "s" : ""}` : ""}</>;
}

/** One guardrail as a choice the person makes, with the default already chosen. */
function GuardChoice({ label, help, value, tool, ctx }: { label: string; help: string; value: Guard; tool: string; ctx: Ctx }) {
  const set = (ask: boolean) => void ctx.guard(() => api.call("set_guardrail", { tool, ask }));
  return (
    <fieldset className="guard">
      <legend>{label}</legend>
      <p className="muted">{help}</p>
      <div className="guard-options" role="radiogroup" aria-label={label}>
        <label className={value === "ask_each_time" ? "on" : ""}><input type="radio" name={tool} checked={value === "ask_each_time"} disabled={!!ctx.busy} onChange={() => set(true)} /> Ask me each time</label>
        <label className={value === "never" ? "on" : ""}><input type="radio" name={tool} checked={value === "never"} disabled={!!ctx.busy} onChange={() => set(false)} /> Never</label>
        {value === "varies_by_role" && <span className="meta">Set for each role in the table</span>}
      </div>
    </fieldset>
  );
}

/** What the AI providers may keep of what they are sent. Keeping nothing is part of the plan, so changing it needs a
 *  new approval; a plan that cannot keep the promise is refused with the reason, and nothing changes. */
function KeepChoice({ ctx, plan }: { ctx: Ctx; plan: Plan }) {
  const [why, setWhy] = useState<string[]>([]);
  const on = plan.privacy === "no_retention";
  const set = (keepNothing: boolean) => void ctx.guard(async () => {
    try {
      await api.call("save_plan", { plan: { ...plan, privacy: keepNothing ? "no_retention" : "standard" } });
      setWhy([]);
    } catch (e) { setWhy(Array.isArray(e) ? e.map(String) : [String(e)]); }
  });
  return (
    <fieldset className="guard">
      <legend>What AI providers may keep</legend>
      <p className="muted">Your brief, sources and results are sent to the AI provider for each step.</p>
      <div className="guard-options" role="radiogroup" aria-label="What AI providers may keep">
        <label className={!on ? "on" : ""}><input type="radio" name="privacy" checked={!on} disabled={!!ctx.busy} onChange={() => set(false)} /> Their own policy</label>
        <label className={on ? "on" : ""}><input type="radio" name="privacy" checked={on} disabled={!!ctx.busy} onChange={() => set(true)} /> Nothing</label>
      </div>
      {on && <p className="muted">Every step runs on OpenRouter, sent only to endpoints that keep no prompts or answers. Web research is off, because the search is not covered.</p>}
      {why.length > 0 && <div className="field-error" role="alert"><p>Not changed:</p><ul>{why.map((w) => <li key={w}>{w}</li>)}</ul></div>}
    </fieldset>
  );
}

/** Keynote building the deck: a permission of its own, shown only for a plan that makes slides on a Mac with
 *  Keynote. It changes the plan, so it is approved with it. */
function KeynoteChoice({ ctx, plan }: { ctx: Ctx; plan: Plan }) {
  const [installed, setInstalled] = useState(false);
  const [why, setWhy] = useState<string[]>([]);
  useEffect(() => { api.call<boolean>("keynote_installed").then(setInstalled).catch(() => setInstalled(false)); }, []);
  if (!installed || !plan.stages.some((st) => st.id.includes("deck"))) return null;
  const on = !!plan.keynote;
  const set = (v: boolean) => void ctx.guard(async () => {
    try { await api.call("set_keynote", { on: v }); setWhy([]); } catch (e) { setWhy(Array.isArray(e) ? e.map(String) : [String(e)]); }
  });
  return (
    <fieldset className="guard">
      <legend>Build the deck in Keynote</legend>
      <p className="muted">Keynote makes a new deck from the slides, saved with this project’s files, up to 50 slides. It never opens or changes your other files. The deck is then read back and checked against the slides.</p>
      <div className="guard-options" role="radiogroup" aria-label="Build the deck in Keynote">
        <label className={!on ? "on" : ""}><input type="radio" name="keynote" checked={!on} disabled={!!ctx.busy} onChange={() => set(false)} /> No</label>
        <label className={on ? "on" : ""}><input type="radio" name="keynote" checked={on} disabled={!!ctx.busy} onChange={() => set(true)} /> Yes, let SproutCrew use Keynote</label>
      </div>
      {on && <p className="muted">The first time, macOS asks whether SproutCrew may control Keynote. Choose OK. You can change this later in System Settings, Privacy &amp; Security, Automation.</p>}
      {why.length > 0 && <div className="field-error" role="alert"><p>Not changed:</p><ul>{why.map((w) => <li key={w}>{w}</li>)}</ul></div>}
    </fieldset>
  );
}

/** The three promises under "While it works", each as one plain sentence. */
function guardLine(v: Guard, ask: string, never: string, varies: string): string {
  return v === "never" ? never : v === "ask_each_time" ? ask : varies;
}

/** "notes.pdf, data.csv and 1 more": the first two names, then how many are left. */
function firstNames(names: string[]): string {
  const [a, b] = names;
  return names.length <= 2 ? names.join(" and ") : `${a}, ${b} and ${names.length - 2} more`;
}

/** The plan approval as one card: what the crew will do in a sentence, what it costs, what it reads, which models
 *  and what limit, and what it may do while it works as a few short lines. Every choice that used to be on the
 *  earlier plan screen is still here, under Change. Approving binds exactly what the card shows; changing any of it
 *  afterwards needs a new approval, as before. It is a modal: Tab stays inside, Esc closes it from anywhere in it,
 *  and focus goes back to what opened it. */
export default function ReviewBoard({ ctx, plan, estimate, onClose, onApprove }: { ctx: Ctx; plan: Plan; estimate: Estimate | null; onClose: () => void; onApprove: (start: boolean) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState<"" | "models" | "limit" | "guard">("");
  const [picking, setPicking] = useState<"" | "writing" | "checking">("");
  const [err, setErr] = useState("");
  const [noKey, setNoKey] = useState(false);
  const p = ctx.s.policy;
  const g = p?.guardrails;
  useEffect(() => { api.call<ApiRoute[]>("api_routes").then((r) => setNoKey(r.some((x) => x.route === "R12" && x.state === "no_key"))).catch(() => undefined); }, []);
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    box.current?.focus();
    return () => before?.focus?.();
  }, []);
  const toggle = (what: "models" | "limit" | "guard") => setOpen(open === what ? "" : what);
  const checking = (role: string) => sideOf(ctx, role) === "checking";
  // A refused change shows here, inside the card, because the app's own message sits behind it.
  const cardCtx: Ctx = { ...ctx, guard: (f) => ctx.guard(async () => { try { await f(); setErr(""); } catch (e) { setErr(text(e)); throw e; } }) };
  const writing = sideModels(plan, (r) => !checking(r)), checkers = sideModels(plan, checking);
  const side = (which: "writing" | "checking") => {
    const onSide = which === "checking" ? checking : (r: string) => !checking(r);
    const from = (which === "checking" ? checkers : writing)[0] ?? "";
    return { onSide, from, ...scopeOf(plan, onSide, from) };
  };
  const secondCheck = secondCheckOf(plan.team, plan.stages, checking);
  // The company note comes from the one place that decides it, so the card and the sentence cannot disagree.
  const company = /, (a different|the same) company$/.exec(secondCheck)?.[1];
  const sentence = planSentence(plan.stages, "", company ? company === "the same" : null, { label: (r) => roleName(r).toLowerCase(), checking });
  const builds = plan.stages.some((st) => /build|code|app/.test(st.id));
  const names = ctx.s.sources.map((x) => x.title || x.name);
  // Only steps already on OpenRouter are changed here, each keeping its route and effort; the web step is left alone.
  const choose = (which: "writing" | "checking", model: string) => {
    setPicking("");
    const { onSide, from } = side(which);
    const stages = replaceModel(plan, onSide, from, model);
    if (stages) void cardCtx.guard(() => api.call("save_plan", { plan: { ...plan, stages } }));
  };
  // Steps on another route are changed one step at a time, which keeps each step's own provider.
  const stepByStep = () => {
    onClose();
    setTimeout(() => {
      const d = document.querySelector<HTMLDetailsElement>(".plan-customize");
      if (d) { d.open = true; d.scrollIntoView({ block: "start" }); d.querySelector("summary")?.focus({ preventScroll: true }); }
    }, 0);
  };
  const keys = (e: KeyboardEvent) => {
    const el = box.current;
    // Keys from the model picker, which sits beside the card, belong to the picker.
    if (!el || (e.target !== e.currentTarget && !el.contains(e.target as Node))) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onClose(); }
    else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); if (!ctx.busy) onApprove(true); }
    else if (e.key === "Tab") {
      const f = [...el.querySelectorAll<HTMLElement>("button:not(:disabled), summary, input:not(:disabled), select:not(:disabled), [tabindex='0']")].filter((x) => x.getClientRects().length > 0);
      if (f.length === 0) return;
      const first = f[0], last = f[f.length - 1], at = document.activeElement;
      if (e.shiftKey && (at === first || at === el || !el.contains(at))) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && (at === last || !el.contains(at))) { e.preventDefault(); first.focus(); }
    }
  };
  return (
    <div className="scrim" onKeyDown={keys} onMouseDown={(e) => { if (e.target === e.currentTarget) { e.preventDefault(); box.current?.focus(); } }}>
      <div ref={box} className="gate plan-card" role="dialog" aria-modal="true" aria-labelledby="board-title" tabIndex={-1}>
        <header>
          <p className="eyebrow">Your approval is needed</p>
          <h2 id="board-title" className="gate-title">Here’s the plan</h2>
        </header>
        <div className="plan-body">
          <p className="plan-sentence">{sentence}</p>
          <ol className="plan-chips" aria-label={`${plan.stages.length} steps`}>
            {plan.stages.map((st) => <li key={st.id} className={`plan-chip ${checking(st.owner_role) ? "check" : ""}`}>{st.title}</li>)}
          </ol>
          <section className="plan-guard" aria-labelledby="while-it-works">
            <h3 id="while-it-works">While it works <button className="link" aria-expanded={open === "guard"} onClick={() => toggle("guard")}>Change</button></h3>
            {!g ? <p className="muted">Loading what it may do.</p> : (
              <ul className="plain guard-lines">
                <li>{guardLine(g.send_outside, "Asks you before it sends anything outside this Mac.", "Never sends anything outside this Mac.", "Sending outside this Mac depends on the role.")}</li>
                <li>{guardLine(g.delete_files, "Asks you before it deletes files.", "Never deletes files.", "Deleting files depends on the role.")}</li>
                <li>{guardLine(g.run_commands, "Asks you before it runs commands on this Mac.", "Never runs commands on this Mac.", "Running commands depends on the role.")}{g.checkers_read_only ? " The second check reads the work and never changes it." : ""}</li>
                {builds && <li>{guardLine(g.install_software, "Asks you before it installs software.", "Never installs software.", "Installing software depends on the role.")}</li>}
                <li>{plan.privacy === "no_retention" ? "AI providers keep nothing." : "AI providers may keep what they're sent, as their own terms say."}</li>
                {plan.keynote && <li>Builds the deck in Keynote.</li>}
              </ul>
            )}
            {open === "guard" && g && (
              <div className="plan-guard-open">
                <GuardChoice ctx={cardCtx} tool="external_send" value={g.send_outside} label="Send anything outside this Mac" help="Posting, publishing or writing to Notion or GitHub." />
                <GuardChoice ctx={cardCtx} tool="delete" value={g.delete_files} label="Delete files" help="Removing anything the crew made or was given." />
                {builds && <GuardChoice ctx={cardCtx} tool="dependency_install" value={g.install_software} label="Install software" help="Adding packages while building an app." />}
                <KeepChoice ctx={cardCtx} plan={plan} />
                <KeynoteChoice ctx={cardCtx} plan={plan} />
                <h4>What each role may do</h4>
                <PerRole ctx={cardCtx} />
              </div>
            )}
          </section>
          {(ctx.s.will_skip ?? []).map((w) => (
            <p key={w.id} className="notice warn" role="note">“{w.title}” will be skipped: the step it works on is no longer in the plan. The rest of the run goes on without it.</p>
          ))}
          <dl className="plan-facts">
            <div>
              <dt>Cost</dt>
              <dd><Preflight e={estimate} compact /></dd>
            </div>
            <div>
              <dt>The crew reads</dt>
              <dd>
                Your brief{names.length > 0 ? <> and {names.length} {names.length === 1 ? "source" : "sources"}: {firstNames(names)}</> : null}.
                <details className="board-brief">
                  <summary>Every step and the brief</summary>
                  <ol className="board-steps">
                    {plan.stages.map((st) => { const t = modelOf(plan.team, st); return <li key={st.id}>{st.title} <span className="muted">· {roleName(st.owner_role)}{t ? ` · ${t.model}` : ""}</span></li>; })}
                  </ol>
                  <p className="muted">{plan.brief}</p>
                </details>
              </dd>
            </div>
            <div>
              <dt>Models</dt>
              <dd>
                Writer <Models ids={writing} />
                {checkers.length > 0 && <> · Second check <Models ids={checkers} />{company ? `, ${company} company` : ""}</>}
                {" · "}<button className="link" aria-expanded={open === "models"} onClick={() => toggle("models")}>Change</button>
                {open === "models" && (["writing", "checking"] as const).filter((w) => w === "writing" || checkers.length > 0).map((w) => {
                  const sc = side(w);
                  const what = w === "writing" ? "the writer" : "the second check";
                  const steps = `${sc.steps.length} step${sc.steps.length === 1 ? "" : "s"}`;
                  return (
                    <span key={w} className="block">
                      {sc.onOpenRouter
                        ? <button className="ghost" disabled={noKey || !!ctx.busy} onClick={() => setPicking(w)}>Change {what} ({steps})</button>
                        : <button className="ghost" onClick={stepByStep}>Change {what} step by step</button>}
                      {!sc.onOpenRouter && <span className="muted small block">Some of these steps run on another provider, so they are changed one step at a time and keep their provider.</span>}
                      {sc.onOpenRouter && noKey && <span className="muted small block">An OpenRouter key is needed to pick a model here. Add one in Settings.</span>}
                    </span>
                  );
                })}
                <span className="muted small block">A model you choose goes into the plan. Nothing is sent until you approve it.</span>
              </dd>
            </div>
            {ctx.s.project_limit && (
              <div>
                <dt>Limit for this project</dt>
                <dd>
                  ${ctx.s.project_limit.cap.toFixed(2)} · <button className="link" aria-expanded={open === "limit"} onClick={() => toggle("limit")}>Change</button>
                  <LimitLine l={ctx.s.project_limit} max={estimate?.max_usd ?? null} />
                  {open === "limit" && <LimitForm busy={!!ctx.busy} guard={ctx.guard} button="Set the limit" />}
                  <span className="muted small block">
                    {ctx.s.budget ? `It also stops at ${ctx.s.budget.limit.toLocaleString("en-US")} tokens.` : "The limit shared by all projects still applies."} <button className="link" onClick={() => { onClose(); ctx.goTo("usage"); }}>Change limits</button>
                  </span>
                </dd>
              </div>
            )}
          </dl>
        </div>
        <footer className="board-footer">
          {err && <p className="field-error" role="alert">{err}</p>}
          <p className="muted">Changing the plan, sources, what it may do or the token limit needs a new approval. The project’s dollar limit can be changed any time.</p>
          {ctx.s.approval_hash && <p className="muted small">This approval covers version <span className="mono" title={ctx.s.approval_hash}>{shortHash(ctx.s.approval_hash)}</span>. If anything in it changes before you approve, the approval is refused and nothing runs.</p>}
          <div className="actions">
            <button className="ghost" onClick={onClose}>Cancel <kbd>Esc</kbd></button>
            <button disabled={!!ctx.busy} onClick={() => onApprove(false)}>Approve only</button>
            <button className="primary" disabled={!!ctx.busy} onClick={() => onApprove(true)}>Approve and start <kbd>⌘⏎</kbd></button>
          </div>
        </footer>
      </div>
      {picking && <ModelPicker side={picking} value={side(picking).from} onClose={() => setPicking("")} onPick={(id) => choose(picking, id)} />}
    </div>
  );
}

/** The full role-by-action table for people who want it. Running commands is fixed off. */
export function PerRole({ ctx }: { ctx: Ctx }) {
  const p = ctx.s.policy;
  if (!p) return null;
  const approved = ctx.s.gate === "allowed";
  const tools = Object.keys(Object.values(p.matrix)[0] ?? {});
  return (
    <div className="per-role-table">
      <table>
        <thead><tr><th>Role</th>{tools.map((t) => <th key={t}>{TOOL_WORD[t] ?? t}</th>)}</tr></thead>
        <tbody>
          {Object.entries(p.matrix).map(([role, row]) => (
            <tr key={role}>
              <th scope="row">{roleName(role)}</th>
              {Object.entries(row).map(([tool, permission]) => (
                <td key={tool}>
                  <select aria-label={`${roleName(role)}: ${TOOL_WORD[tool] ?? tool}`} value={permission} disabled={approved || !!ctx.busy || tool === "shell"}
                    onChange={(e) => { const matrix = { ...p.matrix, [role]: { ...row, [tool]: e.target.value as Permission } }; void ctx.guard(() => api.call("set_action_policy", { matrix })); }}>
                    <option value="allow">Allow</option><option value="ask">Ask</option><option value="deny">Never</option>
                  </select>
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="muted">Sending, deleting and installing can never be set to Allow; each one asks you. The second check stays read-only.</p>
    </div>
  );
}

/** A warning when the plan's most could pass what is left under the project's dollar limit. */
function LimitLine({ l, max }: { l: ProjectLimit; max: number | null }) {
  const left = Math.max(0, l.cap - l.spent - l.held);
  return (
    <>
      <span className="muted small block">${(l.spent + l.held).toFixed(2)} is used so far. Paid models stop at the limit.</span>
      {max !== null && max > left && <span className="notice warn block" role="alert">This plan could cost up to ${max.toFixed(2)}, more than the ${left.toFixed(2)} left under the limit. Steps pause when it is reached, until you raise it.</span>}
    </>
  );
}
