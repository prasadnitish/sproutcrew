import { useEffect, useState } from "react";
import { api, TemplateSaved } from "./api";
import { Confirm, Ctx, Section, text, useConfirm } from "./ui";

type Listed = { id: string; template: TemplateSaved };

/** One line on a look: fonts, size, page, margins, colour and logo. */
export function lookLine(t: TemplateSaved): string {
  const l = t.look;
  if (!l) return "SproutCrew's own look";
  const parts = [l.heading_font ? `${l.heading_font} headings, ${l.font} text` : l.font];
  if (l.body_pt) parts.push(`${l.body_pt} pt`);
  if (l.page_mm) parts.push(l.page_mm[0] === 216 && l.page_mm[1] === 279 ? "US Letter" : l.page_mm[0] === 210 && l.page_mm[1] === 297 ? "A4" : `${l.page_mm[0]} by ${l.page_mm[1]} mm`);
  if (l.margins_mm) parts.push(`margins ${l.margins_mm[0]} mm`);
  if (t.has_logo) parts.push("logo");
  return parts.join(", ");
}

/** The project's template: what the person kept from one of their files, a starter, or a saved template. Each part is
 *  shown and can be changed: the look (for the client files), the sections (the steps use them as headings, and
 *  the second check looks for them) and the writing style. */
