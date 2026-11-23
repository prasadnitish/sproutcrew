/** What a second check said, as the program reports it. `verdict` is null when the check gave none. */
export type Verdict = "ready" | "fix_first" | "do_not_send";
export type Check = { verdict: Verdict | null; flagged: string[] };

const WORD: Record<Verdict, string> = { ready: "Ready", fix_first: "Fix first", do_not_send: "Do not send" };
const things = (n: number) => `${n} ${n === 1 ? "thing" : "things"}`;

/** The lines that ask for a change. A line the check wrote to say a point is fine ("No fix needed") is not one. */
export const asks = (c: Check): string[] => c.flagged.filter((f) => !/no fix needed/i.test(f));

/** "Fix first, 3 things": the verdict, and how many problems the check listed. A check that says ready lists none. */
export function headline(c: Check): string {
  if (c.verdict === null) return "No verdict";
  const n = asks(c).length;
  return c.verdict === "ready" || n === 0 ? WORD[c.verdict] : `${WORD[c.verdict]}, ${things(n)}`;
}

/** Whether the step's mark may be the green tick: a step that was asked for no verdict, or a check that says ready. */
export function ticked(c: Check | undefined): boolean {
  return !c || c.verdict === "ready";
}

/** What a "do not send" verdict leaves the person to do: the finish step is held until a check says otherwise. */
export const DO_NOT_SEND_NEXT = "Run the second check again, or use Ask for a change to fix the draft.";

/** What the finished result says about the check, or null when there is nothing to add. `applied` is whether the
 *  finish step ran after the check. It says the notes were sent to the final step, never that they were applied. */
export function resultLine(c: Check, applied: boolean): { tone: "warn" | "stop"; text: string; hint?: string } | null {
  if (c.verdict === "do_not_send") return { tone: "stop", text: "Not ready: the second check said do not send", hint: DO_NOT_SEND_NEXT };
  if (!applied) return null;
  if (c.verdict === null) return { tone: "warn", text: "Second check: no verdict, so the final version was not checked. Read it before you send it" };
  if (c.verdict === "fix_first") {
    // The check only lists what to change. The final step is sent the list; whether it made each change is not known.
    const n = asks(c).length;
    return { tone: "warn", text: `Second check: ${WORD.fix_first} · ${n === 0 ? "notes" : `${n} ${n === 1 ? "note" : "notes"}`} sent to the final step` };
  }
  return null;
}

/** A long list of lines cut for the result: the first few to read at once, and the rest behind "Show all". Nothing is
 *  left out, and a short list has no rest. */
export function firstAndRest<T>(items: T[], first = 3): { shown: T[]; rest: T[] } {
  return items.length <= first ? { shown: items, rest: [] } : { shown: items.slice(0, first), rest: items.slice(first) };
}
