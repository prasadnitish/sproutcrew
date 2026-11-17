export type Dest = "app" | "next" | "client" | "notion" | "github" | "posts";

/** Where work can go, most useful first. A project with code leads with what was built, and a project without any
 *  has no app to open. Documents and slides stay available either way. */
export function destOrder(hasCode: boolean): Dest[] {
  return hasCode ? ["app", "next", "github", "client", "notion", "posts"] : ["client", "next", "notion", "github", "posts"];
}

/** The result a follow-on project most likely needs: the finished document, or else the last step that finished. */
export function mainResult(done: { id: string }[]): string {
  return ["finish_document", "finish", "polish_document", "document", "recommend"].find((id) => done.some((st) => st.id === id)) ?? done[done.length - 1]?.id ?? "";
}

/** A first brief for a follow-on project. The person edits it before anything is created. */
export function nextBrief(kind: string, from: string): string {
  if (kind === "app_development") return `Build a small working prototype of what the project "${from}" recommends. Its results are in the sources, and the build must follow them.`;
  return `Continue from the project "${from}". Its results are in the sources. Say here what this project should produce.`;
}

/** What Results & sharing hands to the new project screen. */
export type NextDraft = { from: string; stages: string[]; kind: string; brief: string };
