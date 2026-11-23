import type { UsageRow } from "./usage";

type Titled = { id: string; title: string };

/** What a run has cost so far, in words: nothing when nothing was spent, "under 1¢" for a tiny amount, cents up to
 *  99¢, then dollars. `null` is a cost that was never reported, which is not zero. */
export function costPart(spentUsd: number | null): string {
  if (spentUsd === null) return "cost unknown so far";
  if (spentUsd <= 0) return "";
  if (spentUsd < 0.005) return "under 1¢ so far";
  const cents = Math.round(spentUsd * 100);
  return cents < 100 ? `${cents}¢ so far` : `$${spentUsd.toFixed(2)} so far`;
}

const joined = (parts: string[]) => parts.filter(Boolean).join(" · ");

/** One running step in plain words: "Writing the first draft · step 2 of 5 · 1¢ so far". `index` counts from 0, the
 *  step's place in the plan. The cost part is left out when nothing has been spent. */
export function statusLine(step: { title: string }, index: number, count: number, spentUsd: number | null): string {
  return joined([step.title, `step ${index + 1} of ${count}`, costPart(spentUsd)]);
}

const list = (words: string[]) => (words.length > 1 ? `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}` : words[0] ?? "");

/** Steps that run together: their titles, then "steps 3 and 4 of 5". `indexes` count from 0. */
export function runningLine(steps: { title: string }[], indexes: number[], count: number, spentUsd: number | null): string {
  if (steps.length === 1) return statusLine(steps[0], indexes[0], count, spentUsd);
  const titles = steps.length === 2 ? steps.map((s) => s.title).join(" and ") : list(steps.map((s) => s.title));
  return joined([titles, `steps ${list(indexes.map((i) => String(i + 1)))} of ${count}`, costPart(spentUsd)]);
}

/** What the project says when a run it did not start from this page is going: the page was left and opened again. */
export const RUN_GOING = "A run is already going in this project.";

/** The top line while something runs. `ids` are the plan's steps named in `busy`, which is how the run began, such as
 *  "stage b and stage c on live". A run of steps names them by title, never by ID. Other work, such as the lead's
 *  plan or a request for a change, is named plainly without the route it runs on. */
export function workingLine(busy: string, ids: string[], stages: Titled[], spentUsd: number | null): string {
  const found = ids.map((id) => stages.findIndex((s) => s.id === id)).filter((i) => i >= 0);
  if (found.length > 0) return runningLine(found.map((i) => stages[i]), found, stages.length, spentUsd);
  const plain = busy.replace(/ on [\w-]+$/, "").trim();
  return plain && !/\bstage /.test(plain) ? `Working: ${plain}` : "Working";
}

/** A progress line worth showing under the status line: a wait, or a long run. The first one only repeats the start. */
export function noteLine(line: string): string {
  return line.startsWith("Started:") ? "" : line;
}

/** Dollars the run has used since `since` (seconds), from the project's usage rows. 0 when nothing was recorded.
 *  `null` when requests were recorded and none reported a price, which is not the same as nothing spent. */
export function spentSince(rows: UsageRow[] | undefined, since: number): number | null {
  const used = (rows ?? []).filter((r) => r.at >= since);
  const priced = used.filter((r) => r.dollars !== null);
  if (used.length > 0 && priced.length === 0) return null;
  return priced.reduce((sum, r) => sum + (r.dollars ?? 0), 0);
}

/** What the run bar says after the person pressed Stop. `done` is how many steps have a result. */
export function stoppedLine(done: number): string {
  return done > 0 ? `Stopped by you after step ${done}` : "Stopped by you before any step finished";
}

/** What a project's top line says when nothing is running: where the work stands, in words. */
export function idleLine(p: { steps: number; done: number; approved: boolean; failed: boolean; stopped: boolean }): string {
  if (p.steps === 0) return "No plan yet";
  if (p.done === p.steps) return p.steps === 1 ? "The step is done" : `All ${p.steps} steps are done`;
  if (p.stopped) return stoppedLine(p.done);
  if (p.failed) return `A step did not finish · ${p.done} of ${p.steps} done`;
  if (!p.approved) return "The plan is waiting for your approval";
  return `${p.done} of ${p.steps} steps done`;
}

/** The one thing to do next, and the tab it is on. */
export function nextAction(p: { steps: number; done: number; approved: boolean }): { label: string; tab: "work" | "share" } {
  if (p.steps === 0) return { label: "Suggest a plan", tab: "work" };
  if (p.done === p.steps) return { label: "View the results", tab: "share" };
  if (!p.approved) return { label: "Review the plan", tab: "work" };
  return { label: "Open the plan", tab: "work" };
}
