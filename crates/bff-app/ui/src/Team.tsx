import { useEffect, useState } from "react";
import { Added, api, Assignment, Plan, Stage, UseCase } from "./api";
import { Connectors, ToolsPanel } from "./Extras";
import { AddedNote, AddMaterial, InjectionNotice, SourceList } from "./Files";
import { Choice, DEFAULT_WRITING, ProviderPick } from "./Pick";
import StepModels from "./StepModels";
import TemplatePanel from "./Template";
import { Ctx, Dialog, effort, provider, roleName, Section, text, Monogram, useConfirm } from "./ui";

/** User-controlled memory shared by this project's agents through the existing durable notes store. */
function SharedMemory({ ctx }: { ctx: Ctx }) {
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState("");
  const [error, setError] = useState("");
  const memories = ctx.s.notes.filter((n) => !n.stage);
  const [ask, asking] = useConfirm();
  const save = () => ctx.guard(async () => {
    try {
      await api.call("add_note", { stage: "", text: draft, replaceId: editing || null });
      setDraft(""); setEditing(""); setError("");
    } catch (e) { setError(text(e)); throw e; }
  });
  return <section className="sheet" aria-label="Shared project memory">
    <header className="sheet-head"><div><h2>Shared project memory</h2><p className="muted">Keep your audience, voice, decisions, and constraints here. Every agent in this project receives these notes on its next run, including after you quit and reopen. They are your instructions, not verified facts. They stay within this project; copying results to another project does not copy this memory.</p></div></header>
    {memories.length === 0 && <p className="empty-note">No shared memory yet. Add what your whole crew should keep in mind.</p>}
    {asking}
    <ul className="notes">{memories.map((n) => <li key={n.id}>
      <span className="note-text">{n.text}</span>
      <button className="ghost" disabled={!!ctx.busy} onClick={() => { setDraft(n.text); setEditing(n.id); }}>Edit</button>
      <button className="ghost" disabled={!!ctx.busy} aria-label={`Remove memory: ${n.text.slice(0,40)}`} onClick={() => ask({ title: "Remove this note?", line: "Future steps in this project stop getting it. Earlier results keep what they were given.", action: "Remove", run: () => void ctx.guard(async () => { await api.call("remove_note", { id: n.id }); if (editing === n.id) { setDraft(""); setEditing(""); } }) })}>Remove</button>
    </li>)}</ul>
    <label>{editing ? "Edit shared memory" : "What should the crew remember?"}<textarea rows={3} maxLength={2000} value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="Our audience is small-business owners. Use plain English. Distinguish measured results from estimates." /></label>
    <div className="actions"><button disabled={!!ctx.busy || !draft.trim()} onClick={save}>{editing ? "Save changes" : "Save memory"}</button>{editing && <button className="ghost" onClick={() => { setDraft(""); setEditing(""); }}>Cancel edit</button>}</div>
    {error && <p className="field-error" role="alert">{error}</p>}
    <p className="muted">Do not add passwords or API keys. Removing a note stops it appearing in future requests; it does not erase prior results, request records, or the decision log.</p>
  </section>;
}

/** The rules for this exercise. They come before every step description, note, template, and file. */
function RulesBox({ ctx }: { ctx: Ctx }) {
  const saved = ctx.s.rules?.text ?? "";
  const [draft, setDraft] = useState(saved);
  const [err, setErr] = useState("");
  useEffect(() => setDraft(saved), [saved, ctx.s.name]);
  const dirty = draft.trim() !== saved;
  return (
    <section className="sheet">
      <header className="sheet-head">
        <div>
          <h2>Rules for this exercise</h2>
          <p className="muted">What applies to this exercise whatever else says otherwise, such as "AI tools are allowed", "at most 1,000 words", or "I must record my screen". Every step, every request for a change, and second opinions are told these first, and they win over step descriptions, notes, templates, and files. Changes apply to the next run.</p>
        </div>
      </header>
      <label>Your rules
        <textarea rows={4} value={draft} maxLength={6000} onChange={(e) => setDraft(e.target.value)} placeholder="AI tools are allowed for this exercise. The answer is at most 1,000 words." />
      </label>
      <div className="actions">
        <button disabled={!!ctx.busy || !dirty} onClick={() => ctx.guard(async () => { try { await api.call("set_exercise_rules", { text: draft }); setErr(""); } catch (e) { setErr(text(e)); throw e; } })}>{draft.trim() ? "Save the rules" : "Clear the rules"}</button>
        {dirty && <button className="ghost" onClick={() => setDraft(saved)}>Undo my edits</button>}
      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
    </section>
  );
}

