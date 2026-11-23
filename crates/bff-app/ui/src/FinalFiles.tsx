import { useEffect, useId, useState } from "react";
import { api } from "./api";
import { afterOf } from "./graph";
import { answersNote, Gap, LEFT_OUT_LINE, leftOut, missing, NOTE_LIMIT, writeAroundNote } from "./gaps";
import { Chip, Ctx, text } from "./ui";
import { firstAndRest, resultLine } from "./verdict";

type Final = {
  files: { kind: "word" | "pdf" | "deck" | "sheet" | "keynote"; path: string }[]; notes: string[]; open: string;
  /** The folder in Documents the files were saved to, or null when they could not be (they are then in the project). */
  folder?: string | null;
  /** What the draft lacks that only the person can supply. Files are made once there are none, or the person finished around them. */
  gaps?: Gap[]; accepted?: boolean;
  /** After "Finish with what I have": the details the piece was finished without. They are out of the text and the files. */
  left_out?: Gap[];
  /** The steps that get the person's answer as a note, and the steps to run again after it. */
  note_steps?: string[]; rerun?: string[];
};
const KIND: Record<string, [string, string]> = { word: ["Word document", "Doc"], pdf: ["PDF", "PDF"], deck: ["PowerPoint deck", "Ppt"], sheet: ["Excel workbook (the tables, as values)", "Xls"], keynote: ["Keynote deck", "Key"] };

/** The lines the second check listed. A long list shows its first three and the rest behind "Show all N", so it does not
 *  push what the person has to do off the screen. A "do not send" list is always whole. */
function FlaggedList({ lines, all }: { lines: string[]; all: boolean }) {
  const { shown, rest } = firstAndRest(lines, all ? lines.length : 3);
  return (
    <>
      <ul>{shown.map((f, i) => <li key={i}>{f}</li>)}</ul>
      {rest.length > 0 && (
        <details className="flagged-rest">
          <summary>Show all {lines.length}</summary>
          <ul>{rest.map((f, i) => <li key={i}>{f}</li>)}</ul>
        </details>
      )}
    </>
  );
}

/** What the second check said about the draft, on the result: the fixes applied in the final version, or that the
 *  piece is not ready because the check said not to send it, with its reasons. Nothing when it said ready. */
export function SecondCheckResult({ ctx }: { ctx: Ctx }) {
  const plan = ctx.s.plan;
  if (!plan) return null;
  const last = (name: string) => ctx.s.artifacts.reduce((at, a, i) => (a.name === name ? i : at), -1);
  return (
    <>
      {Object.entries(ctx.s.checks ?? {}).map(([id, check]) => {
        const at = last(id);
        const applied = afterOf(plan.stages, id).some((x) => last(x.id) > at);
        const line = resultLine(check, applied);
        if (!line) return null;
        return (
          <div key={id} className={`notice ${line.tone === "stop" ? "error" : "warn"}`} role="status">
            <p><strong>{line.text}</strong></p>
            {check.flagged.length > 0 && <FlaggedList lines={check.flagged} all={check.verdict === "do_not_send"} />}
            {line.hint && <p>{line.hint}</p>}
            {check.verdict === "do_not_send" && (
              <div className="actions tight">
                <button disabled={!!ctx.busy || ctx.s.gate !== "allowed"} title={ctx.busy ? "Wait for the current run to finish." : ctx.s.gate !== "allowed" ? "Approve the plan first." : undefined}
                  onClick={() => ctx.guard(() => api.run("run_stage", { engine: ctx.engine, stageId: id }, ctx.track))}>Run the second check again</button>
              </div>
            )}
          </div>
        );
      })}
    </>
  );
}

/** The draft's missing details, one field each. The answers go to the finish step as a note and it runs again under the
 *  approval the project already has; or the person finishes with what is there, and the step writes around them. */
