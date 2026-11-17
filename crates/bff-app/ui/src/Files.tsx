import { ReactNode, useEffect, useRef, useState } from "react";
import { Added, ProjectResult, Source, SourceRole } from "./api";
import { api } from "./api";
import { Chip, Tabs } from "./ui";

export const FILE_TYPES = ".txt,.md,.csv,.json,.pdf,.doc,.docx,.xls,.xlsx,.ppt,.pptx,.html,.htm,.png,.jpg,.jpeg,.tif,.tiff,.heic,.gif,.bmp,.webp";

/** The ways to add material, one at a time: files, a web address, another project's results, and optionally a service. It hands back what the person gave and does nothing with it. */
export type MaterialMode = "files" | "web" | "project" | "service";

export function AddMaterial({ onPaths, onFiles, onUrls, onPick, current, disabled, service, mode: held, onMode }: {
  onPaths: (p: string[]) => void; onFiles: (f: File[]) => void; onUrls: (u: string[]) => void;
  onPick: (from: string, stages: string[]) => void; current: string; disabled?: boolean; service?: ReactNode;
  /** Given by a parent that keeps the chosen tab when this is unmounted, as the new project steps do. */
  mode?: MaterialMode; onMode?: (m: MaterialMode) => void;
}) {
  const [own, setOwn] = useState<MaterialMode>("files");
  const mode = held ?? own;
  const setMode = onMode ?? setOwn;
  const modes: [typeof mode, string][] = [["files", "Files"], ["web", "Web address"], ["project", "Another project"], ...(service ? ([["service", "GitHub or Notion"]] as [typeof mode, string][]) : [])];
  return (
    <div className="add-material">
      <Tabs label="Where the sources come from" panelId="source-mode" value={mode} onChange={setMode} items={modes.map(([id, label]) => ({id, label}))} />
      <div className="mode-body" id="source-mode" role="tabpanel" aria-labelledby={`source-mode-${mode}`} key={mode} tabIndex={0}>
        {mode === "files" && <DropZone onPaths={onPaths} onFiles={onFiles} disabled={disabled} />}
        {mode === "web" && <WebBox onUrls={onUrls} disabled={disabled} />}
        {mode === "project" && <FromProject current={current} onPick={onPick} disabled={disabled} />}
        {mode === "service" && service}
      </div>
    </div>
  );
}

function DropZone({ onPaths, onFiles, disabled }: { onPaths: (p: string[]) => void; onFiles: (f: File[]) => void; disabled?: boolean }) {
  const [over, setOver] = useState(false);
  const [path, setPath] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const cb = useRef({ onPaths, onFiles });
  cb.current = { onPaths, onFiles };
  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    api.onDrop((paths, o) => {
      setOver(o);
      if (paths.length) cb.current.onPaths(paths);
    }).then((f) => (gone ? f() : (off = f)));
    return () => { gone = true; off?.(); };
  }, []);
  const addPath = () => { if (path.trim()) { onPaths([path.trim()]); setPath(""); } };
  return (
    <>
      <div
        className={`drop${over ? " over" : ""}`}
        onDragOver={(e) => { e.preventDefault(); setOver(true); }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => { e.preventDefault(); setOver(false); const f = [...e.dataTransfer.files]; if (f.length) onFiles(f); }}
      >
        <p><strong>Drop files or a folder here</strong></p>
        <p className="muted">PDF, Word, Excel, PowerPoint (old and new), pictures, text, csv, json, and html, up to 25 MB each. Pictures and scanned pages are read by character recognition, which can take a minute. A folder brings its subfolders too, such as an Obsidian vault, up to 200 files; hidden folders are skipped.</p>
        <button type="button" disabled={disabled} onClick={() => input.current?.click()}>Choose files</button>
        <input ref={input} type="file" multiple accept={FILE_TYPES} hidden onChange={(e) => { const f = [...(e.target.files ?? [])]; e.target.value = ""; if (f.length) onFiles(f); }} />
      </div>
      <details className="file-path-details"><summary>Add a file or folder by its location</summary><div className="path-row">
        <label>Or paste a path to a file or a folder
          <input value={path} disabled={disabled} onChange={(e) => setPath(e.target.value)} onKeyDown={(e) => e.key === "Enter" && addPath()} placeholder="~/Documents/claims.pdf" />
        </label>
        <button type="button" disabled={disabled || !path.trim()} onClick={addPath}>Add path</button>
      </div></details>
    </>
  );
}

