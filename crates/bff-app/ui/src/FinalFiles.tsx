import { useEffect, useState } from "react";
import { api } from "./api";
import { Chip, Ctx, text } from "./ui";

type Final = { files: { kind: "word" | "pdf" | "deck" | "sheet" | "keynote"; path: string }[]; notes: string[]; open: string };
const KIND: Record<string, [string, string]> = { word: ["Word document", "Doc"], pdf: ["PDF", "PDF"], deck: ["PowerPoint deck", "Ppt"], sheet: ["Excel workbook (the tables, as values)", "Xls"], keynote: ["Keynote deck", "Key"] };

/** The finished files, made once every step has a result. Nothing shows until there is something to hand over. */
export default function FinalFiles({ ctx }: { ctx: Ctx }) {
  const [made, setMade] = useState<Final | null>(null);
  const [err, setErr] = useState("");
  const results = ctx.s.artifacts.map((a) => a.file).join();
  useEffect(() => {
    let live = true;
    api.call<Final>("final_files").then((f) => { if (live) { setMade(f); setErr(""); } }).catch((e) => live && setErr(text(e)));
    return () => { live = false; };
  }, [results]);
  const act = (cmd: string, path: string) => api.call(cmd, { path }).catch((e) => setErr(text(e)));
  if (err) return <p className="field-error" role="alert">{err}</p>;
  if (!made || (made.files.length === 0 && made.notes.length === 0)) return null;
  return (
    <section className="sheet final-files" aria-label="Your final document">
      <header className="sheet-head"><div><h2>Your final document</h2><p className="muted">Ready to hand over. The notes for your team are left out of these files.</p></div></header>
      {made.open && (
        <div className="notice warn">
          <p><Chip tone="warn">Still open before sending</Chip> The crew could not fix these without you.</p>
          <p className="final-open">{made.open}</p>
        </div>
      )}
      {made.files.map((f) => (
        <div className="file-line" key={f.path}>
          <span className="type-tile" aria-hidden="true">{KIND[f.kind][1]}</span>
          <span className="material-main"><strong>{KIND[f.kind][0]}</strong><span className="muted path">{f.path.split("/").pop()}</span></span>
          <span className="row-actions">
            <button className="primary" onClick={() => act("open_export", f.path)}>Open</button>
            <button className="ghost" onClick={() => act("reveal_export", f.path)}>Show in Finder</button>
          </span>
        </div>
      ))}
      {made.notes.map((n) => <p key={n} className="muted">{n}</p>)}
    </section>
  );
}
