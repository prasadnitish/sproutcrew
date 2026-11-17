/** The first of `ids` (in the order given) that names one of `steps`, or "" when none does. */
export function guessStep(steps: { id: string }[], ids: string[]): string {
  for (const id of ids) if (steps.some((st) => st.id === id)) return id;
  return "";
}