/** Sources (DESIGN_SPEC.md §10.1): what every step reads, its rules and notes, and its template, as sections. Adding a
 *  source is one dialog. */
export function MaterialTab({ ctx }: { ctx: Ctx }) {
  const [note, setNote] = useState<Added | null>(null);
  const [err, setErr] = useState("");
  const [adding, setAdding] = useState(false);
  const used = ctx.s.artifacts.length > 0;
  const n = ctx.s.sources.length;
  const flagged = ctx.s.sources.filter((x) => x.suspicious.length > 0).length;
  const merge = (rs: Added[]) => setNote({ added: rs.flatMap((r) => r.added), skipped: rs.flatMap((r) => r.skipped) });
  const add = (f: () => Promise<Added[]>) => ctx.guard(async () => { try { merge(await f()); setErr(""); } catch (e) { setErr(text(e)); throw e; } });
  const rules = ctx.s.rules?.text?.trim();
  const notes = ctx.s.notes.filter((x) => !x.stage).length;
  return (
    <>
      <div className="tab-line"><p className="muted">What every step reads and cites, the rules it follows, and the template its files use.</p><button onClick={() => setAdding(true)}>Add a source</button></div>
      {note && <AddedNote r={note} />}
      <div className="page-sections"><div>
        <Section title="Your sources" value={`${n} ${n === 1 ? "source" : "sources"}${flagged ? `, ${flagged} flagged` : ""}`} flag={flagged ? `${flagged} flagged` : undefined} open={n === 0}>
          <InjectionNotice sources={ctx.s.sources} />
          <p className="muted">Mark task instructions or a template; other files are evidence to read. Changes apply to the next run.{used ? " A source a step has already read cannot be removed." : ""}</p>
          <SourceList sources={ctx.s.sources} locked={used} busy={!!ctx.busy} onRemove={(id) => ctx.guard(() => api.call("remove_source", { id }))} onRole={(id, role) => ctx.guard(() => api.call("set_source_role", { id, role }))} />
        </Section>
        <Section title="Instructions" value={`${rules ? "rules set" : "no rules"}, ${notes} ${notes === 1 ? "note" : "notes"}`}>
          <RulesBox ctx={ctx} />
          <SharedMemory ctx={ctx} />
          <p className="muted">Notes for every project are in Memory.</p>
        </Section>
        <Section title="Template" value={ctx.s.look?.name ?? "none"}>
          <TemplatePanel ctx={ctx} />
        </Section>
      </div></div>
      {adding && <Dialog title="Add a source" onClose={() => setAdding(false)}>
        <AddMaterial disabled={!!ctx.busy} current={ctx.s.name}
          onUrls={(us) => add(async () => { const out: Added[] = []; for (const u of us) out.push(await api.call<Added>("add_source_url", { url: u })); return out; })}
          onPaths={(ps) => add(async () => { const out: Added[] = []; for (const p of ps) out.push(await api.call<Added>("add_source_path", { path: p })); return out; })}
          onFiles={(fs) => add(async () => { const out: Added[] = []; for (const f of fs) out.push(await api.addFile(f)); return out; })}
          onPick={(from, stages) => add(async () => [await api.call<Added>("add_source_results", { from, stages })])}
          service={<Connectors ctx={ctx} />} />
        {err && <p className="field-error" role="alert">{err}</p>}
        <div className="dlg-foot"><span className="sp" /><button className="primary" data-autofocus onClick={() => setAdding(false)}>Done</button></div>
      </Dialog>}
    </>
  );
}


