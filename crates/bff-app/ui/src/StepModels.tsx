import { useState } from "react";
import { api, Plan, Stage, StepModel } from "./api";
import { modelOf } from "./graph";
import { secondCheckOf } from "./secondcheck";
import { Choice, ProviderPick } from "./Pick";
import { Ctx, effort, Monogram, provider, roleName, text } from "./ui";

/** One row per step with the provider and model it runs on. A step starts on its role's model, and choosing another
 *  gives it its own. Saving changes the plan, so it is approved again before anything runs. */
export default function StepModels({ ctx, plan }: { ctx: Ctx; plan: Plan }) {
  const [edits, setEdits] = useState<Record<string, StepModel | null>>({});
  const [open, setOpen] = useState("");
  const [errors, setErrors] = useState<string[]>([]);
  const stages: Stage[] = plan.stages.map((st) => {
    if (!(st.id in edits)) return st;
    const { uses: _, ...rest } = st;
    const own = edits[st.id];
    return own ? { ...rest, uses: own } : rest;
  });
  const checking = (role: string) => (ctx.s.role_info?.[role]?.side ?? (role === "reviewer" ? "checking" : "writing")) === "checking";
  const pick = (st: Stage, [route, model, eff]: Choice) => {
    const role = plan.team.find((t) => t.role === st.owner_role);
    const same = role && role.route === route && role.model === model && role.effort === eff;
    setEdits({ ...edits, [st.id]: same ? null : { route, model, effort: eff } });
  };
  const save = () =>
    ctx.guard(async () => {
      try {
        await api.call("save_plan", { plan: { ...plan, stages } });
        setEdits({}); setOpen(""); setErrors([]);
      } catch (e) { setErrors(Array.isArray(e) ? e.map(text) : [text(e)]); throw "The models were not saved."; }
    });
  return (
    <section className="sheet" aria-label="Model for each step">
      <header className="sheet-head">
        <div>
          <h2>Model for each step</h2>
          <p className="muted">Each step uses its role's model until you choose another. A check works best on a different provider from the work it reads, because it is less likely to repeat the same mistake.</p>
        </div>
      </header>
      <ul className="crew-list">
        {stages.map((st) => {
          const m = modelOf(plan.team, st);
          const side = checking(st.owner_role) ? "checking" : "writing";
          return [
            <li key={st.id} className="crew-row">
              <Monogram side={side} name={roleName(st.owner_role)} />
              <span className="crew-main">
                <strong>{st.title}</strong>
                <span className="muted">{roleName(st.owner_role)}{m ? ` · ${provider(m.route)} ${m.model}, ${effort(m.effort).toLowerCase()} effort` : ""}{st.uses ? " · chosen for this step" : ""}</span>
                {checking(st.owner_role) && <span className="muted">{secondCheckOf(plan.team, stages, checking, st)}</span>}
              </span>
              <button className="ghost" aria-expanded={open === st.id} onClick={() => setOpen(open === st.id ? "" : st.id)}>Change</button>
            </li>,
            open === st.id && m && (
              <li key={`${st.id}-pick`} className="drawer">
                <ProviderPick label="" value={[m.route, m.model, m.effort]} onChange={(c) => pick(st, c)} />
                {st.uses && <button className="ghost" onClick={() => setEdits({ ...edits, [st.id]: null })}>Use the {roleName(st.owner_role)} default</button>}
              </li>
            ),
          ];
        })}
      </ul>
      <div className="stand-ins">
        <h3>If a provider fails</h3>
        {(plan.fallbacks?.length ?? 0) > 0 ? (
          <>
            <p className="muted">If a writing step times out, hits a rate limit, or its provider is unavailable, it tries once more on the other provider. Checks stay where they are, so they remain a second opinion.</p>
            <ul className="plain">{plan.fallbacks!.map((f) => <li key={f.role} className="muted">{roleName(f.role)} moves to {provider(f.route)} {f.model}</li>)}</ul>
          </>
        ) : <p className="muted">A step that fails stops and waits for you. Turn this on and a writing step tries once more on the other provider.</p>}
        <button className="ghost" disabled={!!ctx.busy} onClick={() => ctx.guard(() => api.call("set_stand_ins", { on: !plan.fallbacks?.length }))}>{plan.fallbacks?.length ? "Turn this off" : "Turn this on"}</button>
      </div>
      <div className="actions">
        <button className="primary" disabled={JSON.stringify(stages) === JSON.stringify(plan.stages) || !!ctx.busy} onClick={save}>Save the models</button>
      </div>
      <p className="muted">A change needs a new approval. Results already made stay as they are, so run a step again to use its new model.</p>
      {errors.length > 0 && <ul className="field-error" role="alert">{errors.map((e) => <li key={e}>{e}</li>)}</ul>}
    </section>
  );
}
