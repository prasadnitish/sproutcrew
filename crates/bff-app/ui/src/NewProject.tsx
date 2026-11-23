import { useEffect, useRef, useState } from "react";
import { Added, api, Recommended, Source, UseCase } from "./api";
import { AddedNote, AddMaterial, MaterialMode, SourceList } from "./Files";
import { Choice, DEFAULT_CHECKING, DEFAULT_WRITING, KEYS_CHANGED, ProviderPick, useProviders } from "./Pick";
import { POLISH_HELP, WEB_HELP } from "./Extras";
import { suggestName, uniqueName } from "./projects";
import { CONNECTED_MS, findKeyShape, isKeyInBrief, keyInBriefText, withoutKey } from "./keyshape";
import type { NextDraft } from "./handoff";
import { addedLine, PathInfo, pathLabel, sizeLabel } from "./addedFiles";
import { Chip, Dialog, Panel, text } from "./ui";

/** The outcomes whose plan ends with a step that writes the finished text, so a polishing step can follow it. */
const POLISHABLE = ["writing", "proposal", "research_brief"];
/** Outcomes that can end with a clickable prototype, once running code is allowed in Settings. */
const PROTOTYPABLE = ["brainstorm", "quick_brainstorm", "proposal"];

type Staged = { kind: "path"; value: string } | { kind: "url"; value: string } | { kind: "file"; file: File } | { kind: "results"; from: string; stages: string[] };
const shown = (f: Staged) => (f.kind === "file" ? f.file.name : f.kind === "results" ? `Results of ${f.from}: ${f.stages.join(", ")}` : f.value);

const EXAMPLES: Record<string, string> = {
  writing: "Write a 700-word newsletter for product leaders about what our customer interviews changed in the roadmap. Use a warm, direct voice and cite the supplied notes.",
  case_study: "Make the case for using AI to speed up claims handling at a mid-size insurer. The reader is the COO. Use the files for the numbers.",
  proposal: "Write a proposal for a self-service portal that cuts status calls to our support team. Use the files for what we know about the callers.",
  research_brief: "What do these reports say about the cost of manual claims intake, and where do they disagree?",
  app_development: "Build a small command-line habit tracker that stores its data in a local file and shows a weekly streak.",
  social_publishing: "Announce that the claims dashboard is live. Three posts, plain tone, one with the headline result.",
  brainstorm: "Should we add a shared family calendar to our meal-planning app? Use the files for what customers have told us.",
  quick_brainstorm: "Should we offer a yearly plan at a discount? Use the files for what customers pay today.",
  custom: "Describe the work in your own words. The crew will propose the steps and the team.",
};
const OUTCOMES: Record<string, string> = {
  writing: "Draft, edit, and fact-check an article, newsletter, report, or other piece of writing.",
  case_study: "Turn evidence into a clear recommendation, document, and presentation.",
  proposal: "Shape a problem into a proposal with scope, risks, and next steps.",
  research_brief: "Find the key points in your sources, with claims checked against the evidence.",
  app_development: "Build a small app with code, reviews, tests, and setup instructions.",
  social_publishing: "Prepare post drafts for you to review before sharing.",
  brainstorm: "Compare ideas and assumptions, challenge options, and write a decision memo.",
  quick_brainstorm: "A short decision memo: options from your files, challenged by a second AI. Four steps.",
};

/** Use cases that usually need facts from outside the person's files. */
const WEB_BY_DEFAULT: string[] = [];

export type RestartDraft = { from: string; name: string; brief: string; sources: Source[] };

