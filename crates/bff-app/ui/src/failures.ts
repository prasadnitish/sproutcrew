/** A stop that holds all AI work until the app is reopened: a charge that could not be confirmed, or a step
 *  refused because such a stop is held. */
export function heldStop(why: string): boolean {
  return why.startsWith("stopped:") && (why.includes("AI work is on hold") || why.includes("cannot tell whether the request was charged"));
}

/** A stop held because the app's own token or spending cap was reached. A higher cap in Usage lets it go on
 *  without reopening the app. */
export function capStop(why: string): boolean {
  return /the API (token|spending) cap of/i.test(why);
}

/** A failure note as a sentence: without its "stopped:" or "paused:" label, with a capital and one full stop. The
 *  advice a held stop repeats is left to the box around it. */
export function plainWhy(why: string): string {
  let w = why.replace(/^(stopped|paused):\s*/, "").replace(/\s*Check your usage with the provider, then quit and reopen SproutCrew to go on\.?$/, "").replace(/^AI work is on hold\.\s*/, "").trim().replace(/[.\s]+$/, "");
  w = w.charAt(0).toUpperCase() + w.slice(1);
  return w ? `${w}.` : "";
}

/** "$0.011793 (1 entry unknown)" as a rounded figure and, beneath it, how many requests have no known cost. An
 *  unknown cost is never shown as zero. */
export function costParts(text: string): { center: string; sub: string } {
  const m = /^\$(\d+(?:\.\d+)?)(?:\s*\((\d+) (?:entry|entries) unknown\))?$/.exec(text.trim());
  if (!m) return { center: text, sub: "Not reported or priced yet" };
  const n = Number(m[1]);
  const center = n === 0 ? "$0" : n < 0.01 ? "under $0.01" : `$${n.toFixed(2)}`;
  const unknown = Number(m[2] ?? 0);
  return { center, sub: unknown ? `Plus ${unknown} request${unknown === 1 ? "" : "s"} whose cost is not known` : "As reported by the provider" };
}
