/** A subsequence match, so "wrk" finds "Work". Higher is better, and null means no match. */
export function fuzzy(query: string, label: string): number | null {
  const q = query.trim().toLowerCase();
  if (!q) return 0;
  const l = label.toLowerCase();
  let at = 0, score = 0, last = -2;
  for (const c of q) {
    const i = l.indexOf(c, at);
    if (i < 0) return null;
    score += i === last + 1 ? 3 : 1;
    if (i === 0 || l[i - 1] === " ") score += 2;
    last = i;
    at = i + 1;
  }
  return score;
}

