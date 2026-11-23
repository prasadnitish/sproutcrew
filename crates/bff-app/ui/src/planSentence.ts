type Step = { id: string; title: string; owner_role: string };

type Words = {
  /** A role's name as it reads in a sentence, such as "editor". */
  label?: (role: string) => string;
  /** Whether a role is on the checking side. Only the reviewer is, unless this says more. */
  checking?: (role: string) => boolean;
};

const CUT = 4;

/** The plan in one sentence for the card: who does the work, and whether a second check from another company reads
 *  it. `sameMaker` is null when the maker of the second check is not known, so the sentence makes no claim about it.
 *  The check for what is missing and the web research are named as steps of their own. With no steps at all the
 *  outcome's promise is the sentence. */
export function planSentence(stages: Step[], promise: string, sameMaker: boolean | null, words: Words = {}): string {
  const label = words.label ?? ((r: string) => r.replace(/_/g, " "));
  const checking = words.checking ?? ((r: string) => r === "reviewer");
  if (stages.length === 0) return promise;

  const work = stages.filter((st) => st.id !== "facts" && st.id !== "web_research" && !checking(st.owner_role));
  const roles = [...new Set(work.map((st) => `the ${label(st.owner_role)}`))];
  const shown = roles.slice(0, CUT);
  const more = roles.length - shown.length;
  const names = more > 0 ? `${shown.join(", ")} and ${more} other${more === 1 ? "" : "s"}` : shown.length > 1 ? `${shown.slice(0, -1).join(", ")} and ${shown[shown.length - 1]}` : shown[0] ?? "";

  const last = stages[stages.length - 1];
  const checked = stages.some((st) => checking(st.owner_role));
  const parts = [
    stages.some((st) => st.id === "facts") ? "the crew checks what's missing" : "",
    stages.some((st) => st.id === "web_research") ? "a web researcher looks things up" : "",
    names ? `${names} ${roles.length > 1 ? "do" : "does"} the work` : "",
    checked ? `a second check${sameMaker === null ? "" : ` from ${sameMaker ? "the same" : "a different"} company`} reads it` : "",
    checked && !checking(last.owner_role) && last.id !== "web_research" ? `the ${label(last.owner_role)} finishes it` : "",
  ].filter(Boolean);
  if (parts.length === 0) return promise;

  // "First ..., then ..." only when a step before the work leads the sentence.
  const lead = stages[0].id === "facts" || stages[0].id === "web_research";
  const text = parts.map((p, i) => (i === 0 ? (lead && parts.length > 1 ? `First ${p}` : p) : lead && i === 1 ? `then ${p}` : i === parts.length - 1 ? `and ${p}` : p)).join(", ");
  return `${text.charAt(0).toUpperCase()}${text.slice(1)}.`;
}
