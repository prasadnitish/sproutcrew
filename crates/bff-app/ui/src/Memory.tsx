import { useEffect, useState } from "react";
import { api, Note } from "./api";
import { Confirm, Section, text } from "./ui";
import { SharedTemplates } from "./Template";

/** Memory (DESIGN_SPEC.md §10.1): what SproutCrew remembers for every project, the saved templates, and where each
 *  project's own memory is. Notes for every project are given to every step of every project on its next run, before
 *  that project's own notes, which win where they differ. */
export default function Memory({ project, onProject }: { project: string | null; onProject: () => void }) {
  const [notes, setNotes] = useState<Note[] | null>(null);
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [asking, setAsking] = useState<Note | null>(null);
  const load = () => api.call<Note[]>("memory_list").then((n) => { setNotes(n); setErr(""); }).catch((e) => setErr(text(e)));
  useEffect(() => { void load(); }, []);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try { await f(); await load(); } catch (e) { setErr(text(e)); } finally { setBusy(false); }
  };
  const save = () => act(async () => { await api.call("memory_save", { text: draft, replaceId: editing || null }); setDraft(""); setEditing(""); });
  return (
    <div className="page-body">
      <div className="page-sections"><div>
        <Section title="For every project" value={notes ? `${notes.length} ${notes.length === 1 ? "note" : "notes"}` : "reading"} open>
          <p className="muted">Your own notes for every crew: who you write for, your voice, words to avoid. They are your instructions, not checked facts.</p>
          {notes && notes.length === 0 && <p className="empty-note">Nothing yet. Add what every crew should keep in mind.</p>}
          {notes && notes.length > 0 && (
            <ul className="notes">{notes.map((n) => <li key={n.id}>
              <span className="note-text">{n.text}</span>
              <button className="ghost" disabled={busy} onClick={() => { setDraft(n.text); setEditing(n.id); }}>Edit</button>
              <button className="ghost" disabled={busy} aria-label={`Remove: ${n.text.slice(0, 40)}`} onClick={() => setAsking(n)}>Remove</button>
            </li>)}</ul>
          )}
          <label>{editing ? "Change this note" : "What should every crew remember?"}<textarea rows={3} maxLength={2000} value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="I write for small-business owners. Use plain English and US spelling." /></label>
          <div className="actions"><button disabled={busy || !draft.trim()} onClick={() => void save()}>{editing ? "Save changes" : "Remember this"}</button>{editing && <button className="ghost" onClick={() => { setDraft(""); setEditing(""); }}>Cancel</button>}</div>
          {err && <p className="field-error" role="alert">{err}</p>}
          <p className="muted">Never add passwords or API keys: notes go to the AI with each step. Removing a note stops it going to future steps; earlier results and their records keep what they were given.</p>
        </Section>
        <SharedTemplates />
        <Section title="By project" value={project ? `${project} is open` : "in each project"}>
          <p className="muted">Each project also keeps its own rules and step notes, under Sources, Instructions.{project ? "" : " Open a project to see them."}</p>
          {project && <button onClick={onProject}>Open {project}'s instructions</button>}
        </Section>
      </div></div>
      {asking && <Confirm title="Remove this note?" line="Future steps in every project stop getting it. Earlier results keep what they were given." action="Remove"
        onKeep={() => setAsking(null)} onConfirm={() => { const id = asking.id; setAsking(null); void act(async () => { await api.call("memory_remove", { id }); if (editing === id) { setDraft(""); setEditing(""); } }); }} />}
    </div>
  );
}
