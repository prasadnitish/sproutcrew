import Composer from "./Composer";
import Gate from "./Gate";
import { useEffect, useState } from "react";
import { api, Proposal, Turn } from "./api";
import Markdown from "./Document";
import { Chip, Ctx, provider, roleName } from "./ui";

const WORD: Record<Proposal["status"], string> = {
  pending: "Waiting for you",
  blocked: "Cannot be applied",
  applied: "Applied",
  dismissed: "Not applied",
  superseded: "Replaced by a newer one",
};

function Card({ ctx, p, reload }: { ctx: Ctx; p: Proposal; reload: () => Promise<void> }) {
  const [gate, setGate] = useState(false);
  const live = p.status === "pending" || p.status === "blocked";
  const changesPlan = p.changes.some((c) => c.op !== "add_note");
  const stepName = (id: string) => ctx.s.plan?.stages.find((x) => x.id === id)?.title ?? id;
  const apply = () => ctx.guard(async () => { await api.call("coordinator_apply", { project: ctx.s.name, id: p.id }); await reload(); });
  const runAgain = () =>
    ctx.guard(async () => {
      for (const id of p.rerun) {
        await api.run("run_stage", { engine: ctx.engine, stageId: id }, ctx.track);
        await ctx.refresh();
      }
      await reload();
    });
  return (
    <div className={`proposal ${p.status}`}>
      <div className="proposal-head">
        <strong>{p.changes.length ? "Proposed change" : "Suggested next step"}</strong>
        <Chip tone={p.status === "applied" ? "ok" : p.status === "blocked" ? "bad" : p.status === "pending" ? "info" : "quiet"}>{WORD[p.status]}</Chip>
      </div>
      <ul className="plain">{p.summary.map((l, i) => <li key={i}>{l}</li>)}</ul>
      {gate && (
        <Gate title="Apply this plan change?" approveLabel={ctx.s.gate === "allowed" ? "Apply and approve" : "Apply changes"} rejectLabel="Not yet" busy={!!ctx.busy}
          what={ctx.s.gate === "allowed"
            ? <p>Applying saves the new plan and approves it as you, so the steps below can run. Anything it leaves out of date is listed first.</p>
            : <p>Applying saves the new steps. The plan is not approved yet, so nothing runs: you approve the whole new plan afterwards.</p>}
          payload={[...p.summary, ...(p.stale.length ? [`Out of date after this: ${p.stale.join(", ")}`] : [])].join("\n")}
          onReject={() => setGate(false)} onApprove={() => { setGate(false); void apply(); }} />
      )}
      {p.problems.length > 0 && <ul className="field-error">{p.problems.map((x, i) => <li key={i}>{x}</li>)}</ul>}
      {live && p.stale.length > 0 && (
        <p className="muted">These steps have results this would leave out of date: {p.stale.map(stepName).join(", ")}.</p>
      )}
      {live && changesPlan && p.problems.length === 0 && (
        <p className="muted">{ctx.s.gate === "allowed" ? "Applying this saves the new plan and approves it as you." : "Applying this saves the new steps. You approve the plan afterwards."}</p>
      )}
      <div className="actions tight">
        {live && p.changes.length > 0 && (
          <button className="primary" disabled={!!ctx.busy || p.problems.length > 0} onClick={() => (changesPlan ? setGate(true) : apply())}>
            {changesPlan ? "Review the plan change" : "Add the note"}
          </button>
        )}
        {live && <button className="ghost" disabled={!!ctx.busy} onClick={() => ctx.guard(async () => { await api.call("coordinator_dismiss", { id: p.id }); await reload(); })}>Not now</button>}
        {(p.status === "applied" || (live && p.changes.length === 0)) && p.rerun.length > 0 && ctx.s.gate === "allowed" && (
          <button disabled={!!ctx.busy} onClick={runAgain}>
            {p.rerun.length === 1 ? `Run “${stepName(p.rerun[0])}” again` : `Run ${p.rerun.length} steps again`}
          </button>
        )}
      </div>
    </div>
  );
}

/** Asking for a change to the plan and the notes, as a conversation. It cannot change a result. */
export default function Coordinator({ ctx, onClose }: { ctx: Ctx; onClose: () => void }) {
  const [turns, setTurns] = useState<Turn[] | null>(null);
  const [msg, setMsg] = useState("");
  const [role, setRole] = useState("");
  const team = ctx.s.plan?.team ?? [];
  const load = () => api.call<Turn[]>("coordinator_chat").then(setTurns).catch(() => setTurns([]));
  useEffect(() => { void load(); }, []);
  const send = () =>
    ctx.guard(async () => {
      await api.run("coordinator_say", { engine: ctx.engine, role, message: msg }, ctx.track);
      setMsg("");
      await load();
    });
  return (
    <section id="coordinator-panel" className="sheet coordinator" aria-label="Ask for a change">
      <div className="sheet-head">
        <div>
          <h2>Ask for a change</h2>
          <p className="muted">
            Ask for changes to the plan or give an instruction. It can add, remove, move, or rewrite steps, change who does what, and add notes to steps. It never edits a result: it adds a note and suggests running the step again. Nothing changes until you apply it.
          </p>
        </div>
        <button className="ghost" onClick={onClose}>Close</button>
      </div>
      {turns && turns.length > 0 && (
        <div className="thread" aria-live="polite">
          {turns.map((t, i) =>
            t.role === "user" ? (
              <div className="bubble you" key={i}>{t.text}</div>
            ) : (
              <div className="bubble answer" key={i}>
                <Markdown text={t.text} />
                {t.proposal && <Card ctx={ctx} p={t.proposal} reload={load} />}
                <p className="muted who-line">{t.by}</p>
              </div>
            ),
          )}
        </div>
      )}
      <Composer label="Your message" value={msg} onChange={setMsg} onSend={send} sendLabel="Send" placeholder="Use 14 January as the board date, then redo the document and the deck."
        blocked={ctx.askBusy ? "Wait for the current run to finish." : ""}
        menu={<select className="quiet-menu" aria-label="Who answers" value={role} onChange={(e) => setRole(e.target.value)}>
          <option value="">Default (the first role)</option>
          {team.map((t) => <option key={t.role} value={t.role}>{roleName(t.role)}, {provider(t.route)} {t.model}</option>)}
        </select>} />
    </section>
  );
}