/** The crew: who is on the team, what each does, and which provider and model each uses. */
function Crew({ ctx }: { ctx: Ctx }) {
  const p = ctx.s.plan;
  if (!p) return <p className="empty-note">There is no plan yet. Suggest one, or use a ready-made one.</p>;
  return (
    <ul className="crew-list">
      {p.team.map((t) => {
        const info = ctx.s.role_info?.[t.role];
        const side = info?.side ?? (t.role === "reviewer" ? "checking" : "writing");
        const steps = p.stages.filter((s) => s.owner_role === t.role).length;
        return (
          <li key={t.role} className="crew-row">
            <Monogram side={side} name={roleName(t.role)} />
            <span className="crew-main">
              <strong>{roleName(t.role)} <span className={`side ${side}`}>{side === "writing" ? "Writes" : "Checks"}</span></strong>
              <span className="muted">{info?.does ?? "A role you added."}</span>
            </span>
            <div className="crew-meta"><span>{steps} {steps === 1 ? "step" : "steps"}</span><details><summary>AI details</summary><p>{provider(t.route)} · {t.model}<br />{effort(t.effort)} effort</p></details></div>
          </li>
        );
      })}
    </ul>
  );
}

function ReadyMade({ ctx }: { ctx: Ctx }) {
  const [cases, setCases] = useState<UseCase[]>([]);
  const [kind, setKind] = useState("case_study");
  const [author, setAuthor] = useState<Choice>(["R12", DEFAULT_WRITING, "medium"]);
  const [err, setErr] = useState("");
  useEffect(() => { api.call<UseCase[]>("use_cases").then(setCases).catch(() => setCases([])); }, []);
  return (
    <div>
      <p className="muted">Replace the plan with one of the ready-made ones. A different connected API provider is preferred for checking; otherwise a second pass uses the same provider.</p>
      <label>Use case
        <select value={kind} onChange={(e) => setKind(e.target.value)}>{cases.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}</select>
      </label>
      <ProviderPick label="Writing side" value={author} onChange={setAuthor} />
      <div className="actions">
        <button disabled={!!ctx.busy || !author[1]} onClick={() => ctx.guard(async () => {
          try {
            const r = await api.call<string[]>("default_reviewer", { route: author[0] });
            await api.call("use_workflow", { name: kind, author, reviewer: [r[0], r[1], r[2]] });
            setErr("");
          } catch (e) { setErr(text(e)); throw e; }
        })}>Use this plan</button>
      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
    </div>
  );
}

function AssignmentRow({ a, onChange, onRemove }: { a: Assignment; onChange: (a: Assignment) => void; onRemove: () => void }) {
  return (
    <div className="team-row">
      <label>Role<input value={a.role} onChange={(e) => onChange({ ...a, role: e.target.value })} /></label>
      <ProviderPick label="" value={[a.route, a.model, a.effort]} onChange={([route, model, eff]) => onChange({ ...a, route, model, effort: eff })} />
      <button className="ghost" onClick={onRemove} aria-label={`Remove ${a.role}`}>Remove</button>
    </div>
  );
}

function PlanEditor({ ctx, draft, setDraft }: { ctx: Ctx; draft: Plan; setDraft: (p: Plan | null) => void }) {
  const [errors, setErrors] = useState<string[]>([]);
  const stages = draft.stages;
  const edit = (i: number, patch: Partial<Stage>) => setDraft({ ...draft, stages: stages.map((x, j) => (j === i ? { ...x, ...patch } : x)) });
  return (
    <div>
      <h3>Steps</h3>
      {stages.map((st, i) => (
        <div className="grid-row stage" key={i}>
          <input aria-label="Step name" value={st.id} onChange={(e) => edit(i, { id: e.target.value })} />
          <input aria-label="Step title" value={st.title} onChange={(e) => edit(i, { title: e.target.value })} />
          <select aria-label="Who does it" value={st.owner_role} onChange={(e) => edit(i, { owner_role: e.target.value })}>
            {[...new Set([st.owner_role, ...draft.team.map((t) => t.role)])].map((r) => <option key={r} value={r}>{roleName(r)}</option>)}
          </select>
          <button className="ghost" onClick={() => setDraft({ ...draft, stages: stages.filter((_, j) => j !== i) })} aria-label={`Remove step ${st.id}`}>Remove</button>
          <label className="guide">Instructions for this step
            <textarea rows={2} value={st.guide ?? ""} onChange={(e) => edit(i, { guide: e.target.value })} />
          </label>
        </div>
      ))}
      <button className="ghost" onClick={() => setDraft({ ...draft, stages: [...stages, { id: `step${stages.length + 1}`, title: "New step", owner_role: draft.team[0]?.role ?? "" }] })}>Add a step</button>
      <h3>Crew roles</h3>
      {draft.team.map((a, i) => (
        <AssignmentRow key={i} a={a} onChange={(x) => setDraft({ ...draft, team: draft.team.map((y, j) => (j === i ? x : y)) })} onRemove={() => setDraft({ ...draft, team: draft.team.filter((_, j) => j !== i) })} />
      ))}
      <button className="ghost" onClick={() => setDraft({ ...draft, team: [...draft.team, { role: `role${draft.team.length + 1}`, route: "R1", model: "gpt-6-luna", effort: "medium" }] })}>Add a role</button>
      <div className="actions">
        <button className="primary" disabled={!ctx.dirty || !!ctx.busy} onClick={() => ctx.guard(async () => {
          try { await api.call("save_plan", { plan: draft }); setErrors([]); } catch (e) { setErrors(Array.isArray(e) ? e.map(text) : [text(e)]); throw "The plan was not saved."; }
        })}>Save the plan</button>
        {ctx.dirty && <span className="muted">Unsaved edits. A saved edit needs a new approval.</span>}
      </div>
      {errors.length > 0 && <ul className="field-error" role="alert">{errors.map((e) => <li key={e}>{e}</li>)}</ul>}
    </div>
  );
}

