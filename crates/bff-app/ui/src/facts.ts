/** What only the person knows. After the plan is approved, a first step asks for facts the brief and files don't give.
 *  The run waits on a short form, the answers are kept as notes, and the run goes on. These words match what the app's
 *  core holds the steps back with. */
export type Question = { ask: string; from: string | null; suggested: string | null };

export const WAITING = "Waiting for your answers";

/** Whether some step is held for the person's answers. */
export const asking = (blocked?: Record<string, string>) => Object.values(blocked ?? {}).includes(WAITING);

/** What each field holds to begin with: the answer the crew found in a file, or nothing. */
export const startValues = (qs: Question[]) => qs.map((q) => q.suggested ?? "");

/** The small line under a question that has a suggestion: the file it came from. */
export const fieldNote = (q: Question) => (q.suggested ? (q.from ? `From ${q.from}` : "Suggested") : "");

/** What the app is given: one entry for each question, `null` for one left empty or skipped. */
export const answersOf = (values: string[], skipAll: boolean): (string | null)[] =>
  values.map((v) => (skipAll ? null : v.trim() || null));
