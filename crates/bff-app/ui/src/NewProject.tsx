import { KeyboardEvent, useEffect, useRef, useState } from "react";
import { Added, api, Recommended, Source, UseCase } from "./api";
import { AddedNote, AddMaterial, MaterialMode, SourceList } from "./Files";
import { Choice, DEFAULT_CHECKING, DEFAULT_WRITING, KEYS_CHANGED, ProviderPick, useProviders } from "./Pick";
import { choiceIndex } from "./navigation";
import { POLISH_HELP, WEB_HELP } from "./Extras";
import { suggestName } from "./projects";
import type { NextDraft } from "./handoff";
import { Chip, Panel, text } from "./ui";

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
  custom: "Describe the work in your own words. The coordinator will propose the steps and the team.",
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

export default function NewProject({ onOpen, onBack, restart, next, nextName }: { onOpen: (name: string) => void; onBack: () => void; restart?: RestartDraft; next?: NextDraft; nextName?: string }) {
  const providers = useProviders();
  const [cases, setCases] = useState<UseCase[]>([]);
  const [kind, setKind] = useState(next?.kind ?? "writing");
  const [name, setName] = useState(restart?.name ?? nextName ?? "");
  const [brief, setBrief] = useState(restart?.brief ?? next?.brief ?? "");
  const [rules, setRules] = useState("");
  const [inherited, setInherited] = useState<Source[]>(restart?.sources ?? []);
  const [staged, setStaged] = useState<Staged[]>(next ? [{ kind: "results", from: next.from, stages: next.stages }] : []);
  const [author, setAuthor] = useState<Choice>(["R12", DEFAULT_WRITING, "medium"]);
  const [reviewer, setReviewer] = useState<Choice | null>(null);
  const [auto, setAuto] = useState<Choice | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [done, setDone] = useState<Added | null>(null);
  const [recs, setRecs] = useState<Recommended[]>([]);
  const [skills, setSkills] = useState<string[]>([]);
  const [web, setWeb] = useState<boolean | null>(null);
  const [polish, setPolish] = useState(false);
  const [proto, setProto] = useState(false);
  const [codeOk, setCodeOk] = useState(false);
  useEffect(() => { api.call<boolean>("running_code_allowed").then(setCodeOk).catch(() => setCodeOk(false)); }, []);
  const [step, setStep] = useState(next ? 1 : 0);
  const [named, setNamed] = useState(!!(restart?.name ?? nextName));
  const [mode, setMode] = useState<MaterialMode>("files");
  const screen = useRef<HTMLDivElement>(null);
  // On the outcome step, focus goes back to the card already chosen, so going back never loses the place.
  useEffect(() => { screen.current?.querySelector<HTMLElement>(step === 0 ? '[role="radio"][aria-checked="true"]' : 'h2')?.focus(); }, [step, cases.length > 0]);
  // Until the person names the project, the name follows the outcome and the first words of the brief.
  useEffect(() => { if (!named) setName(suggestName(kind, brief)); }, [kind, brief, named]);

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
  const nameOk = /^[A-Za-z0-9_-]{1,64}$/.test(name);
  const addPaths = (p: string[]) => setStaged((s) => [...s, ...p.filter((v) => !s.some((x) => x.kind === "path" && x.value === v)).map((value) => ({ kind: "path" as const, value }))]);
  const addUrls = (u: string[]) => setStaged((s) => [...s, ...u.filter((v) => !s.some((x) => x.kind === "url" && x.value === v)).map((value) => ({ kind: "url" as const, value }))]);
  const addResults = (from: string, stages: string[]) => setStaged((s) => [...s, { kind: "results" as const, from, stages }]);
  const addFiles = (f: File[]) => setStaged((s) => [...s, ...f.map((file) => ({ kind: "file" as const, file }))]);

  const create = async () => {
    setBusy(true);
    setErr("");
    try {
      if (restart) await api.call("create_project_from", { from: restart.from, name, brief, sourceIds: inherited.map((source) => source.id) });
      else await api.call("create_project", { name, brief });
      if (rules.trim()) await api.call("set_exercise_rules", { text: rules });
    } catch (e) {
      setErr(text(e));
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
  const steps = custom ? ["Choose an outcome", "Your brief"] : ["Choose an outcome", "Your brief", "Customize the crew (optional)"];
  const detailsOk = nameOk && !!brief.trim();
  const outcomes = [...cases, { id: "custom", name: "Something else", promise: "Tell us the outcome. Your crew will suggest a plan.", steps: [], wants_files: true, team: [], owners: [] }];
  const pick = (id: string) => { setKind(id); setStep(1); };
  // Arrow keys move between outcomes without leaving the step; Enter or a click moves on.
  const arrows = (e: KeyboardEvent<HTMLDivElement>) => {
    const options = [...e.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    const at = choiceIndex(e.key, options.indexOf(e.target as HTMLButtonElement), options.length);
    if (at < 0) return;
    e.preventDefault();
    setKind(outcomes[at].id);
    options[at].focus();
  };

  return (
    <div className="home wizard">
      <button className="link" disabled={busy} onClick={step === 0 ? onBack : () => setStep(step - 1)}>{step === 0 ? (restart ? "Back to the original project" : next ? `Back to ${next.from}` : "All projects") : "Back"}</button>
      {next && <p className="notice restart-note">Continuing from <strong>{next.from}</strong>. {next.stages.length} {next.stages.length === 1 ? "result is" : "results are"} ready to use as sources, and the two projects will be linked.</p>}
      {restart && <p className="notice restart-note">Starting over from <strong>{restart.from}</strong>. Your brief and {inherited.length} {inherited.length === 1 ? "source are" : "sources are"} ready to reuse. The original project stays unchanged.</p>}
      <ol className="stepper" aria-label="Progress">
        {steps.map((label, i) => (
          <li key={label} className={i < step ? "past" : i === step ? "now" : ""} aria-current={i === step ? "step" : undefined}>
            <span className="dot" aria-hidden="true">{i < step ? <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg> : i + 1}</span>{label}
          </li>
        ))}
      </ol>

      <div className="wizard-screen" key={step} ref={screen}>
        {step === 0 && (
          <>
            <h2 className="screen-title compact" tabIndex={-1}>What would you like help with?</h2>
            <p className="lede one-line">Pick one to start. You can change the plan before anything runs.</p>
            <div className="cards outcomes" role="radiogroup" aria-label="Outcome" onKeyDown={arrows}>
              {outcomes.map((c) => (
                <button key={c.id} role="radio" aria-checked={kind === c.id} tabIndex={kind === c.id ? 0 : -1} className={`choice${kind === c.id ? " on" : ""}`} onClick={() => pick(c.id)}>
                  <strong>{c.id === "proposal" ? "Proposal or product brief" : c.name}</strong>
                  <span className="muted">{OUTCOMES[c.id] ?? c.promise}</span>
                </button>
              ))}
            </div>
          </>
        )}

        {step === 1 && (
          <>
            <h2 className="screen-title compact" tabIndex={-1}>Your brief</h2>
            {providers.length === 0 && <ConnectKey />}
            <label>What would you like to achieve?
              <textarea rows={5} value={brief} onChange={(e) => setBrief(e.target.value)} placeholder={EXAMPLES[kind]} autoFocus />
            </label>
            <label>Project name
              <input value={name} onChange={(e) => { setNamed(true); setName(e.target.value.replace(/\s+/g, "-")); }} placeholder="claims-newsletter" />
            </label>
            {name && !nameOk && <p className="field-error" role="alert">Use letters, numbers, - and _ only, up to 64 characters.</p>}
            <h3>Rules (optional)</h3>
            <p className="muted">Anything your crew must follow, such as a word limit or a required format. Rules come before everything else your crew is told.</p>
            <textarea aria-label="Rules" rows={2} value={rules} maxLength={6000} onChange={(e) => setRules(e.target.value)} placeholder="Keep it under 800 words. Use British spelling. Cite a source for every number." />
            <h3>Sources (optional)</h3>
            {restart && <><p className="muted">These are saved copies from the original project, including source roles and warnings. Removing one here only leaves it out of the fresh project.</p><SourceList sources={inherited} locked={false} busy={busy} onRemove={(id) => setInherited((sources) => sources.filter((source) => source.id !== id))} /></>}
            <p className="muted">{wantsFiles ? "Add reports, notes, or instructions for your crew to read and cite. You can add more later." : "Add any context that would help. You can also continue without files."}</p>
            <AddMaterial onPaths={addPaths} onFiles={addFiles} onUrls={addUrls} onPick={addResults} current={name} disabled={busy} mode={mode} onMode={setMode} />
            {staged.length > 0 && (
              <ul className="plain staged">
                {staged.map((f, i) => (
                  <li key={i}>
                    <span>{f.kind === "url" && <Chip tone="info">Web</Chip>}{f.kind === "results" && <Chip tone="info">Project</Chip>} {shown(f)}</span>
                    <button className="ghost" onClick={() => setStaged(staged.filter((_, j) => j !== i))} aria-label={`Remove ${shown(f)}`}>Remove</button>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}

        {step === 2 && !custom && (
          <>
            <h2 className="screen-title compact" tabIndex={-1}>Customize the crew</h2>
            <p className="lede one-line">Writers make the work; checkers read it and raise problems for you to decide.</p>
            <ul className="plain crew-compact">
              {(chosen?.team ?? []).map((r) => (
                <li key={r.role} title={r.does}>
                  <strong>{r.label}</strong> <span className={`side ${r.side}`}>{r.side === "writing" ? "Writes" : "Checks"}</span>
                </li>
              ))}
            </ul>
            {recs.length > 0 && (
              <section className="recommended" aria-label="Skills">
                <h3>Skills</h3>
                {recs.map((r) => (
                  <label className="check rec" key={r.skill}>
                    <input type="checkbox" checked={skills.includes(r.skill)} onChange={(e) => setSkills(e.target.checked ? [...skills, r.skill] : skills.filter((x) => x !== r.skill))} />
                    <span><strong>{r.label ?? r.skill}</strong> <span className="muted">{r.why}</span></span>
                  </label>
                ))}
              </section>
            )}
            <label className="check rec">
              <input type="checkbox" disabled={!canSearch} checked={webOn && canSearch} onChange={(e) => setWeb(e.target.checked)} />
              <span><strong>Web research</strong> <span className="muted">{canSearch ? (kind === "social_publishing" ? "Recommended for posts: first checks each platform's current character limits, image sizes and hashtag norms on the web. Needs a paid model and adds a small search fee." : WEB_HELP) : "Needs an OpenRouter, Anthropic or OpenAI key. Until then, add web page addresses as sources."}</span></span>
            </label>
            {POLISHABLE.includes(kind) && (
              <label className="check rec">
                <input type="checkbox" checked={polish} onChange={(e) => setPolish(e.target.checked)} />
                <span><strong>Polish the finished text</strong> <span className="muted">{POLISH_HELP}</span></span>
              </label>
            )}
            {codeOk && PROTOTYPABLE.includes(kind) && (
              <label className="check rec">
                <input type="checkbox" checked={proto} onChange={(e) => setProto(e.target.checked)} />
                <span><strong>Add a clickable prototype</strong> <span className="muted">One more step builds a few clickable screens of what is recommended, with sample data, to try in SproutCrew’s own preview. If your sources include screenshots, it shows a before and an after. You review the code before it is used.</span></span>
              </label>
            )}
            <details className="customize-models">
              <summary>Customize models <span className="meta">Writing: {author[1]} · Checking: {rev?.[1] ?? "automatic"}</span></summary>
              <ProviderPick label="Writing side" side="writing" value={author} onChange={setAuthor} />
              {rev && <ProviderPick label="Checking side" side="checking" value={rev} onChange={setReviewer} />}
              {reviewer && <div className="actions tight"><button className="ghost" onClick={() => setReviewer(null)}>Use the automatic checking side</button></div>}
              <p className="muted">Checking works best on a model from a different maker than writing. After the project exists you can choose a model for each step.</p>
            </details>
          </>
        )}
      </div>

      {err && <p className="notice error" role="alert">{err}</p>}
      {step >= 1 && (
        <div className="wizard-footer">
          <span className="muted" role="status">{!brief.trim() ? "Say what you want made to continue." : !nameOk ? "The project name needs fixing to continue." : "Nothing runs until you approve the plan."}</span>
          <div className="actions">
            <button className="ghost" disabled={busy} onClick={onBack}>Cancel</button>
            {step === 1 && !custom && <button disabled={busy || !detailsOk} onClick={() => setStep(2)}>Customize the crew</button>}
            <button className="primary big" disabled={busy || !detailsOk} onClick={create}>{busy ? "Creating" : restart ? "Start fresh project" : "Create project"}</button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Lets a person with no key connect OpenRouter here, without leaving the new project. The key goes to the Keychain. */
function ConnectKey() {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const save = () => {
    setBusy(true);
    setErr("");
    api.call("save_key", { route: "R12", key })
      .then(() => { setKey(""); window.dispatchEvent(new Event(KEYS_CHANGED)); })
      .catch((e) => setErr(text(e)))
      .finally(() => setBusy(false));
  };
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