/** Your crew (DESIGN_SPEC.md §10.1): the brief, the crew, steps and models, and tools, as sections. Changing the plan
 *  and using a ready-made one are dialogs; the plan dialog holds the tab's only Save. */
export default function TeamTab({ ctx, draft, setDraft }: { ctx: Ctx; draft: Plan | null; setDraft: (p: Plan | null) => void }) {
  const [open, setOpen] = useState<"" | "ready" | "edit">("");
  const p = ctx.s.plan;
  const firstWords = ctx.s.brief.split(/\s+/).slice(0, 8).join(" ");
  return (
    <>
      <div className="tab-line">
        <p className="muted">Who prepares and checks the work, on which models. A new or changed plan is approved again before anything runs.</p>
        <span className="actions tight">
          {draft && <button onClick={() => setOpen("edit")}>Change the plan</button>}
          <button onClick={() => setOpen("ready")}>Use a ready-made plan</button>
          <button disabled={!!ctx.busy} onClick={() => ctx.guard(() => api.run("propose_plan", { engine: ctx.engine }, ctx.track))}>{p ? "Suggest a new plan" : "Suggest a plan"}</button>
        </span>
      </div>
      <div className="page-sections"><div>
        <Section title="Brief" value={`${firstWords}${ctx.s.brief.split(/\s+/).length > 8 ? "…" : ""}`}>
          <p className="brief ask-text">{ctx.s.brief}</p>
        </Section>
        <Section title="Crew" value={p ? `${p.team.length} ${p.team.length === 1 ? "role" : "roles"}` : "no plan yet"} open>
          <Crew ctx={ctx} />
          <p className="muted">Writers prepare the work. The second check gives it another pass; a different provider gives a more independent opinion.</p>
        </Section>
        {p && <Section title="Steps and models" value={`${p.stages.length} ${p.stages.length === 1 ? "step" : "steps"}`}>
          <StepModels ctx={ctx} plan={p} />
        </Section>}
        <Section title="Tools" value="skills and connections">
          <ToolsPanel ctx={ctx} />
        </Section>
      </div></div>
      {open === "ready" && <Dialog title="Use a ready-made plan" onClose={() => setOpen("")}><ReadyMade ctx={ctx} /><div className="dlg-foot"><span className="sp" /><button data-autofocus onClick={() => setOpen("")}>Close</button></div></Dialog>}
      {open === "edit" && draft && <Dialog title="Change the plan" onClose={() => setOpen("")}><PlanEditor ctx={ctx} draft={draft} setDraft={setDraft} /><p className="muted">Saving ends the plan's approval, so you approve it again before more steps run.</p><div className="dlg-foot"><span className="sp" /><button data-autofocus onClick={() => setOpen("")}>Close</button></div></Dialog>}
    </>
  );
}
