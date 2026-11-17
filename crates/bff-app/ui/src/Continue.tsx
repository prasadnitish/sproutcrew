import { useEffect, useState } from "react";
import { api, UseCase } from "./api";
import { mainResult, nextBrief } from "./handoff";
import { Chip, Ctx, text } from "./ui";

const size = (b: number) => (b < 1024 ? `${b} bytes` : `${Math.round(b / 1024)} KB`);

/** What the crew built: one button to open it, and its files. */
export function AppBox({ ctx }: { ctx: Ctx }) {
  const [err, setErr] = useState("");
  const files = ctx.s.code_files ?? [];
  const app = ctx.s.app;
  const act = (f: () => Promise<unknown>) => ctx.guard(async () => { try { await f(); setErr(""); } catch (e) { setErr(text(e)); throw e; } });
  return (
    <>
      {app?.opens && <p className="muted lead">Opens in your browser from this Mac, at a private address. It is read-only, and it cannot reach the internet.</p>}
      {app?.page && !app.opens && <p className="field-error" role="alert">This app cannot be opened here: {app.why}. You can still open its files.</p>}
      {app && !app.page && <p className="muted lead">This app has no web page to open. Its README says how to run it.</p>}
      <div className="actions">
        {app?.opens && <button className="primary big" disabled={!!ctx.busy} onClick={() => act(() => api.call("start_preview"))}>Open the app</button>}
        <button className={app?.opens ? "ghost" : "primary"} disabled={!!ctx.busy} onClick={() => act(() => api.call("reveal_code"))}>Show the files</button>
      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
      <ul className="plain code-files" aria-label="Files">
        {files.map((f) => <li key={f.path}><span className="path">{f.path}</span><span className="muted tabular">{size(f.bytes)}</span></li>)}
      </ul>
    </>
  );
}

/** Carries results into a new project of any use case, and links projects that already exist. */
export function ContinueBox({ ctx }: { ctx: Ctx }) {
  const { s } = ctx;
  const done = (s.plan?.stages ?? []).filter((st) => s.artifacts.some((a) => a.name === st.id));
  const [cases, setCases] = useState<UseCase[]>([]);
  const [kind, setKind] = useState(s.code_files?.length ? "custom" : "app_development");
  const [on, setOn] = useState<string[]>(() => [mainResult(done)].filter(Boolean));
  const [brief, setBrief] = useState(() => nextBrief(s.code_files?.length ? "custom" : "app_development", s.name));
  const [names, setNames] = useState<string[]>([]);
  const [other, setOther] = useState("");
  const [err, setErr] = useState("");
  useEffect(() => { api.call<UseCase[]>("use_cases").then(setCases).catch(() => setCases([])); }, []);
  useEffect(() => { api.call<string[]>("list_projects").then(setNames).catch(() => setNames([])); }, [s.links]);
  const from = s.links?.from ?? [];
  const to = s.links?.to ?? [];
  const free = names.filter((n) => n !== s.name && !from.includes(n) && !to.includes(n));
  const choose = (k: string) => {
    if (brief === nextBrief(kind, s.name)) setBrief(nextBrief(k, s.name));
    setKind(k);
  };
  const act = (f: () => Promise<unknown>) => ctx.guard(async () => { try { await f(); setErr(""); } catch (e) { setErr(text(e)); throw e; } });
  return (
    <>
      <p className="muted lead">Starts a new project that reads the results you pick here. For example, build the prototype after the case study is finished. The two projects are linked, and this one does not change.</p>
      {done.length === 0 ? <p className="empty-note">Run a step first. A new project starts from finished results.</p> : (
        <>
          <label>What should the new project do?
            <select value={kind} onChange={(e) => choose(e.target.value)}>
              {cases.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
              <option value="custom">Something else</option>
            </select>
          </label>
          <fieldset className="plain-field">
            <legend>Which results should it read?</legend>
            {done.map((st) => (
              <label className="check" key={st.id}>
                <input type="checkbox" checked={on.includes(st.id)} onChange={(e) => setOn(e.target.checked ? [...on, st.id] : on.filter((x) => x !== st.id))} />
                {st.title}
              </label>
            ))}
          </fieldset>
          <label>Brief for the new project
            <textarea rows={3} value={brief} onChange={(e) => setBrief(e.target.value)} />
          </label>
          <div className="actions">
            <button className="primary" disabled={!!ctx.busy || on.length === 0 || !brief.trim()} onClick={() => ctx.onContinue({ from: s.name, stages: done.map((st) => st.id).filter((id) => on.includes(id)), kind, brief: brief.trim() })}>Set up the new project</button>
          </div>
          <p className="muted">You name it and check the crew next. Nothing runs until you approve its plan.</p>
        </>
      )}
      <div className="link-box">
        <h3>Link a project that already exists</h3>
        <p className="muted">A link lets you move between two projects. It gives neither one the other's results. To use results, add them under Sources.</p>
        {from.length > 0 && <ul className="plain">{from.map((n) => <li key={n}><span>Built from <strong>{n}</strong></span><button className="ghost" disabled={!!ctx.busy} onClick={() => act(() => api.call("unlink_project", { from: n }))}>Remove the link</button></li>)}</ul>}
        {free.length > 0 && (
          <div className="actions tight">
            <select aria-label="Project this one was built from" value={other} onChange={(e) => setOther(e.target.value)}>
              <option value="">Choose a project</option>
              {free.map((n) => <option key={n} value={n}>{n}</option>)}
            </select>
            <button disabled={!!ctx.busy || !other} onClick={() => act(async () => { await api.call("link_project", { from: other }); setOther(""); })}>This project was built from it</button>
          </div>
        )}
      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
    </>
  );
}

/** The projects around this one, always in view, so moving between them takes one click. */
export function LinkedLine({ ctx }: { ctx: Ctx }) {
  const from = ctx.s.links?.from ?? [];
  const to = ctx.s.links?.to ?? [];
  if (from.length + to.length === 0) return null;
  const go = (n: string) => <button key={n} className="link" onClick={() => ctx.openProject(n)}>{n}</button>;
  return (
    <p className="linked-line">
      <Chip tone="quiet">Linked</Chip>
      {from.length > 0 && <span>Built from {from.map(go)}</span>}
      {to.length > 0 && <span>Continued in {to.map(go)}</span>}
    </p>
  );
}
