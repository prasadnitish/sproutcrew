import { useEffect, useState } from "react";
import { api, Exported, Post, WriteItem } from "./api";
import { AppBox, ContinueBox } from "./Continue";
import FinalFiles from "./FinalFiles";
import { Dest, destOrder } from "./handoff";
import { guessStep } from "./steps";
import { lookLine } from "./Template";
import Gate from "./Gate";
import { Chip, Ctx, Empty, PillState, StatusPill, text, radioKeys, Section } from "./ui";

const PILL: Record<Post["status"], PillState> = { draft: "approval", approved: "queued", needs_check: "approval", sent: "done", failed: "failed", cancelled: "cancelled" };
const PILL_NOTE: Record<Post["status"], string> = { draft: "A draft: it needs your approval", approved: "Approved, waiting for its time", needs_check: "A send was cut off: check the destination", sent: "Sent", failed: "The send failed", cancelled: "Cancelled" };
const when = (t: number) => new Date(t * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });

/** One click for everything: the final document, each step's result, the sources, and the code. */
function ExportBox({ ctx }: { ctx: Ctx }) {
  const [notes, setNotes] = useState("");
  const [out, setOut] = useState<Exported | null>(null);
  const [report, setReport] = useState<{ folder: string; markdown: string; pdf: string | null; notes: string[] } | null>(null);
  const [err, setErr] = useState("");
  const [log, setLog] = useState<{ path: string; text: string } | null>(null);
  const [raw, setRaw] = useState<{ folder: string } | null>(null);
  const reveal = (path: string) => api.call("reveal_export", { path }).catch((e) => setErr(text(e)));
  return (
    <section className="sheet">
      <p className="muted">One folder with the final document, every step's result in order, your sources, and any code. It is scanned for secrets first, and one secret stops the export.</p>
      <div className="actions">
        <button disabled={!!ctx.busy || ctx.s.artifacts.length === 0} onClick={() => ctx.guard(async () => {
          try { setOut(await api.call<Exported>("export_package", { runInstructions: notes })); setErr(""); } catch (e) { setOut(null); setErr(text(e)); throw e; }
        })}>Export everything</button>
        <button className="ghost" disabled={!!ctx.busy} title="A short summary: the steps, the models, the cost, what the second check said, and what is still open" onClick={() => ctx.guard(async () => {
          setReport(null);
          setReport(await api.call("export_run_report"));
        })}>Export run report · Markdown + PDF</button>
        <button className="ghost" disabled={!!ctx.busy} title="What the second check said, what you changed, and what changed in the work" onClick={() => ctx.guard(async () => {
          try { setLog(await api.call<{ path: string; text: string }>("export_decision_log")); setErr(""); } catch (e) { setLog(null); setErr(text(e)); throw e; }
        })}>Make the decision log</button>
      </div>
      <details>
        <summary>Advanced: raw data</summary>
        <p className="muted">The records behind the run report as JSON: the approved plan and its hash, every attempt, and every cost entry. Long, and for anyone who wants to audit a run.</p>
        <div className="actions tight">
          <button className="ghost" disabled={!!ctx.busy} onClick={() => ctx.guard(async () => {
            setRaw(null);
            try { setRaw(await api.call<{ folder: string }>("export_run_report_raw")); setErr(""); } catch (e) { setErr(text(e)); throw e; }
          })}>Export the raw data · Markdown</button>
        </div>
        {raw && <div className="result" role="status"><p>Raw data saved</p><p className="muted path">{raw.folder}</p><button className="ghost" onClick={() => reveal(raw.folder)}>Show in Finder</button></div>}
      </details>
      <details>
        <summary>Add a note for whoever opens it</summary>
        <label>Note
          <textarea rows={3} value={notes} onChange={(e) => setNotes(e.target.value)} />
        </label>
      </details>
      {report && <div className="result" role="status"><p>Run report saved · Markdown{report.pdf ? " + PDF" : ""}</p><p className="muted path">{report.folder}</p>{report.notes.map(n => <p key={n}>{n}</p>)}<button className="ghost" onClick={() => reveal(report.folder)}>Show run report in Finder</button></div>}
      {log && (
        <div className="result" role="status">
          <p><Chip tone="ok">Decision log made</Chip> It is also inside every export.</p>
          <p className="muted path">{log.path}</p>
          <div className="actions"><button className="ghost" onClick={() => reveal(log.path)}>Show in Finder</button></div>
          <details><summary>Read it here</summary><pre className="log-text">{log.text}</pre></details>
        </div>
      )}
      {ctx.s.artifacts.length === 0 && <p className="muted">Nothing to export until a step has produced a result.</p>}
      {err && <p className="field-error" role="alert">{err}</p>}
      {out && (
        <div className="result" role="status">
          <p><Chip tone="ok">Exported</Chip> {out.files} files</p>
          {out.final ? <p>The final document comes from the step “{out.final}”.</p> : <p className="muted">No single document is marked as final. {(ctx.s.code_files?.length ?? 0) > 0 ? "What was built is in the code folder, and each" : "Each"} step's result is in the steps folder.</p>}
          <p className="muted path">{out.path}</p>
          <div className="actions"><button className="ghost" onClick={() => reveal(out.path)}>Show in Finder</button></div>
          {out.notes?.map((n) => <p key={n} className="muted">{n}</p>)}
          {out.incomplete.length > 0 && <p><Chip tone="warn">Incomplete</Chip> {out.incomplete.join("; ")}</p>}
          {out.missing.length > 0 && <p className="muted">Nothing was added for: {out.missing.join(", ")}</p>}
          {out.broken_links.length > 0 && <p className="muted">Broken links: {out.broken_links.join("; ")}</p>}
          {out.warnings.length > 0 && <p className="muted">Private-looking details in: {out.warnings.join("; ")}</p>}
        </div>
      )}
    </section>
  );
}

function Outbox({ ctx }: { ctx: Ctx }) {
  const posts = ctx.s.outbox ?? [];
  const [body, setBody] = useState("");
  const [at, setAt] = useState("");
  const [err, setErr] = useState("");
  const steps = (ctx.s.plan?.stages ?? []).filter((st) => ctx.s.artifacts.some((a) => a.name === st.id));
  const [from, setFrom] = useState(guessStep(steps, ["queue"]));
  const [first, setFirst] = useState("");
  const [days, setDays] = useState("1");
  const [review, setReview] = useState<Post | null>(null);
  const [made, setMade] = useState<{ created: { id: string; title: string }[]; skipped: { title: string; reason: string }[] } | null>(null);
  const act = (cmd: string, args: Record<string, unknown>) =>
    ctx.guard(async () => { try { await api.call(cmd, args); setErr(""); } catch (e) { setErr(text(e)); throw e; } });
  return (
    <>
      {steps.length > 0 && (
        <div className="drawer">
          <p className="muted">Turn the posts a step wrote into drafts, one for each post, a day apart or as you choose. A draft is only a draft: you approve each one below before anything is sent.</p>
          <div className="actions tight">
            <select aria-label="The step that has the posts" value={from} onChange={(e) => setFrom(e.target.value)}>
              <option value="">Choose the step</option>
              {steps.map((st) => <option key={st.id} value={st.id}>{st.title}</option>)}
            </select>
            <input type="datetime-local" aria-label="When the first post goes" value={first} onChange={(e) => setFirst(e.target.value)} />
            <label className="inline">every <input type="number" min="0" max="365" value={days} onChange={(e) => setDays(e.target.value)} style={{ width: 64 }} /> days</label>
            <button disabled={!from || !first || !!ctx.busy} onClick={() => ctx.guard(async () => {
              try {
                setMade(await api.call("queue_to_drafts", { stage: from, start: Math.floor(new Date(first).getTime() / 1000), spacing: Math.round(Number(days) * 86400) }));
                setErr("");
              } catch (e) { setMade(null); setErr(text(e)); throw e; }
            })}>Make drafts</button>
          </div>
          {made && (
            <p className="muted" role="status">
              {made.created.length} {made.created.length === 1 ? "draft" : "drafts"} made.
              {made.skipped.length > 0 && ` Left out: ${made.skipped.map((s) => `${s.title} (${s.reason})`).join("; ")}.`}
            </p>
          )}
        </div>
      )}
      <p className="muted lead">Posts go to a stand-in destination that writes to a file in this project; no social network receives them. Each post needs your approval of its exact text and time.</p>
      <label>Text
        <textarea rows={2} maxLength={280} value={body} onChange={(e) => setBody(e.target.value)} placeholder="Up to 280 characters" />
      </label>
      <label>Send on
        <input type="datetime-local" value={at} onChange={(e) => setAt(e.target.value)} />
      </label>
      <div className="actions">
        <button disabled={!body.trim() || !at || !!ctx.busy} onClick={() => act("outbox_draft", { text: body, at: Math.floor(new Date(at).getTime() / 1000) }).then(() => setBody(""))}>Save as a draft</button>
        <button className="ghost" disabled={!!ctx.busy} onClick={() => act("outbox_send_due", {})}>Send what is approved and due</button>
      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
      {review && (
        <Gate title="Approve this post?" approveLabel="Approve post" rejectLabel="Not yet" busy={!!ctx.busy}
          what={<p>This post will be written to the stand-in file in this project on {when(review.scheduled_at)}; no social network receives it. Approve the exact text below.</p>}
          payload={review.text}
          onReject={() => setReview(null)}
          onApprove={() => { const id = review.id; setReview(null); void act("outbox_approve", { project: ctx.s.name, id }); }} />
      )}
      {posts.length === 0 && <Empty>No posts yet.</Empty>}
      {posts.map((p) => (
        <div className="post" key={p.id}>
          <div>
            <p>{p.text}</p>
            <p className="muted">{when(p.scheduled_at)}{p.receipt ? `, receipt ${p.receipt.remote_id}` : ""}</p>
          </div>
          <StatusPill state={PILL[p.status]} title={PILL_NOTE[p.status]} />
          <div className="row-actions">
            {p.status === "draft" && <button className="primary" onClick={() => setReview(p)}>Review and approve</button>}
            {(p.status === "draft" || p.status === "approved") && <button className="ghost" onClick={() => act("outbox_cancel", { id: p.id })}>Cancel post</button>}
          </div>
        </div>
      ))}
    </>
  );
}

const W_PILL: Record<WriteItem["status"], PillState> = { proposed: "approval", approved: "queued", sending: "approval", done: "done", failed: "failed", cancelled: "cancelled" };
const W_NOTE: Record<WriteItem["status"], string> = { proposed: "Waiting for your approval", approved: "Approved, ready to send", sending: "A send was cut off: check the destination", done: "Written", failed: "The send failed", cancelled: "Cancelled" };

/** One file of a GitHub proposal. Its exact text is read when the person opens it, so a big proposal stays light. */
function FileText({ id, path, bytes }: { id: string; path: string; bytes: number }) {
  const [text, setText] = useState<string | null>(null);
  const [err, setErr] = useState("");
  return (
    <details onToggle={(e) => { if ((e.target as HTMLDetailsElement).open && text === null) api.call<string>("write_file", { id, path }).then(setText).catch((x) => setErr(String(x))); }}>
      <summary>{path} <span className="muted">{bytes} bytes</span></summary>
      {err ? <p className="field-error">{err}</p> : <pre className="exact">{text ?? "Loading"}</pre>}
    </details>
  );
}

/** A send that was cut off may or may not have arrived. The person checks the destination and says which. */
function Interrupted({ ctx, id }: { ctx: Ctx; id: string }) {
  const [url, setUrl] = useState("");
  const act = (arrived: string | null) => ctx.guard(async () => { await api.call("resolve_write", { id, arrived }); });
  return (
    <div className="inline-form">
      <input aria-label="Address of what was written" placeholder="Address of what arrived" value={url} onChange={(e) => setUrl(e.target.value)} />
      <button disabled={!url.trim()} onClick={() => act(url)}>It arrived</button>
      <button onClick={() => act(null)}>It did not arrive</button>
    </div>
  );
}

/** The proposals for Notion and GitHub, each with its exact content. Nothing is sent to them until it is approved. */
function WriteList({ ctx }: { ctx: Ctx }) {
  const items = ctx.s.writes ?? [];
  const act = (f: () => Promise<unknown>) => ctx.guard(async () => { await f(); });
  const [review, setReview] = useState<WriteItem | null>(null);
  if (items.length === 0) return null;
  const waiting = items.filter((w) => w.status === "proposed" || w.status === "approved").length;
  return (
    <Section title="Waiting and sent" value={`${waiting} waiting, ${items.length - waiting} done`} flag={waiting ? `${waiting} waiting` : undefined}>
      {review && (
        <Gate title="Approve this write?" approveLabel="Approve write" rejectLabel="Not yet" busy={!!ctx.busy}
          what={review.kind === "notion"
            ? (review.append
              ? <p>This will add the text below to the end of an existing Notion page, under the heading “{review.title}”. Nothing already on the page is changed. It is sent only after you approve it, and only once.</p>
              : <p>This will create one page called “{review.title}” in Notion. It is sent only after you approve it, and only once. Approve the exact text below.</p>)
            : <p>This will put {review.files.length} files in the folder {review.folder} on a new branch, {review.branch}, in {review.repo}. The main branch is not touched. Open a file to read it before you approve.</p>}
          payload={review.kind === "notion" ? review.body : review.files.map((f) => `${f.path}  (${f.bytes} bytes)`).join("\n")}
          onReject={() => setReview(null)}
          onApprove={() => { const id = review.id; setReview(null); void act(() => api.call("approve_write", { project: ctx.s.name, id })); }}>
          {review.kind === "github" && <ul className="plain">{review.files.map((f) => <li key={f.path}><FileText id={review.id} path={f.path} bytes={f.bytes} /></li>)}</ul>}
        </Gate>
      )}
      <p className="muted">Each one is shown in full. Approve it, then send it.</p>
      {items.map((w) => (
        <div className="post" key={w.id} id={`write-${w.id}`}>
          <div>
            <p><strong>{w.kind === "notion" ? (w.append ? `Added to a Notion page: ${w.title}` : `Notion page: ${w.title}`) : `GitHub: ${w.repo}`}</strong></p>
            {w.kind === "github" && <p className="muted">Folder {w.folder} on branch {w.branch}. The main branch is not touched.</p>}
            <details>
              <summary>{w.kind === "notion" ? "Exact text" : `${w.files.length} files`}</summary>
              {w.kind === "notion" ? <pre className="exact">{w.body}</pre> : <ul className="plain">{w.files.map((f) => <li key={f.path}><FileText id={w.id} path={f.path} bytes={f.bytes} /></li>)}</ul>}
            </details>
            {w.note && <p className="field-error">{w.note}</p>}
            {w.url && <p className="muted path">{w.url}</p>}
          </div>
          <StatusPill state={W_PILL[w.status]} title={W_NOTE[w.status]} />
          <div className="row-actions">
            {w.status === "proposed" && <button className="primary" onClick={() => setReview(w)}>Review and approve</button>}
            {w.status === "approved" && <button className="primary" disabled={!!ctx.busy} onClick={() => act(() => api.call("send_write", { id: w.id }))}>Send now</button>}
            {w.status === "sending" && <Interrupted ctx={ctx} id={w.id} />}
            {w.status === "failed" && <button onClick={() => act(() => api.call("retry_write", { id: w.id }))}>Retry write</button>}
            {(w.status === "proposed" || w.status === "approved" || w.status === "failed") && <button className="ghost" onClick={() => act(() => api.call("cancel_write", { id: w.id }))}>Cancel write</button>}
          </div>
        </div>
      ))}
    </Section>
  );
}

type ClientMade = { folder: string; files: { kind: "word" | "pdf" | "deck" | "sheet" | "keynote"; path: string }[]; notes: string[] };
const KIND: Record<string, string> = { word: "Word document", pdf: "PDF", deck: "PowerPoint deck", sheet: "Excel workbook (the tables, as values)", keynote: "Keynote deck" };

/** The document and the deck for the client, made from two steps' results. */
/** One line on the template the files follow, with where to change it. */
function LookChoice({ ctx }: { ctx: Ctx }) {
  const t = ctx.s.look;
  return (
    <p className="muted look-choice">
      {t?.look ? <><span className="swatch" style={{ background: `#${t.look.accent}` }} aria-hidden="true" /> The files follow the template <strong>{t.name || t.from_name}</strong>: {lookLine(t)}.</> : "The files use SproutCrew's own look."}{" "}
      <button className="link" onClick={() => ctx.goTo("material")}>Change the template</button>
    </p>
  );
}

/** A step's result as a new Markdown file in a folder the person names, such as an Obsidian vault. Nothing is
 *  replaced: a second save gets a new name. */
function SaveToFolder({ ctx, steps }: { ctx: Ctx; steps: { id: string; title: string }[] }) {
  const [stage, setStage] = useState(steps[steps.length - 1]?.id ?? "");
  const [folder, setFolder] = useState("");
  const [out, setOut] = useState("");
  const [err, setErr] = useState("");
  return (
    <fieldset className="save-folder">
      <legend>Save a result into a folder</legend>
      <p className="muted">As a Markdown file, for example into your Obsidian vault. A file that is already there is never replaced.</p>
      <div className="grid-row conn">
        <label>Result<select value={stage} onChange={(e) => setStage(e.target.value)}>{steps.map((st) => <option key={st.id} value={st.id}>{st.title}</option>)}</select></label>
        <label>Folder<input value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="~/Documents/Vault/Projects" /></label>
      </div>
      <div className="actions tight">
        <button className="ghost" disabled={!!ctx.busy || !stage || !folder.trim()} onClick={() => ctx.guard(async () => {
          try { const p = await api.call<string>("save_result_to_folder", { stage, folder: folder.trim() }); setOut(`Saved as ${p.split("/").pop()}.`); setErr(""); } catch (e) { setOut(""); setErr(text(e)); throw e; }
        })}>Save it there</button>
      </div>
      {out && <p className="muted" role="status">{out}</p>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </fieldset>
  );
}

function ClientForm({ ctx }: { ctx: Ctx }) {
  const steps = (ctx.s.plan?.stages ?? []).filter((st) => ctx.s.artifacts.some((a) => a.name === st.id));
  const [doc, setDoc] = useState(guessStep(steps, ["finish_document", "polish_document", "polish_finish", "finish", "document", "client_doc", "recommend"]));
  const [deck, setDeck] = useState(guessStep(steps, ["finish_deck", "polish_deck", "deck", "present"]));
  const [subtitle, setSubtitle] = useState("");
  const [out, setOut] = useState<ClientMade | null>(null);
  const [err, setErr] = useState("");
  // Files wait for the details only the person can supply, here as on the result: made once none are missing, or the
  // person finished around them.
  const [held, setHeld] = useState(false);
  const results = [ctx.s.artifacts.map((a) => a.file).join(), ctx.s.notes.map((n) => n.id).join(), ctx.busy ? "busy" : "idle"].join("|");
  useEffect(() => {
    let live = true;
    api.call<{ gaps?: unknown[]; accepted?: boolean }>("final_files").then((f) => live && setHeld(!!f.gaps?.length && !f.accepted)).catch(() => live && setHeld(false));
    return () => { live = false; };
  }, [results]);
  const pick = (v: string, set: (x: string) => void, label: string, none: string) => (
    <label>{label}
      <select value={v} onChange={(e) => set(e.target.value)}>
        <option value="">{none}</option>
        {steps.map((st) => <option key={st.id} value={st.id}>{st.title}</option>)}
      </select>
    </label>
  );
  if (steps.length === 0) return <p className="empty-note">Run the steps first. The files are made from their results.</p>;
  return (
    <>
      <p className="muted lead">Makes a Word document and a PDF of it, a PowerPoint deck, and an Excel file of the document's tables, ready to hand over. A section for the team (Source warnings) is left out. The files are saved in your Documents folder, under SproutCrew and this project's name, and nothing there is replaced.</p>
      <LookChoice ctx={ctx} />
      <SaveToFolder ctx={ctx} steps={steps} />
      {pick(doc, setDoc, "Which result is the document?", "None")}
      {pick(deck, setDeck, "Which result is the deck?", "Make slides from the document")}
      <label>Subtitle on the front (optional)
        <input value={subtitle} onChange={(e) => setSubtitle(e.target.value)} />
      </label>
      {held && <p className="field-error" role="status">Some details are still missing from the draft. Fill them in on the result first, or finish with what you have.</p>}
      <div className="actions">
        <button className="primary" disabled={!!ctx.busy || held || (!doc && !deck)} onClick={() => ctx.guard(async () => {
          try { setOut(await api.call<ClientMade>("export_client_files", { documentStep: doc || null, deckStep: deck || null, subtitle })); setErr(""); } catch (e) { setOut(null); setErr(text(e)); throw e; }
        })}>Make the files</button>
      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
      {out && (
        <div className="result" role="status">
          <p className="muted path">{out.folder}</p>
          {out.files.map((f) => (
            <div className="file-line" key={f.path}>
              <span className="type-tile" aria-hidden="true">{f.kind === "word" ? "Doc" : f.kind === "pdf" ? "PDF" : f.kind === "sheet" ? "Xls" : f.kind === "keynote" ? "Key" : "Ppt"}</span>
              <span className="material-main"><strong>{KIND[f.kind]}</strong><span className="muted path">{f.path.split("/").pop()}</span></span>
              <button className="ghost" onClick={() => api.call("reveal_export", { path: f.path }).catch((e) => setErr(text(e)))}>Show in Finder</button>
            </div>
          ))}
          {out.notes.map((n) => <p key={n} className="muted">{n}</p>)}
        </div>
      )}
    </>
  );
}

function NotionForm({ ctx }: { ctx: Ctx }) {
  const { s } = ctx;
  const [parent, setParent] = useState("");
  const [append, setAppend] = useState(false);
  const [title, setTitle] = useState(`${s.name}: drafts`);
  const [file, setFile] = useState("");
  const [err, setErr] = useState("");
  const results = s.artifacts.map((a) => ({ file: a.file, title: s.plan?.stages.find((x) => x.id === a.name)?.title ?? a.name }));
  const pick = file || results[results.length - 1]?.file || "";
  const act = (f: () => Promise<unknown>) => ctx.guard(async () => { try { await f(); setErr(""); } catch (e) { setErr(text(e)); throw e; } });
  return (
    <>
      <div className="choices" role="radiogroup" aria-label="Where in Notion">
        <label className="check"><input type="radio" checked={!append} onChange={() => setAppend(false)} /> A new page under a page I choose</label>
        <label className="check"><input type="radio" checked={append} onChange={() => setAppend(true)} /> Add to the end of a page I already have</label>
      </div>
      <p className="muted lead">{append
        ? "The result is added at the bottom of the page you choose, under a heading. Nothing already on the page is changed or removed."
        : "Adds one new page under a Notion page you choose. Make a page for it first, such as SproutCrew drafts."} In Notion, open the page’s menu, choose Connections, and add your integration. The key is the Keychain item notion-write.</p>
      <label>{append ? "Page to add to" : "Page to put it under"}
        <input value={parent} onChange={(e) => setParent(e.target.value)} placeholder="Paste the address of the Notion page" />
      </label>
      <label>{append ? "Heading above the added text" : "New page title"}
        <input value={title} onChange={(e) => setTitle(e.target.value)} />
      </label>
      <label>Which result to send
        <select value={pick} onChange={(e) => setFile(e.target.value)} disabled={results.length === 0}>
          {results.length === 0 && <option value="">No results yet</option>}
          {results.map((r) => <option key={r.file} value={r.file}>{r.title}</option>)}
        </select>
      </label>
      <div className="actions">
        <button className="primary" disabled={!!ctx.busy || !parent.trim() || !pick} onClick={() => act(async () => { const body = await api.call<string>("read_artifact", { file: pick }); await api.call("propose_notion", { parent, title, body, append }); })}>{append ? "Review what will be added" : "Review the page"}</button>
      </div>
      <p className="muted">You see the exact text before anything is written.</p>
      {err && <p className="field-error" role="alert">{err}</p>}
    </>
  );
}

function GithubForm({ ctx }: { ctx: Ctx }) {
  const { s } = ctx;
  const [repo, setRepo] = useState("");
  const [folder, setFolder] = useState("");
  const [branch, setBranch] = useState("");
  const [err, setErr] = useState("");
  const act = (f: () => Promise<unknown>) => ctx.guard(async () => { try { await f(); setErr(""); } catch (e) { setErr(text(e)); throw e; } });
  return (
    <>
      <p className="muted lead">Adds the project's code to a repository as new files on a new branch. The main branch is never touched and no pull request is opened. The repository needs a first commit. The key is the Keychain item github-write, limited to that repository.</p>
      <label>Repository
        <input value={repo} onChange={(e) => setRepo(e.target.value)} placeholder="owner/name" />
      </label>
      <label>Folder for this app
        <input value={folder} onChange={(e) => setFolder(e.target.value)} placeholder={s.name} />
      </label>
      <details>
        <summary>Branch name</summary>
        <label>Branch
          <input value={branch} onChange={(e) => setBranch(e.target.value)} placeholder="bff/project-name-time" />
        </label>
      </details>
      <div className="actions">
        <button className="primary" disabled={!!ctx.busy || !repo.trim()} onClick={() => act(() => api.call("propose_github", { repo, branch, folder }))}>Review the files</button>
      </div>
      <p className="muted">You see every file before anything is written.</p>
      {err && <p className="field-error" role="alert">{err}</p>}
    </>
  );
}

const DESTS: { id: Dest; name: string; line: string; mark: string }[] = [
  { id: "app", name: "Your app", line: "Open it and see its files", mark: "App" },
  { id: "next", name: "Continue in a new project", line: "Use these results in another use case", mark: "Next" },
  { id: "client", name: "Documents & slides", line: "Word, PDF, and PowerPoint", mark: "Doc" },
  { id: "notion", name: "Notion", line: "A new page, or add to one", mark: "No" },
  { id: "github", name: "GitHub", line: "Code on a new branch", mark: "Gi" },
  { id: "posts", name: "Post drafts", line: "Drafts kept on this Mac", mark: "Post" },
];

export default function ShareTab({ ctx }: { ctx: Ctx }) {
  const order = destOrder((ctx.s.code_files?.length ?? 0) > 0);
  const [picked, setDest] = useState<Dest | null>(() => {
    try { const item = JSON.parse(sessionStorage.getItem("sproutcrew:write-decision") ?? "null"); return ctx.s.writes.find(w => w.id === item?.id)?.kind ?? null; } catch { return null; }
  });
  useEffect(() => {
    try { const item = JSON.parse(sessionStorage.getItem("sproutcrew:write-decision") ?? "null"); if (item) requestAnimationFrame(() => document.getElementById(item.anchor)?.scrollIntoView({ block: "start" })); } finally { sessionStorage.removeItem("sproutcrew:write-decision"); }
  }, []);
  const dest = picked && order.includes(picked) ? picked : order[0];
  const d = DESTS.find((x) => x.id === dest)!;
  return (
    <>
      <div className="page-sections"><div>
      <Section title={(ctx.s.result_review?.finished ?? false) ? "Final document" : "Draft"} value={ctx.s.result_review?.finished ? (ctx.s.result_review.reviewed ? "reviewed" : "not reviewed") : `${ctx.s.artifacts.length} results so far`} open>
        <FinalFiles ctx={ctx} />
      </Section>
      <Section title="Export" value={ctx.s.artifacts.length ? "everything, a run report or the decision log" : "after the first result"}>
        <ExportBox ctx={ctx} />
      </Section>
      <Section title="Send it somewhere" value={d.name}>
        <p className="muted">Results are sent only after you approve exactly what goes. Your brief and the context each step needs already go to the AI service you chose.</p>
        <div className="dests" role="radiogroup" aria-label="Destination" onKeyDown={radioKeys}>
          {order.map((id) => DESTS.find((x) => x.id === id)!).map((x) => (            <button key={x.id} role="radio" aria-checked={dest === x.id} tabIndex={dest === x.id ? 0 : -1} className={`dest${dest === x.id ? " on" : ""}`} onClick={() => setDest(x.id)}>
              <span className="type-tile" aria-hidden="true">{x.mark}</span>
              <strong>{x.name}</strong>
              <span className="muted">{x.line}</span>
            </button>
          ))}
        </div>
        <div className="dest-form" key={dest}>
        {dest === "app" && <AppBox ctx={ctx} />}
        {dest === "next" && <ContinueBox ctx={ctx} />}
        {dest === "client" && <ClientForm ctx={ctx} />}
        {dest === "notion" && <NotionForm ctx={ctx} />}
        {dest === "github" && <GithubForm ctx={ctx} />}
        {dest === "posts" && <Outbox ctx={ctx} />}
        </div>
      </Section>
      <WriteList ctx={ctx} />
      </div></div>
    </>
  );
}