export default function NewProject({ onOpen, onBack, restart, next, nextName, taken = [] }: { onOpen: (name: string) => void; onBack: () => void; restart?: RestartDraft; next?: NextDraft; nextName?: string; taken?: string[] }) {
  const providers = useProviders();
  const [cases, setCases] = useState<UseCase[]>([]);
  const [kind, setKind] = useState(next?.kind ?? "writing");
  const [name, setName] = useState(restart?.name ?? nextName ?? "");
  const [brief, setBrief] = useState(restart?.brief ?? next?.brief ?? "");
  const [rules, setRules] = useState("");
  const [inherited, setInherited] = useState<Source[]>(restart?.sources ?? []);
  // What the app found about each added path (name, size, folder or not), by the path as given.
  const [pathInfo, setPathInfo] = useState<Record<string, PathInfo>>({});
  const [staged, setStaged] = useState<Staged[]>(next ? [{ kind: "results", from: next.from, stages: next.stages }] : []);
  const [author, setAuthor] = useState<Choice>(["R12", DEFAULT_WRITING, "medium"]);
  const [reviewer, setReviewer] = useState<Choice | null>(null);
  const [auto, setAuto] = useState<Choice | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [createErr, setCreateErr] = useState("");
  // Set when the brief or name held something shaped like a key. Only that fact is kept, never the text.
  const [keyHit, setKeyHit] = useState("");
  const [connected, setConnected] = useState(false);
  const [moveErr, setMoveErr] = useState("");
  const createErrRef = useRef<HTMLParagraphElement>(null);
  const keyHitRef = useRef<HTMLDivElement>(null);
  const [done, setDone] = useState<Added | null>(null);
  const [recs, setRecs] = useState<Recommended[]>([]);
  const [skills, setSkills] = useState<string[]>([]);
  const [web, setWeb] = useState<boolean | null>(null);
  const [polish, setPolish] = useState(false);
  const [proto, setProto] = useState(false);
  const [codeOk, setCodeOk] = useState(false);
  useEffect(() => { api.call<boolean>("running_code_allowed").then(setCodeOk).catch(() => setCodeOk(false)); }, []);
  // Which option's small dialog is open (DESIGN_SPEC.md §10.1, New project).
  const [opt, setOpt] = useState<"" | "files" | "rules" | "crew" | "web" | "models">("");
  const [named, setNamed] = useState(!!(restart?.name ?? nextName));
  const [mode, setMode] = useState<MaterialMode>("files");
  // On the outcome step, focus goes back to the card already chosen, so going back never loses the place.
  // Until the person names the project, the name follows the outcome and the first words of the brief, and never repeats a project that exists.
  useEffect(() => { if (!named) setName(uniqueName(suggestName(kind, brief), taken, "-")); }, [kind, brief, named, taken.join("\n")]);
  // A create error is read at the button that caused it.
  useEffect(() => { if (createErr) createErrRef.current?.focus(); }, [createErr]);
  useEffect(() => { if (keyHit) keyHitRef.current?.focus(); }, [keyHit]);
  // "Connected to OpenRouter" shows for a few seconds, then the card goes.
  useEffect(() => {
    if (!connected) return;
    const t = setTimeout(() => setConnected(false), CONNECTED_MS);
    return () => clearTimeout(t);
  }, [connected]);

  useEffect(() => {
    if (!providers.length || providers.some(([id]) => id === author[0])) return;
    const route = providers[0][0];
    setAuthor([route, route === "R3" ? "claude-sonnet-5-5" : route === "R5" ? "gpt-6-luna" : DEFAULT_WRITING, "medium"]);
  }, [providers.map(([id]) => id).join(",")]);
  useEffect(() => { api.call<UseCase[]>("use_cases").then(setCases).catch((e) => setErr(text(e))); }, []);
  useEffect(() => {
    const other = providers.find(([id]) => id !== author[0]);
    // With OpenRouter alone, checking still uses a different maker's model from writing.
    setAuto(other ? [other[0], other[0] === "R3" ? "claude-sonnet-5-5" : other[0] === "R5" ? "gpt-6-luna" : DEFAULT_CHECKING, "high"] : [author[0], author[0] === "R12" ? DEFAULT_CHECKING : author[1], "high"]);
  }, [author.join(","), providers.map(([id]) => id).join(",")]);

  const chosen = cases.find((c) => c.id === kind);
  // What suits this crew, ticked as recommended, for the person to change before the project exists.
  const crew = (chosen?.team ?? []).map((r) => r.role).join(",");
  useEffect(() => {
    if (!crew) { setRecs([]); setSkills([]); return; }
    api.call<Recommended[]>("recommended_skills", { roles: crew.split(",") }).then((r) => { setRecs(r); setSkills(r.filter((x) => x.on).map((x) => x.skill)); }).catch(() => setRecs([]));
  }, [crew]);
  const webOn = web ?? WEB_BY_DEFAULT.includes(kind);
  const canSearch = providers.some(([id]) => id === "R12" || id === "R3" || id === "R5");
  const wantsFiles = chosen?.wants_files ?? true;
  const rev: Choice | null = reviewer ?? auto;
  const keyFound = keyHit ? (findKeyShape(brief) ?? findKeyShape(name, true)) : null;
  const nameOk = /^[A-Za-z0-9_-]{1,64}$/.test(name);
  const addPaths = (p: string[]) => {
    const fresh = p.filter((v) => !staged.some((x) => x.kind === "path" && x.value === v));
    setStaged((s) => [...s, ...p.filter((v) => !s.some((x) => x.kind === "path" && x.value === v)).map((value) => ({ kind: "path" as const, value }))]);
    // The list shows each path by name and size once the app has looked; until then it shows the path as given.
    if (fresh.length) api.call<PathInfo[]>("path_info", { paths: fresh }).then((info) => setPathInfo((m) => ({ ...m, ...Object.fromEntries(fresh.map((v, i) => [v, info[i]])) }))).catch(() => undefined);
  };
  const addUrls = (u: string[]) => setStaged((s) => [...s, ...u.filter((v) => !s.some((x) => x.kind === "url" && x.value === v)).map((value) => ({ kind: "url" as const, value }))]);
  const addResults = (from: string, stages: string[]) => setStaged((s) => [...s, { kind: "results" as const, from, stages }]);
  const addFiles = (f: File[]) => setStaged((s) => [...s, ...f.map((file) => ({ kind: "file" as const, file }))]);

  // `allowKeyShape` is "It isn't a key", for this one create only.
  const create = async (allowKeyShape = false) => {
    setBusy(true);
    setErr("");
    setCreateErr("");
    setKeyHit("");
    setMoveErr("");
    try {
      if (restart) await api.call("create_project_from", { from: restart.from, name, brief, sourceIds: inherited.map((source) => source.id), ...(allowKeyShape ? { allowKeyShape: true } : {}) });
      else await api.call("create_project", allowKeyShape ? { name, brief, allowKeyShape: true } : { name, brief });
      if (rules.trim()) await api.call("set_exercise_rules", { text: rules });
    } catch (e) {
      const why = text(e);
      if (isKeyInBrief(why)) setKeyHit(keyInBriefText(why));
      else setCreateErr(why);
      setBusy(false);
      return;
    }
    const all: Added = { added: [], skipped: [] };
    for (const f of staged) {
      try {
        const r = f.kind === "results" ? await api.call<Added>("add_source_results", { from: f.from, stages: f.stages }) : f.kind === "path" ? await api.call<Added>("add_source_path", { path: f.value }) : f.kind === "url" ? await api.call<Added>("add_source_url", { url: f.value }) : await api.addFile(f.file);
        all.added.push(...r.added);
        all.skipped.push(...r.skipped);
      } catch (e) {
        all.skipped.push({ name: shown(f), reason: text(e) });
      }
    }
    let planNote = "";
    if (kind !== "custom" && rev) {
      try {
        await api.call("use_workflow", { name: kind, author, reviewer: rev });
      } catch (e) {
        planNote = `the plan was not set: ${text(e)}. Choose a plan in Your crew`;
      }
      if (!planNote && webOn) {
        try { await api.call("set_web_research", { on: true }); } catch (e) { planNote = `web research was left out: ${text(e)}`; }
      }
      if (!planNote && polish && POLISHABLE.includes(kind)) {
        try { await api.call("set_humaniser", { on: true }); } catch (e) { planNote = `the polishing step was left out: ${text(e)}`; }
      }
      if (!planNote && proto && codeOk && PROTOTYPABLE.includes(kind)) {
        try { await api.call("set_prototype", { on: true }); } catch (e) { planNote = `the prototype step was left out: ${text(e)}`; }
      }
      for (const name of skills) {
        try { await api.call("select_tool", { kind: "skill", name }); } catch (e) { all.skipped.push({ name, reason: text(e) }); }
      }
    }
    setBusy(false);
    if (all.skipped.length === 0 && !planNote) return onOpen(name);
    setDone(all);
    setErr(planNote ? `The project was created, but ${planNote}.` : "");
  };

  // Saves the key found in the brief (or the name) and takes it out of the field. The text goes only to save_key.
  const moveKey = async () => {
    const inBrief = findKeyShape(brief);
    const found = inBrief ?? findKeyShape(name, true);
    if (!found?.openrouter) return;
    setBusy(true);
    setMoveErr("");
    try {
      await api.call("save_key", { route: "R12", key: found.key });
    } catch (e) {
      setMoveErr(text(e));
      setBusy(false);
      return;
    }
    if (inBrief) setBrief(withoutKey(brief, found.key));
    else { setNamed(false); setName(""); }
    setKeyHit("");
    setConnected(true);
    setBusy(false);
    window.dispatchEvent(new Event(KEYS_CHANGED));
  };
  const dropKey = () => {
    const inBrief = findKeyShape(brief);
    const found = inBrief ?? findKeyShape(name, true);
    if (found && inBrief) setBrief(withoutKey(brief, found.key));
    else if (found) { setNamed(false); setName(""); }
    setKeyHit("");
  };

  if (done) {
    return (
      <div className="home wizard">
        <h1>Project created</h1>
        <Panel title={name}>
          <AddedNote r={done} />
          {err && <p className="field-error" role="alert">{err}</p>}
          <div className="actions"><button className="primary big" onClick={() => onOpen(name)}>Open the project</button></div>
        </Panel>
      </div>
    );
  }

  const custom = kind === "custom";
  const detailsOk = nameOk && !!brief.trim();
  const outcomes = [...cases, { id: "custom", name: "Something else", promise: "Tell us the outcome. Your crew will suggest a plan.", steps: [], wants_files: true, team: [], owners: [] }];
  const fileCount = staged.length + inherited.length;
  const ready = detailsOk && providers.length > 0 && !!kind;
  // What is missing, in words, or the promise that nothing runs yet.
  const footLine = providers.length === 0 ? "Connect an AI service to continue." : !brief.trim() ? "Say what you want made to continue." : !nameOk ? "The project name needs fixing to continue." : "Nothing runs until you approve the plan.";
  const options: { id: Exclude<typeof opt, "">; name: string; value: string }[] = [
    { id: "files", name: "Files", value: fileCount ? `${fileCount} ${fileCount === 1 ? "file" : "files"}` : "none" },
    { id: "rules", name: "Rules", value: rules.trim() ? "set" : "none" },
    ...(custom ? [] : [
      { id: "crew" as const, name: "Crew", value: `${chosen?.team.length ?? 0} roles${skills.length ? `, ${skills.length} ${skills.length === 1 ? "skill" : "skills"}` : ""}` },
      { id: "web" as const, name: "Web research", value: !canSearch ? "needs a key" : webOn ? "on" : "off" },
      { id: "models" as const, name: "Models", value: author[1].split("/").pop() ?? author[1] },
    ]),
  ];
  const closeOpt = () => setOpt("");
  const Done = () => <div className="dlg-foot"><span className="sp" /><button className="primary" data-autofocus onClick={closeOpt}>Done</button></div>;

  return (
    <div className="composer-page" onKeyDown={(e) => { if (e.metaKey && e.key === "Enter" && ready && !busy) { e.preventDefault(); void create(); } }}>
      <div className="composer-body">
      <button className="link" disabled={busy} onClick={onBack}>{restart ? "Back to the original project" : next ? `Back to ${next.from}` : "All projects"}</button>
      {next && <p className="note">Continuing from <strong>{next.from}</strong>. {next.stages.length} {next.stages.length === 1 ? "result is" : "results are"} ready to use as sources, and the two projects will be linked.</p>}
      {restart && <p className="note">Starting over from <strong>{restart.from}</strong>. Your brief and {inherited.length} {inherited.length === 1 ? "source are" : "sources are"} ready to reuse. The original project stays unchanged.</p>}
      {(providers.length === 0 || connected) && <section className="composer-connect" aria-label="Connect an AI service"><ConnectKey connected={connected} onConnected={() => setConnected(true)} /></section>}
      <label className="composer-field">It should make
        <select value={kind} onChange={(e) => setKind(e.target.value)}>
          {outcomes.map((c) => <option key={c.id} value={c.id}>{c.id === "proposal" ? "Proposal or product brief" : c.name}</option>)}
        </select>
      </label>
      <p className="muted composer-promise">{OUTCOMES[kind] ?? chosen?.promise ?? ""}</p>
      <label className="composer-field">The brief
        <textarea className="composer-brief" rows={7} value={brief} onChange={(e) => { setBrief(e.target.value); setKeyHit(""); }} placeholder={EXAMPLES[kind]} autoFocus />
      </label>
      <label className="composer-field">Project name
        <input className="mono composer-name" value={name} onChange={(e) => { setNamed(true); setKeyHit(""); setName(e.target.value.replace(/\s+/g, "-")); }} placeholder="claims-newsletter" />
      </label>
      {name && !nameOk && <p className="field-error" role="alert">Use letters, numbers, - and _ only, up to 64 characters.</p>}
      <div className="composer-options" role="group" aria-label="Options">
        {options.map((o) => <button key={o.id} className="opt-btn" onClick={() => setOpt(o.id)}><span className="opt-name">{o.name}</span> <span className="opt-value">{o.value}</span></button>)}
      </div>
      {err && <p className="notice error" role="alert">{err}</p>}
      </div>

      {opt === "files" && <Dialog title="Files" onClose={closeOpt}>
        {restart && <><p className="muted">These are saved copies from the original project, including source roles and warnings. Removing one here only leaves it out of the fresh project.</p><SourceList sources={inherited} locked={false} busy={busy} onRemove={(id) => setInherited((sources) => sources.filter((source) => source.id !== id))} /></>}
        <p className="muted">{wantsFiles ? "Add reports, notes, or instructions for your crew to read and cite, such as the dates, prices and names it should use. You can add more later." : "Add any context that would help. You can also continue without files."}</p>
        <AddMaterial onPaths={addPaths} onFiles={addFiles} onUrls={addUrls} onPick={addResults} current={name} disabled={busy} mode={mode} onMode={setMode} />
        <p className="added-count" role="status" aria-live="polite">{addedLine(staged.map((f) => ({ kind: f.kind, folder: f.kind === "path" && !!pathInfo[f.value]?.is_dir })))}</p>
        {staged.length > 0 && (
          <ul className="plain staged" aria-label="Files added">
            {staged.map((f, i) => (
              <li key={i}>
                <span title={f.kind === "path" ? f.value : undefined}>{f.kind === "url" && <Chip tone="info">Web</Chip>}{f.kind === "results" && <Chip tone="info">Project</Chip>} {f.kind === "path" ? pathLabel(f.value, pathInfo[f.value]) : f.kind === "file" ? `${f.file.name} · ${sizeLabel(f.file.size)}` : shown(f)}</span>
                <button className="ghost" onClick={() => setStaged(staged.filter((_, j) => j !== i))} aria-label={`Leave out ${shown(f)}`}>Leave out</button>
              </li>
            ))}
          </ul>
        )}
        <Done />
      </Dialog>}
      {opt === "rules" && <Dialog title="Rules" onClose={closeOpt}>
        <p className="muted">Anything your crew must follow, such as a word limit or a required format. Rules come before everything else your crew is told.</p>
        <textarea aria-label="Rules" rows={4} value={rules} maxLength={6000} onChange={(e) => setRules(e.target.value)} placeholder="Keep it under 800 words. Use British spelling. Cite a source for every number." />
        <Done />
      </Dialog>}
      {opt === "crew" && <Dialog title="Crew" onClose={closeOpt}>
        <p className="muted">Writers make the work; a second check reads it and raises problems for you to decide.</p>
        <ul className="plain crew-compact">
          {(chosen?.team ?? []).map((r) => <li key={r.role} title={r.does}><strong>{r.label}</strong> <span className={`side ${r.side}`}>{r.side === "writing" ? "Writes" : "Checks"}</span></li>)}
        </ul>
        {recs.map((r) => (
          <label className="check rec" key={r.skill}>
            <input type="checkbox" checked={skills.includes(r.skill)} onChange={(e) => setSkills(e.target.checked ? [...skills, r.skill] : skills.filter((x) => x !== r.skill))} />
            <span><strong>{r.label ?? r.skill}</strong> <span className="muted">{r.why}</span></span>
          </label>
        ))}
        {POLISHABLE.includes(kind) && <label className="check rec"><input type="checkbox" checked={polish} onChange={(e) => setPolish(e.target.checked)} /><span><strong>Polish the finished text</strong> <span className="muted">{POLISH_HELP}</span></span></label>}
        {codeOk && PROTOTYPABLE.includes(kind) && <label className="check rec"><input type="checkbox" checked={proto} onChange={(e) => setProto(e.target.checked)} /><span><strong>Add a clickable prototype</strong> <span className="muted">One more step builds a few clickable screens of what is recommended, with sample data, to try in SproutCrew’s own preview. You review the code before it is used.</span></span></label>}
        <Done />
      </Dialog>}
      {opt === "web" && <Dialog title="Web research" onClose={closeOpt}>
        <label className="check rec">
          <input type="checkbox" disabled={!canSearch} checked={webOn && canSearch} onChange={(e) => setWeb(e.target.checked)} />
          <span><strong>Search the web first</strong> <span className="muted">{canSearch ? (kind === "social_publishing" ? "Recommended for posts: first checks each platform's current character limits, image sizes and hashtag norms on the web. Needs a paid model and adds a small search fee." : WEB_HELP) : "Needs an OpenRouter, Anthropic or OpenAI key. Until then, add web page addresses as sources."}</span></span>
        </label>
        <Done />
      </Dialog>}
      {opt === "models" && <Dialog title="Models" onClose={closeOpt}>
        <ProviderPick label="Writing side" side="writing" value={author} onChange={setAuthor} />
        {rev && <ProviderPick label="Checking side" side="checking" value={rev} onChange={setReviewer} />}
        {reviewer && <div className="actions tight"><button className="ghost" onClick={() => setReviewer(null)}>Use the automatic checking side</button></div>}
        <p className="muted">Checking works best on a model from a different maker than writing. After the project exists you can choose a model for each step.</p>
        <Done />
      </Dialog>}

      <div className="composer-foot">
        {createErr && <p ref={createErrRef} className="notice error" role="alert" tabIndex={-1}>{createErr}</p>}
        {keyHit && (
          <div ref={keyHitRef} className="notice error key-in-brief" role="alert" tabIndex={-1}>
            <p>{keyHit}</p>
            {moveErr && <p className="field-error">{moveErr}</p>}
            {keyFound ? (
              <div className="actions tight">
                {keyFound.openrouter ? <button disabled={busy} onClick={() => void moveKey()}>Move it there</button> : <button disabled={busy} onClick={dropKey}>Remove it</button>}
                <button className="ghost" disabled={busy} onClick={() => void create(true)}>It isn’t a key</button>
              </div>
            ) : <p className="muted">Take the key out of the brief by hand, then press Create.</p>}
          </div>
        )}
        <div className="composer-foot-row">
          <span className="muted" role="status">{footLine}</span>
          <button disabled={busy} onClick={onBack}>Cancel</button>
          <button className="primary" disabled={busy || !ready} onClick={() => void create()}>{busy ? "Creating" : restart ? "Start fresh project" : "Create project"} {!busy && ready && <kbd>⌘⏎</kbd>}</button>
        </div>
      </div>
    </div>
  );
}

