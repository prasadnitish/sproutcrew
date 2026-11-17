import Composer from "./Composer";
import { useEffect, useState } from "react";
import { api, Artifact, ChatMessage, Input, Note, Stage, Trace } from "./api";
import Markdown from "./Document";
import { Chip, Ctx, effort, provider, roleName } from "./ui";

type Tool = "ask" | "note" | "inputs";

/** The name a person knows a source by, from a reference such as `src-2#p3`. */
function sourceName(ctx: Ctx, ref: string) {
  if (ref === "result") return "This result";
  if (ref.startsWith("step:")) return `The result of ${ref.slice(5)}`;
  const id = ref.split("#")[0];
  const s = ctx.s.sources.find((x) => x.id === id);
  const page = /#p(\d+)/.exec(ref)?.[1];
  return s ? `${s.name}${page && s.pages > 1 ? `, page ${page}` : ""}` : ref;
}

function Answer({ ctx, m }: { ctx: Ctx; m: ChatMessage }) {
  const found = m.citations.filter((c) => c.found).length;
  return (
    <div className="bubble answer">
      <Markdown text={m.text} />
      {m.citations.length > 0 ? (
        <>
          <p className="muted cite-head">
            {found === m.citations.length ? `All ${found} quotes were found in the text they name.` : `${found} of ${m.citations.length} quotes were found in the text they name.`}
          </p>
          <ul className="cites">
            {m.citations.map((c, i) => (
              <li key={i} className={c.found ? "found" : "missing"}>
                <Chip tone={c.found ? "ok" : "warn"}>{c.found ? "Found" : "Not found"}</Chip>
                <span className="cite-body">
                  <span className="cite-src">{sourceName(ctx, c.source)}</span>
                  <q>{c.quote}</q>
                  {!c.found && c.note && <span className="muted"> {c.note}</span>}
                </span>
              </li>
            ))}
          </ul>
        </>
      ) : (
        <p className="muted cite-head">It offered no quotes, so nothing in this answer has been checked.</p>
      )}
      {m.basis === "current" && <p className="muted cite-head">This step ran before its inputs were recorded, so the model was shown the files as they are now.</p>}
      {m.basis === "rebuilt" && <p className="muted cite-head">This step ran before its exact text was kept, so the model was shown the same files and results put back together. With very large inputs a part may differ from what the step saw.</p>}
      <p className="muted who-line">{m.by}</p>
    </div>
  );
}

function AskPanel({ ctx, st }: { ctx: Ctx; st: Stage }) {
  const [msgs, setMsgs] = useState<ChatMessage[] | null>(null);
  const [q, setQ] = useState("");
  const [role, setRole] = useState("");
  const roles = (ctx.s.plan?.team ?? []).map((t) => t.role);
  const load = () => api.call<ChatMessage[]>("step_chat", { stage: st.id }).then(setMsgs).catch(() => setMsgs([]));
  useEffect(() => { setMsgs(null); setRole(""); void load(); }, [st.id]);
  const send = () =>
    ctx.guard(async () => {
      await api.run("ask_step", { engine: ctx.engine, stage: st.id, role, question: q }, ctx.track);
      setQ("");
      await load();
    });
  return (
    <div className="drawer talk">
      <p className="muted">
        Ask where a figure came from, why a choice was made, or whether the files support a claim. A model answers from what this step was given and what it wrote. It did not watch the step run, so treat the answer as its account of the record. The quotes marked Found are checked against the stored text.
      </p>
      {msgs && msgs.length > 0 && (
        <div className="thread" aria-live="polite">
          {msgs.map((m, i) =>
            m.role === "user" ? <div className="bubble you" key={i}>{m.text}</div> : <Answer ctx={ctx} m={m} key={i} />,
          )}
        </div>
      )}
      <Composer label="Your question" value={q} onChange={setQ} onSend={send} sendLabel="Ask" placeholder="Where does the 84% come from?"
        blocked={ctx.busy ? "Wait for the current run to finish." : ""}
        menu={<select className="quiet-menu" aria-label="Who answers" value={role} onChange={(e) => setRole(e.target.value)}>
          <option value="">{roleName(st.owner_role)} (wrote it)</option>
          {roles.filter((r) => r !== st.owner_role).map((r) => <option key={r} value={r}>{roleName(r)}</option>)}
        </select>} />
    </div>
  );
}

