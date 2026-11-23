type Step = { id: string; owner_role: string; uses?: { route: string; model: string; effort: string } };
type Team = { role: string; route: string; model: string; effort: string }[];
type Shape<S extends Step> = { stages: S[]; team: Team };

/** The model, route and effort a step runs on: its own if it has them, or else its role's (as `graph.ts` says; kept
 *  here so the tests can load this file alone). */
const modelOf = (team: Team, st: Step) => {
  const t = team.find((x) => x.role === st.owner_role);
  return t && (st.uses ? { ...t, ...st.uses } : t);
};

/** The web step keeps its own search model, so a change to a side's model never touches it. */
const changeable = (st: Step) => st.id !== "web_research";

/** The models one side of the crew uses, the one most steps use first and the web step's last. */
export function sideModels<S extends Step>(plan: Shape<S>, onSide: (role: string) => boolean): string[] {
  const count = new Map<string, number>();
  for (const st of plan.stages) {
    const m = onSide(st.owner_role) ? modelOf(plan.team, st)?.model : undefined;
    if (m) count.set(m, (count.get(m) ?? 0) + (changeable(st) ? 1 : 0));
  }
  return [...count.keys()].sort((a, b) => count.get(b)! - count.get(a)!);
}

/** The steps a change of `model` would touch: this side's steps that run on it now. `onOpenRouter` is false when any of
 *  them runs on another route, where the card must not pick a model, because that would move the step to OpenRouter. */
export function scopeOf<S extends Step>(plan: Shape<S>, onSide: (role: string) => boolean, model: string): { steps: S[]; onOpenRouter: boolean } {
  const steps = plan.stages.filter((st) => changeable(st) && onSide(st.owner_role) && modelOf(plan.team, st)?.model === model);
  return { steps, onOpenRouter: steps.length > 0 && steps.every((st) => modelOf(plan.team, st)?.route === "R12") };
}

/** The plan's steps with `from` replaced by `to` on this side, each step keeping its own effort. Null when a step it
 *  would touch is not on OpenRouter. Steps on any other model are returned as they were. */
export function replaceModel<S extends Step>(plan: Shape<S>, onSide: (role: string) => boolean, from: string, to: string): S[] | null {
  const { steps, onOpenRouter } = scopeOf(plan, onSide, from);
  if (!onOpenRouter) return null;
  return plan.stages.map((st) => {
    const own = steps.includes(st) ? modelOf(plan.team, st) : undefined;
    return own ? { ...st, uses: { route: own.route, model: to, effort: own.effort } } : st;
  });
}