export default function TemplatePanel({ ctx }: { ctx: Ctx }) {
  const t = ctx.s.look ?? null;
  const [ask, asking] = useConfirm();
  const [lists, setLists] = useState<{ starters: Listed[]; shared: Listed[] }>({ starters: [], shared: [] });
  const [from, setFrom] = useState("");
  const [preview, setPreview] = useState<TemplateSaved | null>(null);
  const [keep, setKeep] = useState({ look: true, outline: true, style: true });
  const [edit, setEdit] = useState<{ name: string; outline: string; style: string } | null>(null);
  const [err, setErr] = useState("");
  const [note, setNote] = useState("");
  const files = ctx.s.sources.filter((x) => /\.(docx|pptx|pdf|md|txt)$/i.test(x.name));
  const load = () => api.call<{ starters: Listed[]; shared: Listed[] }>("templates").then(setLists).catch(() => undefined);
  useEffect(() => { void load(); }, []);
  const act = (f: () => Promise<unknown>, done = "") => ctx.guard(async () => {
    try { await f(); setErr(""); setNote(done); await load(); } catch (e) { setErr(text(e)); setNote(""); throw e; }
  });
  const look = () => {
    setPreview(null);
    if (!from) return;
    api.call<TemplateSaved>("template_preview", { sourceId: from }).then((p) => { setPreview(p); setKeep({ look: !!p.look, outline: p.outline.length > 0, style: !!p.style }); setErr(""); }).catch((e) => setErr(text(e)));
  };
  return (
    <section className="sheet template-panel" aria-label="Template">
      {asking}
      <header className="sheet-head"><div><h2>Template</h2><p className="muted">Make the work look and read like your organisation's: the look of the files, the sections, and the writing style. Each part is yours to change. The steps are told the sections and the style on their next run; the plan's approval is not affected.</p></div></header>
      {t ? (
        <div className="template-now">
          <p><strong>{t.name || t.from_name}</strong> <span className="muted">from {t.from_id === "starter" ? "a starter" : t.from_name}</span></p>
          <dl className="template-parts">
            <dt>Look of the files</dt>
            <dd>{t.look && <span className="swatch" style={{ background: `#${t.look.accent}` }} aria-hidden="true" />} {lookLine(t)}</dd>
            <dt>Sections</dt>
            <dd>{t.outline.length ? t.outline.join(" · ") : <span className="muted">Left to the steps</span>}</dd>
            <dt>Writing style</dt>
            <dd>{t.style || <span className="muted">None</span>}</dd>
          </dl>
          {edit ? (
            <div className="template-edit">
              <label>Name<input value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} /></label>
              <label>Sections, one per line<textarea rows={6} value={edit.outline} onChange={(e) => setEdit({ ...edit, outline: e.target.value })} /></label>
              <label>Writing style<textarea rows={3} maxLength={2000} value={edit.style} onChange={(e) => setEdit({ ...edit, style: e.target.value })} /></label>
              <div className="actions tight">
                <button disabled={!!ctx.busy} onClick={() => act(async () => { await api.call("edit_template", { name: edit.name, outline: edit.outline.split("\n"), style: edit.style, keepLook: !!t.look }); setEdit(null); }, "Saved.")}>Save changes</button>
                <button className="ghost" onClick={() => setEdit(null)}>Cancel</button>
              </div>
            </div>
          ) : (
            <div className="actions tight">
              <button className="ghost" onClick={() => setEdit({ name: t.name, outline: t.outline.join("\n"), style: t.style })}>Edit sections and style</button>
              {t.look && <button className="ghost" disabled={!!ctx.busy} onClick={() => act(() => api.call("edit_template", { name: t.name, outline: t.outline, style: t.style, keepLook: false }), "The files use SproutCrew's look again.")}>Use SproutCrew's look</button>}
              <button className="ghost" disabled={!!ctx.busy} onClick={() => act(() => api.call("share_template"), "Saved for all your projects. Find it under Memory.")}>Save for all projects</button>
              <button className="ghost" disabled={!!ctx.busy} onClick={() => ask({ title: `Remove ${t.name}?`, line: "This project's files go back to SproutCrew's look and sections. A copy saved for every project stays in Memory.", action: "Remove", run: () => void act(() => api.call("clear_template_look"), "No template now.") })}>Remove</button>
            </div>
          )}
        </div>
      ) : (
        <p className="empty-note">No template. The files use SproutCrew's look, and the steps choose their own sections.</p>
      )}

      <h3>Take one from your file</h3>
      {files.length === 0 ? <p className="muted">Add a Word, PowerPoint or PDF file in Your sources below first. A Word or PowerPoint file gives the look and logo too; a PDF gives the sections and style only.</p> : (
        <>
          <div className="actions tight">
            <select aria-label="File to use as a template" value={from} onChange={(e) => { setFrom(e.target.value); setPreview(null); }}>
              <option value="">Choose a file</option>
              {files.map((f) => <option key={f.id} value={f.id}>{f.title ?? f.name}</option>)}
            </select>
            <button className="ghost" disabled={!from} onClick={look}>See what it offers</button>
          </div>
          {preview && (
            <fieldset className="template-keep">
              <legend>What to keep</legend>
              <label className="check"><input type="checkbox" disabled={!preview.look} checked={keep.look && !!preview.look} onChange={(e) => setKeep({ ...keep, look: e.target.checked })} /> <span><strong>Look</strong> <span className="muted">{preview.look ? lookLine(preview) : "Not in this file"}</span></span></label>
              <label className="check"><input type="checkbox" disabled={!preview.outline.length} checked={keep.outline && preview.outline.length > 0} onChange={(e) => setKeep({ ...keep, outline: e.target.checked })} /> <span><strong>Sections</strong> <span className="muted">{preview.outline.length ? preview.outline.join(" · ") : "None found"}</span></span></label>
              <label className="check"><input type="checkbox" disabled={!preview.style} checked={keep.style && !!preview.style} onChange={(e) => setKeep({ ...keep, style: e.target.checked })} /> <span><strong>Writing style</strong> <span className="muted">{preview.style || "Too little text"}</span></span></label>
              <p className="muted small">Sections and style are worked out from the file and may need a tidy; you can edit them after.</p>
              <button disabled={!!ctx.busy || !(keep.look || keep.outline || keep.style)} onClick={() => act(async () => { await api.call("use_template", { sourceId: from, look: keep.look, outline: keep.outline, style: keep.style }); setPreview(null); setFrom(""); }, "Kept.")}>Keep these</button>
            </fieldset>
          )}
        </>
      )}

      <h3>Or start from one</h3>
      <div className="actions tight">
        {lists.starters.map((s) => <button key={s.id} className="ghost" disabled={!!ctx.busy} onClick={() => act(() => api.call("use_starter", { id: s.id }), `Using the ${s.template.name} starter.`)}>{s.template.name}</button>)}
        {lists.shared.map((s) => <button key={s.id} className="ghost" disabled={!!ctx.busy} onClick={() => act(() => api.call("use_shared_template", { id: s.id }), `Using ${s.template.name}.`)}>{s.template.name} <span className="meta">saved</span></button>)}
      </div>
      {note && <p className="muted" role="status">{note}</p>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </section>
  );
}

/** The templates saved for every project, each with its parts and a way to delete it. */
export function SharedTemplates() {
  const [list, setList] = useState<Listed[] | null>(null);
  const [err, setErr] = useState("");
  const [asking, setAsking] = useState<Listed | null>(null);
  const load = () => api.call<{ shared: Listed[] }>("templates").then((x) => setList(x.shared)).catch((e) => setErr(text(e)));
  useEffect(() => { void load(); }, []);
  return (
    <Section title="Templates" value={list ? `${list.length} saved` : "reading"}>
      <p className="muted">Saved from a project with "Save for every project". Use one from any project under Sources, Template.</p>
      {list && list.length === 0 && <p className="empty-note">None saved yet.</p>}
      {list && list.length > 0 && (
        <ul className="notes">{list.map((s) => (
          <li key={s.id}>
            <span className="note-text"><strong>{s.template.name}</strong> <span className="muted">{lookLine(s.template)}; {s.template.outline.length} sections{s.template.style ? "; a style note" : ""}</span></span>
            <span />
            <button className="ghost" onClick={() => setAsking(s)}>Delete</button>
          </li>
        ))}</ul>
      )}
      {err && <p className="field-error" role="alert">{err}</p>}
      {asking && <Confirm title={`Delete ${asking.template.name}?`} line="It leaves the list for every project. Projects that already use it keep their copy." action="Delete"
        onKeep={() => setAsking(null)} onConfirm={() => { const id = asking.id; setAsking(null); api.call("delete_shared_template", { id }).then(load).catch((e) => setErr(text(e))); }} />}
    </Section>
  );
}
