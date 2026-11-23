/** Details only the person can supply. A writing step marks a missing fact as [NEEDED: what is missing]; the result
 *  lists them, and the person fills them in on the result. These words match what the app's core looks for, so the
 *  finish step reads the note and the app knows the person chose to finish around the details. */
export type Gap = { what: string };

export const DETAILS_NOTE = "Details from the person:";
export const WRITE_AROUND = "Write around these missing details; do not leave placeholders: ";
/** The longest note the app keeps. */
export const NOTE_LIMIT = 2000;

/** "1 detail missing", "3 details missing". */
export const missing = (n: number) => `${n} ${n === 1 ? "detail" : "details"} missing`;

/** "1 detail left out", "6 details left out": what "Finish with what I have" took out of the piece. */
export const leftOut = (n: number) => `${n} ${n === 1 ? "detail" : "details"} left out`;
/** What the list under that says. */
export const LEFT_OUT_LINE = "These were left out of the piece:";

/** The note that gives the finish step the answers the person typed: one `what: answer` line each, and none for a
 *  field left empty. Empty when nothing was answered. */
export function answersNote(gaps: Gap[], answers: Record<string, string>): string {
  const lines = gaps.flatMap((g) => {
    const a = (answers[g.what] ?? "").replace(/\s+/g, " ").trim();
    return a ? [`${g.what}: ${a}`] : [];
  });
  return lines.length ? `${DETAILS_NOTE}\n${lines.join("\n")}` : "";
}

/** The note that tells the finish step to write around what is missing. A long list is cut to what a note can hold. */
export function writeAroundNote(gaps: Gap[]): string {
  // The list is split at "; ", so a ";" inside a detail is written as a comma.
  const note = `${WRITE_AROUND}${gaps.map((g) => g.what.replace(/;/g, ",")).join("; ")}`;
  return note.length > NOTE_LIMIT ? `${note.slice(0, NOTE_LIMIT - 1)}…` : note;
}