function MissingDetails({ ctx, made }: { ctx: Ctx; made: Final }) {
  const gaps = made.gaps ?? [];
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const id = useId();
  const note = answersNote(gaps, answers);
  const tooLong = note.length > NOTE_LIMIT;
  const blocked = ctx.busy ? "Wait for the current run to finish." : ctx.s.gate !== "allowed" ? "Approve the plan first." : "";
  const finish = (text: string) =>
    ctx.guard(async () => {
      // The run holds the project like any run of steps, so it is not started on top of another and another project
      // stays closed between its two steps. A run already going refuses this one.
      await api.call("begin_run");
      try {
        ctx.stopFlag.current = false;
        for (const stage of made.note_steps ?? []) await api.call("add_note", { stage, text });
        for (const stageId of made.rerun ?? []) {
          if (ctx.stopFlag.current) break;
          await api.run("run_stage", { engine: ctx.engine, stageId }, ctx.track);
        }
      } finally {
        await api.call("end_run").catch(() => undefined);
      }
    });
  return (
    <section id="final-files" className="sheet final-files" aria-label="Your draft">
      <header className="sheet-head"><div><h2>Your draft</h2><p className="muted"><strong>Draft · {missing(gaps.length)}.</strong> The crew did not guess these. Word, PDF and PowerPoint files are made once they are filled in.</p></div></header>
      <div className="gap-fields">
        {gaps.map((g, i) => (
          <label key={g.what} htmlFor={`${id}-${i}`} className="gap-field">
            <span>{g.what}</span>
            <input id={`${id}-${i}`} value={answers[g.what] ?? ""} maxLength={400} autoComplete="off"
              onChange={(e) => setAnswers({ ...answers, [g.what]: e.target.value })} />
          </label>
        ))}
      </div>
      {tooLong && <p className="field-error" role="alert">These answers are too long to save together. Shorten them.</p>}
      <div className="actions">
        <button className="primary" disabled={!!blocked || !note || tooLong} title={blocked || (note ? undefined : "Type at least one answer first")} onClick={() => finish(note)}>Finish the article</button>
        <button className="ghost" disabled={!!blocked} title={blocked || undefined} onClick={() => finish(writeAroundNote(gaps))}>Finish with what I have (about 1¢)</button>
      </div>
      <p className="muted">Finishing runs the last step once more, under the approval you already gave. A detail you leave empty stays missing.</p>
      <SecondCheckResult ctx={ctx} />
    </section>
  );
}

/** The finished files, made once every step has a result. Nothing shows until there is something to hand over. */
export default function FinalFiles({ ctx }: { ctx: Ctx }) {
  const [made, setMade] = useState<Final | null>(null);
  const [err, setErr] = useState("");
  // A note the person saves for the finish step changes what is missing, and so does a rerun that gives the same text
  // again, so the list is read again when a run ends.
  const results = [ctx.s.artifacts.map((a) => a.file).join(), ctx.s.notes.map((n) => n.id).join(), ctx.busy ? "busy" : "idle"].join("|");
  useEffect(() => {
    let live = true;
    api.call<Final>("final_files").then((f) => { if (live) { setMade(f); setErr(""); } }).catch((e) => live && setErr(text(e)));
    return () => { live = false; };
  }, [results]);
  const act = (cmd: string, path: string) => api.call(cmd, { path }).catch((e) => setErr(text(e)));
  if (err) return <p className="field-error" role="alert">{err}</p>;
  if (made && made.gaps?.length && !made.accepted) return <MissingDetails ctx={ctx} made={made} />;
  if (!made || (made.files.length === 0 && made.notes.length === 0)) return <SecondCheckResult ctx={ctx} />;
  return (
    <section id="final-files" className="sheet final-files" aria-label="Your final document">
      <header className="sheet-head"><div><h2>Your final document</h2><p className="muted">{made.left_out?.length ? `Finished with ${leftOut(made.left_out.length)}.` : "Ready to hand over."} The notes for your team are left out of these files.</p></div></header>
      {made.folder && (
        <div className="file-line folder-line">
          <span className="material-main"><strong>Saved in your Documents folder</strong><span className="muted path">{made.folder.split("/").slice(-3).join("/")}</span></span>
          <span className="row-actions"><button className="primary" onClick={() => act("reveal_export", made.folder!)}>Show in Finder</button></span>
        </div>
      )}
      <SecondCheckResult ctx={ctx} />
      {made.left_out && made.left_out.length > 0 && (
        <div className="notice warn">
          <p><Chip tone="warn">Finished with {leftOut(made.left_out.length)}</Chip> {LEFT_OUT_LINE}</p>
          <ul>{made.left_out.map((g) => <li key={g.what}>{g.what}</li>)}</ul>
        </div>
      )}
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
            <button className={made.folder ? "ghost" : "primary"} onClick={() => act("open_export", f.path)}>Open</button>
            {!made.folder && <button className="ghost" onClick={() => act("reveal_export", f.path)}>Show in Finder</button>}
          </span>
        </div>
      ))}
      {made.notes.map((n) => <p key={n} className="muted">{n}</p>)}
      <ReviewedLine ctx={ctx} />
    </section>
  );
}

/** Marks the results reviewed as they are now, which takes the project off the Board (DESIGN_SPEC.md §10). A result
 *  recorded later needs reviewing again. */
function ReviewedLine({ ctx }: { ctx: Ctx }) {
  const r = ctx.s.result_review;
  if (!r?.finished) return null;
  if (r.reviewed) return <p className="muted"><span aria-hidden="true">✓ </span>You marked these results reviewed.</p>;
  return (
    <div className="actions">
      <button disabled={!!ctx.busy} onClick={() => void ctx.guard(() => api.call("mark_reviewed", { project: ctx.s.name, version: r.version }))}>Mark reviewed</button>
      <span className="muted">Takes it off the Board. A new result needs reviewing again.</span>
    </div>
  );
}
