import { useEffect, useState } from "react";
import { api, Tools } from "./api";
import { Chip, Ctx, Empty, roleName, text } from "./ui";

/** A Notion page, by its link, kept as readable text named after the page. */
function NotionPage({ ctx }: { ctx: Ctx }) {
  const [link, setLink] = useState("");
  const [out, setOut] = useState("");
  const [err, setErr] = useState("");
  return (
    <div className="conn-box">
      <label>Notion page link<input value={link} onChange={(e) => setLink(e.target.value)} placeholder="https://www.notion.so/Your-page-..." /></label>
      <div className="actions">
        <button disabled={!!ctx.busy || !link.trim()} onClick={() => ctx.guard(async () => {
          try {
            const r = await api.call<{ title: string; chars: number; notes: string[] }>("read_notion_page", { link: link.trim() });
            setOut(`Added "${r.title}" (${r.chars.toLocaleString("en-US")} characters).${r.notes.length ? ` ${r.notes.join(". ")}.` : ""}`); setErr(""); setLink("");
          } catch (e) { setOut(""); setErr(text(e)); throw e; }
        })}>Add the page</button>
      </div>
      <p className="muted small">Needs a Notion integration token with only the Read content capability, kept in this Mac's Keychain and declared read-only, and the page shared with that integration. The app cannot add the token yet. Text only: pictures and databases are left out.</p>
      {out && <p role="status"><Chip tone="ok">Added</Chip> {out}</p>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </div>
  );
}

/** A Google Doc, by its link, read view-only with the Google sign-in made in Settings. */
function GoogleDoc({ ctx }: { ctx: Ctx }) {
  const [link, setLink] = useState("");
  const [out, setOut] = useState("");
  const [err, setErr] = useState("");
  const [st, setSt] = useState<{ available: boolean; connected: boolean } | null>(null);
  useEffect(() => { api.call<{ available: boolean; connected: boolean }>("google_docs_status").then(setSt).catch(() => setSt(null)); }, []);
  if (st && !st.available) return <p className="muted">Google Docs is not available in this version yet.</p>;
  return (
    <div className="conn-box">
      <label>Google Doc link<input value={link} onChange={(e) => setLink(e.target.value)} placeholder="https://docs.google.com/document/d/..." /></label>
      <div className="actions">
        <button disabled={!!ctx.busy || !link.trim() || !st?.connected} onClick={() => ctx.guard(async () => {
          try {
            const r = await api.call<{ title: string; chars: number; notes: string[] }>("read_google_doc", { link: link.trim() });
            setOut(`Added "${r.title}" (${r.chars.toLocaleString("en-US")} characters).${r.notes.length ? ` ${r.notes.join(" ")}` : ""}`); setErr(""); setLink("");
          } catch (e) { setOut(""); setErr(text(e)); throw e; }
        })}>Add the document</button>
      </div>
      <p className="muted small">{st?.connected ? "Reads the document with your Google sign-in, view-only. SproutCrew never edits it. Text and tables only: pictures are left out." : "Connect Google Docs in Settings first. You sign in to Google in your browser, and SproutCrew can only view documents."}</p>
      {out && <p role="status"><Chip tone="ok">Added</Chip> {out}</p>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </div>
  );
}

export function Connectors({ ctx }: { ctx: Ctx }) {
  const [kind, setKind] = useState<"notion" | "google" | "other">("notion");
  return (
    <>
      <div className="actions tight" role="radiogroup" aria-label="What to read">
        <label className="check"><input type="radio" checked={kind === "notion"} onChange={() => setKind("notion")} /> A Notion page</label>
        <label className="check"><input type="radio" checked={kind === "google"} onChange={() => setKind("google")} /> A Google Doc</label>
        <label className="check"><input type="radio" checked={kind === "other"} onChange={() => setKind("other")} /> A GitHub file, or Notion by its API path</label>
      </div>
      {kind === "notion" ? <NotionPage ctx={ctx} /> : kind === "google" ? <GoogleDoc ctx={ctx} /> : <ApiPath ctx={ctx} />}
    </>
  );
}

function ApiPath({ ctx }: { ctx: Ctx }) {
  const [id, setId] = useState("github");
  const [path, setPath] = useState("/repos/owner/name/readme");
  const [out, setOut] = useState("");
  const [err, setErr] = useState("");
  return (
    <div className="conn-box">
      <p className="muted">Reads one page or file and keeps it as a file in this project. It is read-only: This read-only connection cannot change anything on GitHub or Notion. Paths look like /repos/owner/name/readme for GitHub.</p>
      <div className="grid-row conn">
        <label>Service
          <select value={id} onChange={(e) => setId(e.target.value)}><option value="github">GitHub</option><option value="notion">Notion</option></select>
        </label>
        <label>Path<input value={path} onChange={(e) => setPath(e.target.value)} /></label>
      </div>
      <div className="actions">
        <button disabled={!!ctx.busy || !path.startsWith("/")} onClick={() => ctx.guard(async () => {
          try { const r = await api.call<{ source: string; chars: number }>("connector_read", { id, path }); setOut(`Kept as ${r.source}, ${r.chars} characters. It is in the file list now.`); setErr(""); } catch (e) { setOut(""); setErr(text(e)); throw e; }
        })}>Read it and keep it as a file</button>
        <button className="ghost" onClick={() => api.call("unblock_connector", { id }).then(() => setErr(""))}>Unblock {id === "github" ? "GitHub" : "Notion"}</button>
      </div>
      {out && <p role="status"><Chip tone="ok">Saved</Chip> {out}</p>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </div>
  );
}


export function ToolsPanel({ ctx }: { ctx: Ctx }) {
  const [t, setT] = useState<Tools | null>(null);
  const load = () => api.call<Tools>("discover_tools").then(setT).catch(() => setT(null));
  useEffect(() => { void load(); }, []);
  if (!t) return <Empty>Not read yet.</Empty>;
  const chosenSkill = new Set(t.selected.skills.map((s) => s.name));
  const flip = (kind: "skill" | "mcp", name: string, on: boolean) =>
    ctx.guard(async () => { await api.call(on ? "deselect_tool" : "select_tool", on ? { name } : { kind, name }); await load(); });
  const rec = t.recommended ?? [];
  const web = !!ctx.s.web_research;
  const hasPlan = !!ctx.s.plan;
  return (
    <div>
      {t.drift.length > 0 && <p><Chip tone="warn">Changed</Chip> {t.drift.join("; ")}</p>}
      <h3>Recommended for your crew</h3>
      <p className="muted">A skill is written guidance. It goes to the steps it suits, as the text saved when you chose it, and gives no access to anything.</p>
      {rec.length === 0 && <Empty>Nothing is recommended for this crew yet.</Empty>}
      {rec.map((r) => (
        <div className="tool" key={r.skill}>
          <div><strong>{r.label ?? r.skill}</strong><p className="muted">{r.why} For {r.roles.map(roleName).join(", ")}.</p></div>
          <button className={chosenSkill.has(r.skill) ? "" : "ghost"} aria-pressed={chosenSkill.has(r.skill)} onClick={() => flip("skill", r.skill, chosenSkill.has(r.skill))}>{chosenSkill.has(r.skill) ? "In use" : "Use it"}</button>
        </div>
      ))}
      <h3>Web research</h3>
      <div className="tool">
        <div><strong>Web research step</strong><p className="muted">{WEB_HELP}</p></div>
        <button className={web ? "" : "ghost"} aria-pressed={web} disabled={!hasPlan || !!ctx.busy} onClick={() => ctx.guard(() => api.call("set_web_research", { on: !web }))}>{web ? "In the plan" : "Add it"}</button>
      </div>
      {ctx.s.can_humanise && (
        <>
          <h3>Polish the finished text</h3>
          <div className="tool">
            <div><strong>Remove the signs of machine writing</strong><p className="muted">{POLISH_HELP}</p></div>
            <button className={ctx.s.humaniser ? "" : "ghost"} aria-pressed={!!ctx.s.humaniser} disabled={!!ctx.busy} onClick={() => ctx.guard(() => api.call("set_humaniser", { on: !ctx.s.humaniser }))}>{ctx.s.humaniser ? "In the plan" : "Add it"}</button>
          </div>
        </>
      )}
      <details className="all-tools">
        <summary>Everything found on this Mac</summary>
        <p className="muted">Found without changing anything. A skill you choose here goes to the writing steps. MCP servers stay off for every step, because a step that reads your files should not also hold your accounts.</p>
        <h3>Skills</h3>
        {t.skills.length === 0 && <Empty>No skills found.</Empty>}
        {t.skills.map((s) => (
          <div className="tool" key={s.name + s.source}>
            <div><strong>{s.name}</strong><p className="muted">{s.description || "No description"} ({s.source})</p></div>
            <button className={chosenSkill.has(s.name) ? "" : "ghost"} aria-pressed={chosenSkill.has(s.name)} onClick={() => flip("skill", s.name, chosenSkill.has(s.name))}>{chosenSkill.has(s.name) ? "In use" : "Use it"}</button>
          </div>
        ))}
        <h3>MCP servers</h3>
        {t.mcp.length === 0 && <Empty>No servers found.</Empty>}
        {t.mcp.map((m) => (
          <div className="tool" key={m.name + m.source}>
            <div><strong>{m.name}</strong><p className="muted">{m.source}, {m.transport}. Names only: addresses, arguments, and keys are not read.</p></div>
          </div>
        ))}
      </details>
    </div>
  );
}

/** What the web research step does and costs, said the same way everywhere it is offered. */
/** What the humaniser step does, in the words shown wherever it can be turned on. */
export const POLISH_HELP = "One more step at the end rewrites the finished text so it reads as written by a careful person: plainer words, no filler. It changes wording only. If it changes a number, a heading, or the length by much, the app keeps the unpolished text. It adds one request.";

export const WEB_HELP = "A first step searches the public web using only your brief, never your files, and the pages it cites are kept as sources for the rest of the crew. It needs an OpenRouter, Anthropic or OpenAI key. On OpenRouter each search costs about $0.007 plus the model's tokens, even with a free model, so it runs on a paid one.";
