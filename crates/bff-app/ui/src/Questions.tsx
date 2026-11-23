import { useEffect, useId, useState } from "react";
import { api } from "./api";
import { answersOf, fieldNote, Question, startValues } from "./facts";
import { Ctx, text } from "./ui";

/** The few facts the crew can't find in the brief or the files. The run waits here after the plan is approved. The
 *  answers are kept as notes for the steps that follow, nothing is sent to a model by this form, and every field can
 *  be left empty. `onAnswered` carries the run on. */
export default function Questions({ ctx, onAnswered }: { ctx: Ctx; onAnswered: () => void }) {
  const [asked, setAsked] = useState<Question[] | null>(null);
  const [values, setValues] = useState<string[]>([]);
  const [err, setErr] = useState("");
  const [saving, setSaving] = useState(false);
  const id = useId();
  const held = JSON.stringify(ctx.s.blocked ?? {});
  useEffect(() => {
    let live = true;
    api.call<Question[]>("fact_questions").then((q) => { if (live) { setAsked(q); setValues(startValues(q)); } }).catch((e) => live && setErr(text(e)));
    return () => { live = false; };
  }, [held]);
  const send = async (skipAll: boolean) => {
    if (!asked) return;
    setSaving(true);
    setErr("");
    try {
      await api.call("answer_facts", { answers: answersOf(values, skipAll) });
    } catch (e) {
      // A refused answer is not saved, and the form stays as it was so it can be reworded.
      setErr(text(e));
      setSaving(false);
      return;
    }
    await ctx.refresh();
    setSaving(false);
    onAnswered();
  };
  return (
    <section className="sheet questions" id="questions" aria-label="A few things only you know">
      <header className="sheet-head">
        <div>
          <h2>A few things only you know</h2>
          <p className="muted">Your brief and files don't say these, and the crew won't guess them. Answer what you can. A question you leave empty shows up in the draft as a missing detail.</p>
        </div>
      </header>
      {!asked && !err && <p className="muted" role="status">Reading the questions…</p>}
      {asked && (
        <div className="gap-fields">
          {asked.map((q, i) => (
            <label key={`${i}-${q.ask}`} htmlFor={`${id}-${i}`} className="gap-field">
              <span>{q.ask}</span>
              {fieldNote(q) && <span className="muted small">{fieldNote(q)}</span>}
              <input id={`${id}-${i}`} value={values[i] ?? ""} maxLength={400} autoComplete="off" disabled={saving}
                onChange={(e) => setValues(values.map((v, j) => (j === i ? e.target.value : v)))} />
            </label>
          ))}
        </div>
      )}
      {err && <p className="field-error" role="alert">{err}</p>}
      <div className="actions">
        <button className="primary" disabled={!asked || saving || !!ctx.busy} onClick={() => void send(false)}>Continue</button>
        <button className="ghost" disabled={!asked || saving || !!ctx.busy} onClick={() => void send(true)}>Skip all</button>
      </div>
      <p className="muted">Your answers are saved as notes the next steps read, so they go to the same AI models as your brief. You don't approve the plan again.</p>
    </section>
  );
}