/** A tick and "Connected to OpenRouter", read out by screen readers. */
function ConnectedNote() {
  return (
    <p className="connected-note" role="status">
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg> <strong>Connected to OpenRouter</strong>
    </p>
  );
}

/** Lets a person with no key connect OpenRouter here, without leaving the new project. The key goes to the Keychain. */
function ConnectKey({ connected, onConnected }: { connected: boolean; onConnected: () => void }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const save = () => {
    setBusy(true);
    setErr("");
    api.call("save_key", { route: "R12", key })
      .then(() => { setKey(""); onConnected(); window.dispatchEvent(new Event(KEYS_CHANGED)); })
      .catch((e) => setErr(text(e)))
      .finally(() => setBusy(false));
  };
  if (connected) {
    return (
      <div className="notice connect-key connected"><ConnectedNote /></div>
    );
  }
  return (
    <div className="notice connect-key">
      <p><strong>Connect an AI service to run this project.</strong> Paste an OpenRouter key. It is kept in this Mac’s Keychain.</p>
      <div className="path-row first">
        <label>OpenRouter key
          <input type="password" autoComplete="off" value={key} disabled={busy} onChange={(e) => setKey(e.target.value)} onKeyDown={(e) => e.key === "Enter" && key.trim() && save()} />
        </label>
        <button disabled={busy || !key.trim()} onClick={save}>{busy ? "Checking the key" : "Check and save"}</button>
        <button className="link" onClick={() => void api.call("open_link", { link: "R12-key" }).catch(() => undefined)}>Get a key</button>
      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
    </div>
  );
}
