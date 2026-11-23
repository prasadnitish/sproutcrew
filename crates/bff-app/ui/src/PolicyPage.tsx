import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import { Defaults, defaultsLine, FIELDS, fitLine, groupOf, looserThan, Overview, phrase, PolicyRow } from "./policyFit";
import { Dialog, Section, text } from "./ui";

const upper = (x: string) => x.charAt(0).toUpperCase() + x.slice(1);

const CHOICES: Record<string, [string, string][]> = {
  send_outside: [["ask", "Ask me each time"], ["never", "Never"]],
  delete_files: [["ask", "Ask me each time"], ["never", "Never"]],
  install_software: [["ask", "Ask me each time"], ["never", "Never"]],
  privacy: [["standard", "Their own policy"], ["no_retention", "Nothing"]],
};

/** Policy (DESIGN_SPEC.md §10): the defaults every new project starts with, and how each project compares. Saving
 *  defaults changes no project unless it is ticked, and a ticked one is only tightened. */
export default function PolicyPage({ running, onOpen, onSaved }: { running: string | null; onOpen: (name: string) => void; onSaved: () => void }) {
  const [v, setV] = useState<Overview | null>(null);
  const [err, setErr] = useState("");
  const [draft, setDraft] = useState<Defaults | null>(null);
  const [saving, setSaving] = useState(false);
  const [said, setSaid] = useState("");
  const load = useCallback(async () => {
    try { const x = await api.call<Overview>("policy_overview"); setV(x); setDraft(x.defaults); setErr(""); }
    catch (e) { setErr(text(e)); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  if (!v || !draft) return <div className="page-body">{err ? <p className="notice error" role="alert">{err}</p> : <p className="muted" role="status">Reading your projects' policies…</p>}</div>;
  const changed = FIELDS.some((f) => draft[f.id] !== v.defaults[f.id]);
  const groups = { looser: [] as PolicyRow[], stricter: [] as PolicyRow[], same: [] as PolicyRow[] };
  for (const r of v.projects.filter((x) => x.plan)) groups[groupOf(r)].push(r);
  const line = (r: PolicyRow) => (
    <Section key={r.name} title={r.name} value={fitLine(r)}>
      <ul className="plain">
        {(["send_outside", "delete_files", "install_software"] as const).map((k) => <li key={k}>{upper(phrase(k, r.guardrails![k]))}</li>)}
        <li>{upper(phrase("privacy", r.privacy ?? "standard"))}</li>
        <li>Never runs commands. Checking roles only read.</li>
      </ul>
      <p className="muted">{r.approved ? <>Held by approval <span className="mono">{r.hash}</span>. Changing its policy ends that approval.</> : "Not approved yet, so changing its policy ends nothing."}</p>
      <button onClick={() => onOpen(r.name)}>Open {r.name} to change it</button>
    </Section>
  );
  return (
    <div className="page-body">
      {err && <p className="notice error" role="alert">{err}</p>}
      {v.errors.length > 0 && <p className="note" role="status">Not compared, because {v.errors.length === 1 ? "it" : "they"} could not be read: {v.errors.map((x) => x.project).join(", ")}.</p>}
      <p className="vh" role="status" aria-live="polite">{said}</p>
      {said && <p className="note">{said}</p>}
      <div className="page-sections">
        <div>
          <Section title="Your defaults" value={defaultsLine(v.defaults)} open={changed}>
            <p className="page-intro">Every new project starts with these. A project made before you saved them keeps its own policy, and this page says which differ.</p>
            <div className="defs">
              {FIELDS.map((f) => (
                <fieldset key={f.id}>
                  <legend>{f.label}</legend>
                  {CHOICES[f.id].map(([value, word]) => (
                    <label key={value}><input type="radio" name={`d-${f.id}`} checked={draft[f.id] === value} onChange={() => setDraft({ ...draft, [f.id]: value })} /> {word}</label>
                  ))}
                </fieldset>
              ))}
              <fieldset><legend>Run commands</legend><span className="muted">Never. Locked in this version.</span></fieldset>
              <fieldset><legend>Second check</legend><span className="muted">Reads only. Locked.</span></fieldset>
            </div>
            {draft.privacy === "no_retention" && <p className="muted">Keeping nothing needs every step on OpenRouter, so a plan takes it on its own plan card. This page shows the projects that differ.</p>}
            <div className="actions tight">
              <button className="primary" disabled={!changed} onClick={() => setSaving(true)}>Save defaults</button>
              {changed && <button onClick={() => setDraft(v.defaults)}>Undo</button>}
            </div>
          </Section>
        </div>
        <div>
          {groups.looser.length > 0 && <Section title="Looser than your defaults" flag="Check" value={`${groups.looser.length} ${groups.looser.length === 1 ? "project" : "projects"}`} open>{groups.looser.map(line)}</Section>}
          {groups.stricter.length > 0 && <Section title="Stricter than your defaults" value={`${groups.stricter.length} ${groups.stricter.length === 1 ? "project" : "projects"}`}>{groups.stricter.map(line)}</Section>}
          {groups.same.length > 0 && <Section title="Same as your defaults" value={`${groups.same.length} ${groups.same.length === 1 ? "project" : "projects"}`}>{groups.same.map(line)}</Section>}
          {v.projects.every((r) => !r.plan) && <p className="muted">No project has a plan yet, so there is nothing to compare.</p>}
        </div>
      </div>
      {saving && <SaveDefaults next={draft} rows={v.projects} running={running} onClose={() => setSaving(false)}
        onSaved={(changedNames) => {
          setSaving(false);
          setSaid(changedNames.length ? `Defaults saved. Tightened ${changedNames.join(", ")}; approve ${changedNames.length === 1 ? "its plan" : "their plans"} again before more steps run.` : "Defaults saved. New projects start with them.");
          void load();
          onSaved();
        }} />}
    </div>
  );
}

function SaveDefaults({ next, rows, running, onClose, onSaved }: { next: Defaults; rows: PolicyRow[]; running: string | null; onClose: () => void; onSaved: (changed: string[]) => void }) {
  const move = looserThan(rows, next);
  const free = move.filter((r) => !r.approved), held = move.filter((r) => r.approved);
  const [on, setOn] = useState<string[]>(free.map((r) => r.name));
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const flip = (n: string) => setOn(on.includes(n) ? on.filter((x) => x !== n) : [...on, n]);
  const ends = held.filter((r) => on.includes(r.name)).length;
  const save = async () => {
    setBusy(true); setErr("");
    try { onSaved(await api.call<string[]>("set_policy_defaults", { defaults: next, tighten: on })); }
    catch (e) { setErr(text(e)); } finally { setBusy(false); }
  };
  const box = (r: PolicyRow, note: string, off = false) => (
    <label key={r.name} className="pick-row"><input type="checkbox" checked={on.includes(r.name)} disabled={off} onChange={() => flip(r.name)} /><span>{r.name}<br /><span className="muted">{note}</span></span></label>
  );
  return (
    <Dialog title="Save defaults for new projects" onClose={onClose}>
      <p className="muted">Projects you already have keep their policy. {move.length ? "These allow more than the new defaults. Tick one to tighten it." : "None allows more than these."}</p>
      {free.map((r) => box(r, "Not approved yet, so nothing ends."))}
      {held.map((r) => box(r, r.name === running ? "Running. Stop it before changing its policy." : `Ends approval ${r.hash ?? ""}. Its plan is approved again before more steps run.`, r.name === running))}
      {err && <p className="field-error" role="alert">{err}</p>}
      <div className="dlg-foot">
        <button data-autofocus onClick={onClose}>Cancel</button>
        <span className="sp" />
        <button className="primary" disabled={busy} onClick={() => void save()}>{ends ? `Save and end ${ends} ${ends === 1 ? "approval" : "approvals"}` : "Save defaults"}</button>
      </div>
    </Dialog>
  );
}
