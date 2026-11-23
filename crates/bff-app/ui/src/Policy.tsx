import { api } from "./api";
import { secondCheckOf } from "./secondcheck";
import { Ctx, roleName } from "./ui";

const TOOL_WORD: Record<string, string> = { external_send: "Send outside this Mac", delete: "Delete", dependency_install: "Install software", write: "Change files" };

/** What the project may do while it works, in chips under Details. The button goes to the Plan tab, where the plan card opens. */
export function GuardStrip({ ctx }: { ctx: Ctx }) {
  const p = ctx.s.policy;
  const g = p?.guardrails; // the "While it works" choices
  if (!g || !ctx.s.plan) return null;
  const checking = (role: string) => ctx.s.role_info?.[role]?.side === "checking";
  const checked = Object.values(ctx.s.role_info ?? {}).some((r) => r.side === "checking") ? secondCheckOf(ctx.s.plan.team, ctx.s.plan.stages, checking) : "";
  const word = (v: string, ask: string, never: string) => (v === "never" ? never : v === "ask_each_time" ? ask : `${ask.split(" ")[0]}: varies by role`);
  const chips = [
    checked,
    word(g.send_outside, "Asks before sending", "Never sends outside"),
    word(g.delete_files, "Asks before deleting", "Never deletes"),
    "Never runs commands",
    ctx.s.plan.privacy === "no_retention" ? "Providers keep nothing" : "",
    ctx.s.plan.keynote ? "Builds the deck in Keynote" : "",
    ctx.s.project_limit ? `Paid models stop at $${ctx.s.project_limit.cap.toFixed(2)}` : "",
    ctx.s.budget ? `Stops at ${ctx.s.budget.limit.toLocaleString("en-US")} tokens` : "",
    ctx.s.gate === "allowed" ? "Approved" : ctx.s.gate === "plan_changed" ? "Changed since approval" : "Not approved yet",
  ].filter(Boolean);
  return (
    <p className="guard-strip" aria-label="While it works">
      {chips.map((c) => <span key={c} className={`guard-chip${c === "Approved" ? " ok" : c.startsWith("Not") || c.startsWith("Changed") ? " warn" : ""}`}>{c}</span>)}
      <button className="link" onClick={() => { ctx.goTo("work"); setTimeout(() => document.getElementById("plan-approval")?.scrollIntoView({ behavior: "smooth" }), 0); }}>Open the plan</button>
    </p>
  );
}

/** Actions the crew asked to take, waiting for the person, in plain words. */
export default function ActionPolicy({ ctx }: { ctx: Ctx }) {
  const p = ctx.s.policy;
  if (!p || p.pending.length === 0) return null;
  const reversible = p.pending.filter((a) => a.tier === "reversible");
  const decide = (ids: string[], approve: boolean) => ctx.guard(() => api.call("decide_actions", { project: ctx.s.name, ids, approve }));
  return (
    <section className="pending-actions" aria-label="Waiting for you">
      {p.pending.map((a) => (
        <article className="notice" key={a.id} id={`decision-${a.id}`}>
          <p><strong>{roleName(a.action.stage)} wants to {(TOOL_WORD[a.action.tool] ?? a.action.tool.replace(/_/g, " ")).toLowerCase()}:</strong> {a.action.target}</p>
          <p className="muted">{a.tier === "reversible" ? "This can be undone." : a.tier === "destructive" ? "This cannot be undone." : "This reaches outside the project."} Only this exact action is approved.</p>
          <div className="actions tight">
            <button disabled={!!ctx.busy} onClick={() => void decide([a.id], true)}>Approve once</button>
            <button className="ghost" disabled={!!ctx.busy} onClick={() => void decide([a.id], false)}>Reject</button>
          </div>
        </article>
      ))}
      {reversible.length > 1 && <button disabled={!!ctx.busy} onClick={() => void decide(reversible.map((a) => a.id), true)}>Approve all {reversible.length} that can be undone</button>}
    </section>
  );
}
