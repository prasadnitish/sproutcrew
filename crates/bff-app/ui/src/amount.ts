/** The number typed into an amount field, or null when the text is not one. The whole text must be a number above
 *  zero: "1o", "1,000", "10 000" and "5.5.5" are refused rather than read as 1, 1, 10 and 5.5. */
export function amount(t: string): number | null {
  const s = t.trim();
  if (s === "") return null;
  const v = Number(s);
  return Number.isFinite(v) && v > 0 ? v : null;
}
