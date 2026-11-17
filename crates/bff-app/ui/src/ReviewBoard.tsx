import { KeyboardEvent, useEffect, useRef, useState } from "react";
import { api, Permission, Plan, ProjectLimit } from "./api";
import { modelOf } from "./graph";
import { useModelName } from "./ModelPicker";
import Preflight, { Estimate } from "./Preflight";
import { Ctx, roleName } from "./ui";

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
        {value === "varies_by_role" && <span className="meta">Set per role below</span>}
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

/** The plan approval as a review board: what will run and on which models, the guardrails as visible choices, and
 *  what it could cost, with the buttons always on screen. Approving binds exactly what is shown here; changing any
 *  of it afterwards needs a new approval, as before. The brief and every step are one click away. */
export default function ReviewBoard({ ctx, plan, estimate, onClose, onApprove }: { ctx: Ctx; plan: Plan; estimate: Estimate | null; onClose: () => void; onApprove: (start: boolean) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const [steps, setSteps] = useState(false);
  const p = ctx.s.policy;
  const g = p?.guardrails;
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    box.current?.focus();
    return () => before?.focus?.();
  }, []);
  const models = (side: "writing" | "checking") => [...new Set(plan.stages.filter((st) => sideOf(ctx, st.owner_role) === side).map((st) => modelOf(plan.team, st)?.model).filter((m): m is string => !!m))];
  const writing = models("writing"), checking = models("checking");
  const sameMaker = checking.length > 0 && checking.every((c) => writing.some((w) => w.split("/")[0] === c.split("/")[0]));
  const builds = plan.stages.some((st) => /build|code|app/.test(st.id));
  const keys = (e: KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onClose(); }
    else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); if (!ctx.busy) onApprove(true); }
  };
  return (
    <div className="scrim">
      <div ref={box} className="gate review-board" role="dialog" aria-modal="true" aria-labelledby="board-title" tabIndex={-1} onKeyDown={keys}>
        <header>
          <p className="eyebrow">Your approval is needed</p>
          <h2 id="board-title" className="gate-title">Review the plan</h2>
        </header>
        <div className="board-body">
          <section className="board-card" aria-labelledby="card-plan">
            <h3 id="card-plan">The plan</h3>
            <p><strong>{plan.stages.length} steps.</strong> Written by <Models ids={writing} />. {checking.length > 0 && <>Checked by <Models ids={checking} />{sameMaker ? ", from the same maker, so the check is a second pass rather than an independent opinion." : ", a different AI."}</>}</p>
            {(ctx.s.will_skip ?? []).map((w) => (
              <p key={w.id} className="notice warn" role="note">“{w.title}” will be skipped: the step it works on is no longer in the plan. The rest of the run goes on without it.</p>
            ))}
            <button className="link" aria-expanded={steps} onClick={() => setSteps(!steps)}>{steps ? "Hide the steps" : "See every step"}</button>
            {steps && (
              <ol className="board-steps">
                {plan.stages.map((st) => { const t = modelOf(plan.team, st); return <li key={st.id}>{st.title} <span className="muted">· {roleName(st.owner_role)}{t ? ` · ${t.model}` : ""}</span></li>; })}
              </ol>
            )}
            <details className="board-brief"><summary>Brief</summary><p className="muted">{plan.brief}</p></details>
          </section>

          <section className="board-card" aria-labelledby="card-guard">
            <h3 id="card-guard">Guardrails</h3>
            {!g ? <p className="muted">Loading the guardrails.</p> : (
              <>
                <GuardChoice ctx={ctx} tool="external_send" value={g.send_outside} label="Send anything outside this Mac" help="Posting, publishing or writing to Notion or GitHub." />
                <GuardChoice ctx={ctx} tool="delete" value={g.delete_files} label="Delete files" help="Removing anything the crew made or was given." />
                {builds && <GuardChoice ctx={ctx} tool="dependency_install" value={g.install_software} label="Install software" help="Adding packages while building an app." />}
                <KeepChoice ctx={ctx} plan={plan} />
                <KeynoteChoice ctx={ctx} plan={plan} />
                <p className="locked"><span aria-hidden="true">🔒</span> Run commands on this Mac: never.</p>
                {g.checkers_read_only && <p className="locked"><span aria-hidden="true">🔒</span> Checkers can read the work, never change it.</p>}
                <details className="per-role">
                  <summary>Set per role</summary>
                  <PerRole ctx={ctx} />
                </details>
              </>
            )}
          </section>

          <section className="board-card" aria-labelledby="card-cost">
            <h3 id="card-cost">Cost and limits</h3>
            <Preflight e={estimate} />
            {ctx.s.project_limit && <LimitLine l={ctx.s.project_limit} max={estimate?.max_usd ?? null} />}
            <p className="muted">{ctx.s.budget ? `This project stops at ${ctx.s.budget.limit.toLocaleString("en-US")} tokens.` : "No token limit is set for this project; the limit shared by all projects still applies."} <button className="link" onClick={() => { onClose(); ctx.goTo("usage"); }}>Change limits</button></p>
          </section>
        </div>
        <footer className="board-footer">
          <p className="muted">Changing the plan, sources, guardrails or limits needs a new approval.</p>
          <div className="actions">
            <button className="ghost" onClick={onClose}>Cancel <kbd>Esc</kbd></button>
            <button disabled={!!ctx.busy} onClick={() => onApprove(false)}>Approve only</button>
            <button className="primary" disabled={!!ctx.busy} onClick={() => onApprove(true)}>Approve and start <kbd>⌘⏎</kbd></button>
          </div>
        </footer>
      </div>
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
      <p className="muted">Sending, deleting and installing can never be set to Allow; each one asks you. Checkers stay read-only.</p>
    </div>
  );
}

/** The project's dollar limit for paid models, and a warning when the plan's most could pass what is left. */
function LimitLine({ l, max }: { l: ProjectLimit; max: number | null }) {
  const left = Math.max(0, l.cap - l.spent - l.held);
  return (
    <>
      <p>Paid models in this project stop at <strong>${l.cap.toFixed(2)}</strong>; ${(l.spent + l.held).toFixed(2)} is used so far.</p>
      {max !== null && max > left && <p className="notice warn" role="alert">This plan could cost up to ${max.toFixed(2)}, more than the ${left.toFixed(2)} left under the limit. Steps pause when it is reached, until you raise it.</p>}
    </>
  );
}