function WebBox({ onUrls, disabled }: { onUrls: (u: string[]) => void; disabled?: boolean }) {
  const [url, setUrl] = useState("");
  const add = () => { if (url.trim()) { onUrls([url.trim()]); setUrl(""); } };
  return (
    <>
      <div className="path-row first">
        <label>Web address
          <input value={url} disabled={disabled} onChange={(e) => setUrl(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} placeholder="https://docs.google.com/document/d/..." autoFocus />
        </label>
        <button type="button" disabled={disabled || !url.trim()} onClick={add}>Add address</button>
      </div>
      <p className="muted">A web page, or a Google Doc, Sheet, or Slides shared with anyone who has the link. Hidden text on a page is left out.</p>
    </>
  );
}

const kindOf = (f: Source) => (f.origin.startsWith("project ") ? "Project" : f.origin ? "Web" : (f.name.split(".").pop() ?? "").slice(0, 4).toUpperCase());

/** The material a project has, one row each. */
const ROLE_WORD: Record<SourceRole, string> = { evidence: "Evidence to read and cite", requirements: "Instructions for the task", template: "Template to fill" };

export function SourceList({ sources, locked, busy, onRemove, onRole }: { sources: Source[]; locked: boolean; busy: boolean; onRemove: (id: string) => void; onRole?: (id: string, role: SourceRole) => void }) {
  if (sources.length === 0) return <p className="empty-note">Nothing added yet. Add what the team should read.</p>;
  return (
    <ul className="material-list">
      {sources.map((f) => (
        <li key={f.id} className="material-row">
          <span className="type-tile" aria-hidden="true">{kindOf(f)}</span>
          <span className="material-main">
            <strong>{f.title ?? f.name}</strong>
            <span className="muted">{f.id} · {f.bytes < 1024 * 1024 ? `${Math.max(1, Math.round(f.bytes / 1024))} KB` : `${(f.bytes / 1024 / 1024).toFixed(1)} MB`}{f.pages > 1 ? ` · ${f.pages} pages` : ""}{f.origin ? ` · ${f.origin}` : ""}</span>
            <span className="material-chips">
              {f.flags.map((x) => <Chip key={x} tone="warn">{x}</Chip>)}
              {f.suspicious.length > 0 && <Chip tone="bad">Talks to the AI: {f.suspicious.join("; ")}</Chip>}
            </span>
            {onRole && (
              <label className="role-pick">Use as
                <select value={f.role ?? "evidence"} disabled={busy} onChange={(e) => onRole(f.id, e.target.value as SourceRole)}>
                  {(Object.keys(ROLE_WORD) as SourceRole[]).map((r) => <option key={r} value={r}>{ROLE_WORD[r]}</option>)}
                </select>
                {(f.role ?? "evidence") !== "evidence" && !f.role_chosen && <span className="meta">Chosen from its name. Change it if that is wrong.</span>}
                {(f.role ?? "evidence") === "evidence" && f.suspicious.length > 0 && <span className="meta">It tries to steer the AI, so it is only evidence unless you say otherwise.</span>}
              </label>
            )}
          </span>
          <button className="ghost" disabled={locked || busy} title={locked ? "A step has already used the material" : undefined} onClick={() => onRemove(f.id)}>Remove</button>
        </li>
      ))}
    </ul>
  );
}

/** What happened to the files just given: what was kept, and in plain words why anything was not. */
export function AddedNote({ r }: { r: Added }) {
  return (
    <>
      {r.added.length > 0 && <p role="status"><Chip tone="ok">Added</Chip> {r.added.map((s) => s.title ?? s.name).join(", ")}</p>}
      <InjectionNotice sources={r.added} />
      {r.skipped.map((s) => <p key={s.name + s.reason} className="field-error" role="alert">{s.reason}</p>)}
    </>
  );
}

/** Files whose text tries to instruct an AI. Shown before a run so the person knows what the result was read against. */
export function InjectionNotice({ sources }: { sources: Source[] }) {
  const bad = sources.filter((f) => f.suspicious.length > 0);
  if (bad.length === 0) return null;
  return (
    <div className="notice warn" role="alert">
      <p><strong>{bad.length === 1 ? "1 file contains" : `${bad.length} files contain`} text that tries to instruct the AI.</strong></p>
      <ul>{bad.map((f) => <li key={f.id}>{f.title ?? f.name}: {f.suspicious.join("; ")}</li>)}</ul>
      <p>SproutCrew treats these files as evidence only and tells every step to ignore what they ask. A step that finds such text lists it under Source warnings, and the reviewer on the other provider checks whether any step followed it. Read the result with that in mind, or remove the file.</p>
    </div>
  );
}

/** Starts from the results of another project: pick a project, tick the steps whose results this one should read. */
export function FromProject({ current, onPick, disabled }: { current: string; onPick: (from: string, stages: string[]) => void; disabled?: boolean }) {
  const [names, setNames] = useState<string[]>([]);
  const [from, setFrom] = useState("");
  const [rows, setRows] = useState<ProjectResult[]>([]);
  const [on, setOn] = useState<string[]>([]);
  const [err, setErr] = useState("");
  useEffect(() => { api.call<string[]>("list_projects").then((n) => setNames(n.filter((x) => x !== current))).catch(() => setNames([])); }, [current]);
  const choose = async (name: string) => {
    setFrom(name);
    setRows([]);
    setOn([]);
    setErr("");
    if (!name) return;
    try {
      const r = await api.call<ProjectResult[]>("project_results", { name });
      setRows(r);
      setOn(r.map((x) => x.stage));
      if (r.length === 0) setErr("That project has no finished steps yet.");
    } catch (e) { setErr(String(e)); }
  };
  if (names.length === 0) return <p className="empty-note">There are no other projects yet. Finish one, and its results can feed the next.</p>;
  return (
    <div className="from-project">
      <p className="muted">Chain the work: a research brief into a case study, or a case study into an app. The results are added like files, marked as written by AI models, and checked the same way.</p>
      <label>Project
        <select value={from} disabled={disabled} onChange={(e) => void choose(e.target.value)}>
          <option value="">Choose a project</option>
          {names.map((n) => <option key={n} value={n}>{n}</option>)}
        </select>
      </label>
      {rows.length > 0 && (
        <ul className="plain">
          {rows.map((r) => (
            <li key={r.stage}>
              <label className="check"><input type="checkbox" checked={on.includes(r.stage)} onChange={(e) => setOn(e.target.checked ? [...on, r.stage] : on.filter((x) => x !== r.stage))} /> {r.title} <span className="muted">{r.chars.toLocaleString("en-US")} characters</span></label>
            </li>
          ))}
        </ul>
      )}
      {err && <p className="field-error" role="alert">{err}</p>}
      <button type="button" disabled={disabled || on.length === 0} onClick={() => { onPick(from, rows.map((r) => r.stage).filter((x) => on.includes(x))); setFrom(""); setRows([]); setOn([]); }}>Add these results</button>
    </div>
  );
}

/** Which files are followed as the person's own instructions, so it is clear before anything runs. */
export function RequirementsNotice({ sources, onMaterial }: { sources: Source[]; onMaterial: () => void }) {
  const mine = sources.filter((s) => (s.role ?? "evidence") !== "evidence");
  if (mine.length === 0) return null;
  return (
    <div className="notice" role="status">
      <p>
        Followed as your instructions: {mine.map((s, i) => <span key={s.id}>{i > 0 ? "; " : ""}<strong>{s.name}</strong> ({s.role === "template" ? "a template to fill" : "instructions for the task"})</span>)}. Every other file is evidence to read and cite. Change this in Sources if needed.
      </p>
      <button className="ghost dismiss" onClick={onMaterial}>View sources</button>
    </div>
  );
}
