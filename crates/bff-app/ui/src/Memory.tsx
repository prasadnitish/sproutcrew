import { useEffect, useState } from "react";
import { api, Note } from "./api";
import { SectionIntro, text } from "./ui";
import { SharedTemplates } from "./Template";

/** What SproutCrew remembers for every project: the person's own notes, given to every step of every project on
 *  its next run, before that project's own notes, which win where they differ. Each can be read, changed or
 *  removed here. */
export default function Memory({ project, onProject }: { project: string | null; onProject: () => void }) {
  const [notes, setNotes] = useState<Note[] | null>(null);
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const load = () => api.call<Note[]>("memory_list").then((n) => { setNotes(n); setErr(""); }).catch((e) => setErr(text(e)));
  useEffect(() => { void load(); }, []);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try { await f(); await load(); } catch (e) { setErr(text(e)); } finally { setBusy(false); }
  };
  const save = () => act(async () => { await api.call("memory_save", { text: draft, replaceId: editing || null }); setDraft(""); setEditing(""); });
  return (
    <>
      <SectionIntro title="Remembered in every project">Your own notes for every crew: who you write for, your voice, words to avoid. Each step in each project is given them on its next run, before that project's own notes, which win where the two differ. They are your instructions, not checked facts.</SectionIntro>
      <section className="sheet" aria-label="Notes for every project">
        {notes === null && !err && <p className="muted">Loading.</p>}
        {notes && notes.length === 0 && <p className="empty-note">Nothing yet. Add what every crew should keep in mind.</p>}
        {notes && notes.length > 0 && (
          <ul className="notes">{notes.map((n) => <li key={n.id}>
            <span className="note-text">{n.text}</span>
            <button className="ghost" disabled={busy} onClick={() => { setDraft(n.text); setEditing(n.id); }}>Edit</button>
            <button className="ghost" disabled={busy} aria-label={`Remove: ${n.text.slice(0, 40)}`} onClick={() => act(async () => { await api.call("memory_remove", { id: n.id }); if (editing === n.id) { setDraft(""); setEditing(""); } })}>Remove</button>
          </li>)}</ul>
        )}
        <label>{editing ? "Change this note" : "What should every crew remember?"}<textarea rows={3} maxLength={2000} value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="I write for small-business owners. Use plain English and US spelling." /></label>
        <div className="actions"><button disabled={busy || !draft.trim()} onClick={() => void save()}>{editing ? "Save changes" : "Remember this"}</button>{editing && <button className="ghost" onClick={() => { setDraft(""); setEditing(""); }}>Cancel</button>}</div>
        {err && <p className="field-error" role="alert">{err}</p>}
        <p className="muted">Never add passwords or API keys: notes go to the AI with each step. Removing a note stops it going to future steps; earlier results and their records keep what they were given.</p>
      </section>
      <SharedTemplates />
      <section className="sheet">
        <h2>Remembered in one project</h2>
        <p className="muted">Each project also keeps its own memory and step notes, under Sources.{project ? "" : " Open a project to see its memory."}</p>
        {project && <button className="ghost" onClick={onProject}>Open {project}'s memory</button>}
      </section>
    </>
  );
}
