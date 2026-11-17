import { useState } from "react";
import { api, Plan } from "./api";
import Diagram from "./WorkflowDiagram";
import { NewStep, needsOf, reshape, stepId } from "./graph";
import { Ctx, Monogram, roleName, text } from "./ui";

const ADAPT = "Look at my ask in the brief. Change the steps to fit it: leave out steps the ask does not need, add steps it asks for (for example a small prototype), and leave the others alone. Give every step you add full instructions.";

/** Roles a step can use even when the use case's team has none: a step that builds a prototype needs a builder, which
 *  writes code files. Choosing one adds it to the team on the same provider and model as the first writer. */
const EXTRA_ROLES: [string, string][] = [["builder", "Builder (writes the prototype as files)"]];

const sideOf = (ctx: Ctx, role: string): "writing" | "checking" => ctx.s.role_info?.[role]?.side ?? (role === "reviewer" ? "checking" : "writing");

/** Shape the flow before anything runs: tick the steps to keep, add the ones that are missing, see the flow as a
 *  picture, and save. The coordinator can propose the changes from the ask. Saving changes the plan, so it is
 *  approved afterwards from the whole new plan. */
export default function Shape({ ctx, plan, onTalk }: { ctx: Ctx; plan: Plan; onTalk: () => void }) {
  const [off, setOff] = useState<string[]>([]);
  const [added, setAdded] = useState<NewStep[]>([]);
  const [title, setTitle] = useState("");
  const [role, setRole] = useState(plan.team[0]?.role ?? "");
  const [after, setAfter] = useState("");
  const [guide, setGuide] = useState("");
  const [errors, setErrors] = useState<string[]>([]);
  const shaped = reshape(plan.stages, off, added.filter((a) => !off.includes(a.id)));
  const dirty = off.length > 0 || added.length > 0;
  const roles = [...new Set(plan.team.map((t) => t.role))];
  const choices: [string, string][] = [...roles.map((r) => [r, roleName(r)] as [string, string]), ...EXTRA_ROLES.filter(([r]) => !roles.includes(r))];
  const ids = shaped.map((s) => s.id);
  const taken = [...plan.stages.map((s) => s.id), ...added.map((a) => a.id)];
  const why = !title.trim() ? "Name the step." : !guide.trim() ? "Say what it should do." : !role ? "Choose who does it." : "";
  const add = () => {
    const id = stepId(title, taken);
    setAdded([...added, { id, title: title.trim(), owner_role: role, guide: guide.trim(), after: after || shaped[shaped.length - 1]?.id || "" }]);
    setTitle(""); setGuide(""); setAfter("");
  };
  const save = () =>
    ctx.guard(async () => {
      try {
        // A role the team does not have yet joins it on the first writer's provider and model.
        const writer = plan.team.find((t) => sideOf(ctx, t.role) === "writing") ?? plan.team[0];
        const missing = [...new Set(shaped.map((st) => st.owner_role))].filter((r) => !plan.team.some((t) => t.role === r));
        const team = writer ? [...plan.team, ...missing.map((r) => ({ ...writer, role: r }))] : plan.team;
        await api.call("save_plan", { plan: { ...plan, stages: shaped, team } });
        setOff([]); setAdded([]); setErrors([]);
      } catch (e) { setErrors(Array.isArray(e) ? e.map(text) : [text(e)]); throw "The steps were not saved."; }
    });
  const adapt = () => ctx.guard(async () => { await api.run("coordinator_say", { engine: ctx.engine, role: "", message: ADAPT }, ctx.track); onTalk(); });
  return (
    <section className="sheet shape" aria-label="Shape the steps">
      <header className="sheet-head">
        <div>
          <h2>Shape the steps</h2>
          <p className="muted">Tick the steps you want, add the ones that are missing, and look at the flow. Nothing runs until you approve the plan.</p>
        </div>
        <button className="ghost" disabled={!!ctx.busy} onClick={adapt} title="The coordinator reads your ask and proposes changes you can apply">Adapt to my ask</button>
      </header>
      <ul className="shape-list">
        {plan.stages.map((st, i) => {
          const needs = needsOf(plan.stages, i).map((n) => plan.stages.find((x) => x.id === n)?.title ?? n);
          return (
            <li key={st.id} className={off.includes(st.id) ? "off" : ""}>
              <label className="shape-row">
                <input type="checkbox" checked={!off.includes(st.id)} onChange={(e) => setOff(e.target.checked ? off.filter((x) => x !== st.id) : [...off, st.id])} />
                <Monogram side={sideOf(ctx, st.owner_role)} name={roleName(st.owner_role)} />
                <span className="shape-text"><strong>{st.title}</strong><span className="meta">{roleName(st.owner_role)}{needs.length ? ` · after ${needs.join(", ")}` : " · starts the flow"}</span></span>
              </label>
            </li>
          );
        })}
        {added.map((a) => (
          <li key={a.id}>
            <div className="shape-row">
              <span className="chip info">New</span>
              <Monogram side={sideOf(ctx, a.owner_role)} name={roleName(a.owner_role)} />
              <span className="shape-text"><strong>{a.title}</strong><span className="meta">{roleName(a.owner_role)} · after {[...plan.stages, ...added].find((x) => x.id === a.after)?.title ?? "the last step"}</span></span>
              <button className="quiet" onClick={() => setAdded(added.filter((x) => x.id !== a.id))}>Remove step</button>
            </div>
          </li>
        ))}
      </ul>
      <details className="drawer">
        <summary>Add a step</summary>
        <div className="shape-form">
          <label>Name <input value={title} maxLength={80} onChange={(e) => setTitle(e.target.value)} placeholder="Build a small prototype" /></label>
          <label>Who does it
            <select value={role} onChange={(e) => setRole(e.target.value)}>{choices.map(([r, label]) => <option key={r} value={r}>{label}</option>)}</select>
          </label>
          <label>Runs after
            <select value={after} onChange={(e) => setAfter(e.target.value)}>
              <option value="">The last step</option>
              {ids.map((id) => <option key={id} value={id}>{shaped.find((s) => s.id === id)?.title}</option>)}
            </select>
          </label>
          <label>What it should do <textarea rows={3} value={guide} maxLength={1200} spellCheck={false} onChange={(e) => setGuide(e.target.value)} placeholder="Build a small clickable prototype of what the recommendation proposes, as files in this folder." /></label>
          <div className="actions tight">
            <button className="primary" disabled={!!why} onClick={add}>Add step</button>
            <span className="meta">{why}</span>
          </div>
        </div>
      </details>
      <figure className="shape-flow" aria-label="How the steps will flow">
        <figcaption className="eyebrow">How it will flow</figcaption>
        <Diagram steps={shaped.map((st, i) => ({ id: st.id, title: st.title, side: sideOf(ctx, st.owner_role), needs: needsOf(shaped, i) }))}
          status={() => "waiting"} who={(id) => roleName(shaped.find((s) => s.id === id)?.owner_role ?? "")} focus="" onPick={() => undefined} />
      </figure>
      <div className="actions">
        <button className="primary" disabled={!dirty || !!ctx.busy || shaped.length === 0} onClick={save}>Save these steps</button>
        <button className="ghost" disabled={!dirty} onClick={() => { setOff([]); setAdded([]); setErrors([]); }}>Undo my changes</button>
        <span className="meta">{dirty ? `${shaped.length} of the steps will run. Saving needs a new approval.` : `${plan.stages.length} steps will run.`}</span>
      </div>
      {errors.length > 0 && <ul className="field-error" role="alert">{errors.map((e) => <li key={e}>{e}</li>)}</ul>}
    </section>
  );
}
