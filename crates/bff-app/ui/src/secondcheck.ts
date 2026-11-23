import { modelOf } from "./graph";
import { secondCheckBy } from "./maker";

type Team = { role: string; route: string; model: string; effort: string }[];
type Step = { id: string; owner_role: string; uses?: { route: string; model: string; effort: string } };

/** The second check's name for a plan: the models on its checking steps against the models on its writing steps.
 *  With `only`, the name of that one checking step. */
export function secondCheckOf(team: Team, steps: Step[], checking: (role: string) => boolean, only?: Step): string {
  const models = (want: boolean) => [...new Set(steps.filter((st) => checking(st.owner_role) === want).map((st) => modelOf(team, st)?.model).filter((m): m is string => !!m))];
  const checkers = only ? [modelOf(team, only)?.model].filter((m): m is string => !!m) : models(true);
  return secondCheckBy(checkers, models(false));
}