function NotePanel({ ctx, st, hasResult }: { ctx: Ctx; st: Stage; hasResult: boolean }) {
  const [text, setText] = useState("");
  const [scope, setScope] = useState<"step" | "all">("step");
  const [saved, setSaved] = useState(false);
  const mine = ctx.s.notes.filter((n: Note) => !n.stage || n.stage === st.id);
  useEffect(() => setSaved(false), [st.id]);
  const save = () =>
    ctx.guard(async () => {
      await api.call("add_note", { stage: scope === "all" ? "" : st.id, text });
      setText("");
      setSaved(true);
    });
  return (
    <div className="drawer talk">
      <p className="muted">
        A note is your own instruction to this step, such as a date, a figure to use, or something to leave out. It goes into the step's next run, and the result does not change until you run the step again.
      </p>
      {mine.length > 0 && (
        <ul className="notes">
          {mine.map((n) => (
            <li key={n.id}>
              <Chip tone="quiet">{n.stage ? "This step" : "Every step"}</Chip>
              <span className="note-text">{n.text}</span>
              <button className="ghost" aria-label={`Remove the note: ${n.text.slice(0, 40)}`} onClick={() => ctx.guard(() => api.call("remove_note", { id: n.id }))}>Remove</button>
            </li>
          ))}
        </ul>
      )}
      <Composer label="New note" value={text} onChange={(v) => { setText(v); setSaved(false); }} onSend={save} sendLabel="Save note" placeholder="Use 14 January as the board date."
        blocked={ctx.busy ? "Wait for the current run to finish." : ""}
        menu={<select className="quiet-menu" aria-label="Who the note is for" value={scope} onChange={(e) => setScope(e.target.value as "step" | "all")}>
          <option value="step">For this step</option>
          <option value="all">For every step</option>
        </select>}
        extra={saved && hasResult ? (
          <button disabled={!!ctx.busy || ctx.s.gate !== "allowed"} onClick={() => ctx.guard(() => api.run("run_stage", { engine: ctx.engine, stageId: st.id }, ctx.track))}>
            Run this step again
          </button>
        ) : undefined} />
      {saved && <p className="muted" role="status">Saved. It applies the next time this step runs{scope === "all" ? ", and every other step" : ""}.</p>}
    </div>
  );
}

const KIND_HEAD: Record<Input["kind"], string> = {
  requirements: "Instructions for the task",
  template: "Template to fill",
  source: "Files",
  previous: "Its own last version",
  earlier: "Results of earlier steps",
  later: "Results of later steps, as feedback",
};

function InputsPanel({ ctx, st }: { ctx: Ctx; st: Stage }) {
  const [t, setT] = useState<Trace | null | undefined>(undefined);
  useEffect(() => {
    setT(undefined);
    api.call<Trace | null>("step_inputs", { stage: st.id }).then(setT).catch(() => setT(null));
  }, [st.id, ctx.s.artifacts.length]);
  if (t === undefined) return <div className="drawer talk"><p className="muted">Opening the record</p></div>;
  if (t === null) return <div className="drawer talk"><p className="muted">Nothing was recorded for this result. It was made before SproutCrew kept this record, or the step has not run.</p></div>;
  const groups = (["requirements", "template", "source", "previous", "earlier", "later"] as const).map((k) => [k, t.inputs.filter((i) => i.kind === k)] as const).filter(([, l]) => l.length);
  return (
    <div className="drawer talk">
      <p className="muted">What this step was given when it ran, from the record made at the time. Long files and results may have been cut to fit.</p>
      <dl className="given">
        <dt>Ran on</dt>
        <dd>{provider(t.route)}, {t.model}, {effort(t.effort).toLowerCase()} effort</dd>
        {t.guide && <><dt>Its instructions</dt><dd>{t.guide}</dd></>}
        {(t.skills?.length ?? 0) > 0 && <><dt>Skills it used</dt><dd>{t.skills!.join(", ")}</dd></>}
        {t.notes.length > 0 && (
          <>
            <dt>Your notes</dt>
            <dd><ul className="plain">{t.notes.map((n) => <li key={n.id}>{n.text}</li>)}</ul></dd>
          </>
        )}
        {groups.map(([k, list]) => (
          <div key={k} className="given-group">
            <dt>{KIND_HEAD[k]}</dt>
            <dd><ul className="plain">{list.map((i) => <li key={`${i.kind}${i.id}`}>{i.label}</li>)}</ul></dd>
          </div>
        ))}
        {groups.length === 0 && <><dt>Files and results</dt><dd className="muted">None. It worked from the brief alone.</dd></>}
      </dl>
    </div>
  );
}

/** Three things to do with a finished step besides reading it: question it, instruct it, see what it was given. */
export default function StepTalk({ ctx, st, a }: { ctx: Ctx; st: Stage; a?: Artifact }) {
  const [tool, setTool] = useState<Tool | null>(null);
  useEffect(() => setTool(null), [st.id]);
  const notes = ctx.s.notes.filter((n) => !n.stage || n.stage === st.id).length;
  const tab = (id: Tool, label: string, badge?: number) => (
    <button key={id} className={`pill${tool === id ? " on" : ""}`} aria-expanded={tool === id} onClick={() => setTool(tool === id ? null : id)}>
      {label}{badge ? <span className="tab-badge">{badge}</span> : null}
    </button>
  );
  return (
    <div className="steptalk">
      <div className="pills" role="group" aria-label="Question or instruct this step">
        {a && tab("ask", "Ask about this result")}
        {tab("note", "Add a note", notes)}
        {a && tab("inputs", "What it was given")}
      </div>
      {tool === "ask" && a && <AskPanel ctx={ctx} st={st} />}
      {tool === "note" && <NotePanel ctx={ctx} st={st} hasResult={!!a} />}
      {tool === "inputs" && a && <InputsPanel ctx={ctx} st={st} />}
    </div>
  );
}
