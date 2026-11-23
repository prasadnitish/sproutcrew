/** What a step waits for: its own list, or the step before it when it has none. The same rule as the plan's. */
export function needsOf(steps: { id: string; after?: string[] }[], i: number): string[] {
  const s = steps[i];
  if (s.after && s.after.length) return s.after;
  return i > 0 ? [steps[i - 1].id] : [];
}

type Choice = { route: string; model: string; effort: string };
type Owned = { id: string; owner_role: string; after?: string[]; uses?: Choice };

/** The provider, model, and effort a step runs on: its own if it has them, or else its role's. */
export function modelOf<T extends Choice & { role: string }>(team: T[], st: Owned): T | undefined {
  const t = team.find((x) => x.role === st.owner_role);
  return t && (st.uses ? { ...t, ...st.uses } : t);
}

/** Every step `id` reads, directly or through others. */
function upstream(steps: Owned[], id: string, seen = new Set<string>()): Set<string> {
  const i = steps.findIndex((s) => s.id === id);
  for (const n of i < 0 ? [] : needsOf(steps, i)) if (!seen.has(n)) { seen.add(n); upstream(steps, n, seen); }
  return seen;
}

/** Every step that reads this step's result, directly or through others. In plan order. */
export function afterOf<T extends Owned>(steps: T[], id: string): T[] {
  return steps.filter((s) => s.id !== id && upstream(steps, s.id).has(id));
}

/** The checking steps that read this step's result, directly or through others. In plan order. */
export function checksOf<T extends Owned>(steps: T[], id: string, checking: (role: string) => boolean): T[] {
  return steps.filter((s) => checking(s.owner_role) && upstream(steps, s.id).has(id));
}

/** Whether a checking step is on the same provider as all the writing it reads, so it is no second opinion. */
export function sameProviderCheck(team: (Choice & { role: string })[], steps: Owned[], st: Owned, checking: (role: string) => boolean): boolean {
  if (!checking(st.owner_role)) return false;
  const route = modelOf(team, st)?.route;
  const read = steps.filter((s) => !checking(s.owner_role) && upstream(steps, st.id).has(s.id));
  return read.length > 0 && read.every((s) => modelOf(team, s)?.route === route);
}

/** The steps that can start now: not done, and everything they wait for is done. In plan order. */
export function readySteps(steps: { id: string; after?: string[] }[], done: string[]): string[] {
  return steps.filter((s, i) => !done.includes(s.id) && needsOf(steps, i).every((n) => done.includes(n))).map((s) => s.id);
}

/** The steps named as running in the busy text, such as "stage b and stage c on live". */
export function runningIds(busy: string): string[] {
  return [...busy.matchAll(/\bstage ([\w-]+)/g)].map((m) => m[1]);
}

type Shaped = { id: string; title: string; owner_role: string; guide?: string; after?: string[] };
export type NewStep = { id: string; title: string; owner_role: string; guide: string; after: string };

/** The id for a step a person adds: lowercase letters, digits, and underscores from the title, not already taken. */
export function stepId(title: string, taken: string[]): string {
  const base = title.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 40) || "step";
  let id = base, n = 2;
  while (taken.includes(id)) id = `${base}_${n++}`;
  return id;
}

/** The plan's steps after a person leaves some out and adds others. A step that waited for a step that is gone waits
 *  for what that step waited for, so the flow stays connected and nothing waits for a step that is not there. A new
 *  step goes right after the step it waits for. When nothing changes the steps come back as they were, so the
 *  plan's approval is not disturbed. Every step in a changed plan names what it waits for. */
export function reshape(steps: Shaped[], off: string[], added: NewStep[]): Shaped[] {
  if (off.length === 0 && added.length === 0) return steps;
  const needs = new Map<string, string[]>(steps.map((s, i) => [s.id, needsOf(steps, i)]));
  const gone = new Set(off);
  const live = (id: string, seen = new Set<string>()): string[] => {
    if (!gone.has(id)) return [id];
    if (seen.has(id)) return [];
    seen.add(id);
    return (needs.get(id) ?? []).flatMap((d) => live(d, seen));
  };
  const direct = new Map<string, string[]>(steps.filter((s) => !gone.has(s.id)).map((s) => [s.id, [...new Set((needs.get(s.id) ?? []).flatMap((d) => live(d)))]]));
  const above = (id: string, seen = new Set<string>()): Set<string> => {
    for (const d of direct.get(id) ?? []) if (!seen.has(d)) { seen.add(d); above(d, seen); }
    return seen;
  };
  // A step need not wait for something that a step it already waits for waits for.
  const lean = (deps: string[]) => deps.filter((d) => !deps.some((o) => o !== d && above(o).has(d)));
  const kept = steps.filter((s) => !gone.has(s.id)).map((s) => ({ ...s, after: lean(direct.get(s.id) ?? []) }));
  const out: Shaped[] = [...kept];
  for (const a of added) {
    const target = lean(a.after ? live(a.after) : []);
    const at = target.length ? Math.max(...target.map((t) => out.findIndex((s) => s.id === t))) + 1 : out.length;
    out.splice(at, 0, { id: a.id, title: a.title, owner_role: a.owner_role, guide: a.guide, after: target });
  }
  return out;
}
