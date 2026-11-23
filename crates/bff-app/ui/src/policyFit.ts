/** Policy defaults in words (DESIGN_SPEC.md §10 Policy). Plain code, so the words are tested. */
export type Choice = "ask" | "never";
export type Privacy = "standard" | "no_retention";
export type Defaults = { send_outside: Choice; delete_files: Choice; install_software: Choice; privacy: Privacy };
export type Guard = "ask_each_time" | "never" | "varies_by_role";
export type Fit = { looser: string[]; stricter: string[] };
export type PolicyRow = { name: string; plan: boolean; guardrails?: { send_outside: Guard; delete_files: Guard; install_software: Guard }; privacy?: Privacy; fit?: Fit; approved?: boolean; hash?: string | null };
export type Overview = { defaults: Defaults; projects: PolicyRow[]; errors: { project: string; error: string }[] };

export const FIELDS: { id: keyof Defaults; label: string }[] = [
  { id: "send_outside", label: "Send outside this Mac" },
  { id: "delete_files", label: "Delete files" },
  { id: "install_software", label: "Install software" },
  { id: "privacy", label: "Providers may keep" },
];

const VERB: Record<string, [string, string]> = {
  send_outside: ["asks before sending outside", "never sends outside"],
  delete_files: ["asks before deleting", "never deletes"],
  install_software: ["asks before installing", "never installs"],
};

/** One field of a policy as a phrase: "asks before deleting", "providers keep nothing". */
export function phrase(field: string, value: string): string {
  if (field === "privacy") return value === "no_retention" ? "providers keep nothing" : "providers follow their own policy";
  const [ask, never] = VERB[field] ?? [field, field];
  return value === "never" ? never : value === "varies_by_role" ? `${ask} for some roles` : ask;
}

/** The defaults as one line, for the closed section. */
export const defaultsLine = (d: Defaults) => FIELDS.map((f) => phrase(f.id, d[f.id])).join("; ");

/** Which group a project goes in: looser wins, then stricter, then same. */
export function groupOf(r: PolicyRow): "looser" | "stricter" | "same" {
  if (r.fit?.looser.length) return "looser";
  if (r.fit?.stricter.length) return "stricter";
  return "same";
}

/** A project's value against the defaults: what differs, in words, or "same as your defaults". */
export function fitLine(r: PolicyRow): string {
  if (!r.plan || !r.guardrails) return "no plan yet";
  const g = { ...r.guardrails, privacy: r.privacy ?? "standard" } as Record<string, string>;
  const diff = [...(r.fit?.looser ?? []), ...(r.fit?.stricter ?? [])];
  return diff.length ? diff.map((f) => phrase(f, g[f])).join("; ") : "same as your defaults";
}

/** The projects that allow more than `next`, which saving those defaults offers to tighten. */
export function looserThan(rows: PolicyRow[], next: Defaults): PolicyRow[] {
  return rows.filter((r) => r.plan && r.guardrails && (
    (["send_outside", "delete_files", "install_software"] as const).some((k) => next[k] === "never" && r.guardrails![k] !== "never")
  ));
}
