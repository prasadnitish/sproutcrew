import type { UsageRow } from "./usage";
import type { Check as SecondCheck } from "./verdict";
import { Question, WAITING } from "./facts";
import { plainWhy } from "./failures";
// The only file that talks to the backend. Outside the Tauri shell it falls back to an in-memory mock,
// so the screens can be checked in a plain browser. The mock is labelled and never claims a live run.

import { readySteps } from "./graph";
import { failedRun } from "./runs";
import { findKeyShape, KEY_IN_BRIEF } from "./keyshape";

export type EngineMode = "replay" | "live";
export type StepModel = { route: string; model: string; effort: string };
export type Stage = { id: string; title: string; owner_role: string; guide?: string; after?: string[]; uses?: StepModel };
export type Assignment = { role: string; route: string; model: string; effort: string };
export type Plan = { brief: string; stages: Stage[]; team: Assignment[]; fallbacks?: Assignment[]; privacy?: "standard" | "no_retention"; keynote?: boolean };
export type Artifact = { name: string; sha256: string; file: string; from_task: string };
export type Finding = { id: string; claim: string; evidence: string; decision: "accept" | "reject" | null; reason: string | null; escalated?: boolean };
export type ReviewSummary = {
  owner: string;
  version: number;
  head: string;
  findings: Finding[];
  reviewers: Record<string, "valid" | "invalidated" | "none">;
};
export type Budget = { limit: number; spent: number; available: number; paused: boolean; unknown_charges: number } | null;
/** A project's dollar limit for paid models, in dollars. `locked` says why its record could not be read. */
/** A template: its look (null for SproutCrew's own), sections and writing style, and where it came from. */
export type TemplateSaved = {
  name: string; from_id: string; from_name: string; has_logo: boolean; outline: string[]; style: string;
  look: { font: string; accent: string; heading_font?: string | null; body_pt?: number | null; margins_mm?: number[] | null; page_mm?: number[] | null } | null;
};
export type ProjectLimit = { cap: number; spent: number; held: number; available: number; unknown_charges: number; locked: string | null };
export type Post = {
  id: string;
  destination: string;
  text: string;
  scheduled_at: number;
  status: "draft" | "approved" | "needs_check" | "sent" | "failed" | "cancelled";
  receipt: { remote_id: string; sent_at: number } | null;
};
export type WriteItem = (
  | { kind: "notion"; parent: string; title: string; body: string; append?: boolean }
  | { kind: "github"; repo: string; branch: string; folder: string; files: { path: string; bytes: number }[] }
) & { id: string; status: "proposed" | "approved" | "sending" | "done" | "failed" | "cancelled"; note: string; url: string };
export type RouteCost = { tokens: number | null; api_dollars: number | null; unknown_token_steps: number };
export type SourceRole = "evidence" | "requirements" | "template";
export type Source = { id: string; name: string; /** A web page's own title, shown in place of the file name. */ title?: string | null; bytes: number; pages: number; flags: string[]; suspicious: string[]; origin: string; role?: SourceRole; role_chosen?: boolean };
export type RoleInfo = { role: string; label: string; does: string; side: "writing" | "checking" };
export type ProjectResult = { stage: string; title: string; chars: number };
export type UseCase = { id: string; name: string; promise: string; steps: string[]; owners: string[]; team: RoleInfo[]; wants_files: boolean };
export type Added = { added: Source[]; skipped: { name: string; reason: string }[] };
export type Note = { id: string; stage: string; text: string; at: number };
export type Citation = { source: string; quote: string; found: boolean; note: string };
export type ChatMessage = { role: "user" | "assistant"; text: string; at: number; by: string; citations: Citation[]; basis: string };
export type Input = { kind: "source" | "requirements" | "template" | "previous" | "earlier" | "later"; id: string; label: string; file: string; sha256: string };
export type Rules = { text: string; at: number; sha256: string };
export type Trace = { task: string; stage: string; route: string; model: string; effort: string; at: number; guide: string; skills?: string[]; notes: Note[]; rules?: string; inputs: Input[] };
export type Change = { op: string; step: string; text: string; title: string; id: string; role: string; after: string; route: string; model: string; effort: string };
export type Proposal = {
  id: string;
  changes: Change[];
  summary: string[];
  problems: string[];
  stale: string[];
  rerun: string[];
  status: "pending" | "blocked" | "applied" | "dismissed" | "superseded";
  plan_hash: string;
};
export type Turn = { role: "user" | "assistant"; text: string; at: number; by: string; proposal: Proposal | null };
export type Permission = "allow" | "ask" | "deny";
export type PolicyMatrix = Record<string, Record<string, Permission>>;
export type PolicyAsk = { id: string; tier: "destructive" | "consequential" | "reversible"; role: string; action: { stage: string; tool: string; target: string; payload_hash: string } };
export type ActionPolicy = { matrix: PolicyMatrix; guardrails?: import("./ReviewBoard").Guardrails; scope_hash: string; counts: { allowed: number; asked: number; denied: number }; pending: PolicyAsk[]; assurance: string };
export type CodeDraft = { task: string; stage: string; role: string; base: string; patch: string; patch_hash: string; status: string; summary: string };
export type AttentionStatus = { enabled: boolean; count: number | null; errors: number; last_error: string | null; open_inbox: boolean; names?: boolean };
export type OpenRouterLimits = { checked_at: number; daily: { used: number; limit: number; remaining: number } | null };
export type State = {
  open: boolean;
  name: string;
  brief: string;
  sources: Source[];
  plan: Plan | null;
  role_info: Record<string, RoleInfo>;
  problems: string[];
  gate: "allowed" | "not_approved" | "plan_changed";
  approved_by: string | null;
  /** The version an approval would cover, read with this state. The plan card sends it back when approving. */
  approval_hash: string | null;
  /** The results as they are in this state: what "Mark reviewed" covers, and whether it is done. */
  result_review?: { version: string; finished: boolean; reviewed: boolean };
  policy?: ActionPolicy | null;
  code_review?: { items: CodeDraft[]; head: string | null };
  artifacts: Artifact[];
  steps: { step: string; tokens: string; plan_window: string }[];
  meters: { tokens: string; plan_window: string; api_dollars: string };
  engine: string;
  review: Record<string, ReviewSummary> | null;
  web_research?: boolean;
  humaniser?: boolean;
  can_humanise?: boolean;
  will_skip?: { id: string; title: string; missing: string[] }[];
  stand_ins?: Record<string, { from_route: string; from_model: string; to_route: string; to_model: string; why: string }>;
  budget: Budget;
  project_limit?: ProjectLimit;
  usage_rows?: UsageRow[];
  /** A run is going now: a run of steps, or one step in flight. A page that was left and opened again reads this. */
  run_active?: boolean;
  /** Set when the person pressed Stop on a run and no step has started since. */
  stopped?: { after_stage: string | null } | null;
  /** The project's template, or `null` for none. */
  look?: TemplateSaved | null;
  timings: Record<string, { seconds: number; route: string; model: string }>;
  cost_by_route: Record<string, RouteCost>;
  tokens_by_model: ModelTokens[];
  outbox: Post[] | null;
  writes: WriteItem[];
  notes: Note[];
  /** The rules for this exercise, which come before every other instruction. Null when there are none. */
  rules: Rules | null;
  /** Steps whose latest attempt did not finish, with the reason. */
  failures?: Record<string, string>;
  /** Which limit stopped each failed step: per_minute, daily_cap, upstream_busy, out_of_credits, key_limit or other. */
  failure_kinds?: Record<string, string>;
  /** What each second check that was asked for a verdict said, by step. A step that was not asked is left out, so an
   *  older project shows none. A check that gave no verdict has `verdict: null`. */
  checks?: Record<string, SecondCheck>;
  /** Steps held back, with the reason, such as the finish step after a check said do not send. */
  blocked?: Record<string, string>;
  code_files?: { path: string; bytes: number }[];
  app?: { page: boolean; opens: boolean; why: string | null };
  links?: { from: string[]; to: string[] };
};
export type RunEvent =
  | { event: "started"; engine: string; what: string }
  | { event: "progress"; line: string }
  | { event: "finished"; ok: boolean; note: string };
export type Info = { codex_found: boolean; swept_orphans: number; stress_mode?: boolean; built_at?: number | null; version?: string };
export type Tool = { name: "claude" | "codex"; path: string | null; version: string | null; chosen: string | null };
export type Check = { name: string; status: "pass" | "warn" | "fail"; detail: string };
export type ApiRoute = { route: string; state: "ready" | "no_key" | "not_configured"; address?: string | null; model?: string | null; has_key?: boolean };
export type Recommended = { skill: string; label?: string; why: string; roles: string[]; on: boolean };
export type Tools = {
  recommended?: Recommended[];
  skills: { name: string; description: string; source: string }[];
  mcp: { name: string; source: string; transport: string }[];
  truncated: boolean;
  selected: { skills: { name: string }[]; mcp: string[] };
  drift: string[];
};
export type Spend = { cap: number; spent: number; held: number; available: number; unknown_charges: number };
export type SpendStatus = { tokens: Spend; dollars: Spend | null };
export type ModelTokens = { route: string; model: string; tokens: number | null; unknown_token_steps: number };
export type Exported = { path: string; files: number; final?: string | null; notes?: string[]; warnings: string[]; broken_links: string[]; missing: string[]; incomplete: string[] };

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauri<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function tauriBytes<T>(cmd: string, bytes: Uint8Array, name: string): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, bytes, { headers: { name: encodeURIComponent(name) } });
}

async function tauriRun<T>(cmd: string, args: Record<string, unknown>, onEvent: (e: RunEvent) => void): Promise<T> {
  const { invoke, Channel } = await import("@tauri-apps/api/core");
  const channel = new Channel<RunEvent>();
  channel.onmessage = onEvent;
  return invoke<T>(cmd, { ...args, onEvent: channel });
}

// ---- mock ----
type MockProject = {
  look?: State["look"];
  /** The results version the person marked reviewed in the preview. */
  reviewed?: string;
  links?: string[];
  archived?: boolean;
  brief: string;
  plan: Plan | null;
  approvedHash: string | null;
  artifacts: Artifact[];
  texts: Record<string, string>;
  review: Record<string, ReviewSummary>;
  round: Record<string, number>;
  budget: { limit: number; perTask: number; spent: number } | null;
  posts: Post[];
  failures?: Record<string, [string, string]>;
  /** Set when the person pressed Stop and no step has started since. */
  stopped?: boolean;
  /** Steps held back, with the reason: the ones waiting for the person's answers. */
  blocked?: Record<string, string>;
  matrix?: Record<string, Record<string, Permission>>;
  writes: WriteItem[];
  sources: Source[];
  selected: { skills: string[]; mcp: string[] };
  notes: Note[];
  rules: string;
  chats: Record<string, ChatMessage[]>;
  coord: Turn[];
};
const mock = { projects: {} as Record<string, MockProject>, current: "", presentation: false, keys: new Set<string>(), projectCap: 5, memory: [] as Note[], sharedTemplates: [] as TemplateSaved[], ownAi: null as null | { address: string; model: string; has_key: boolean }, runningCode: false };
/** The preview's results version and whether it was marked reviewed, as the core reports them. */
const mockReview = (p: MockProject) => {
  const version = hashPlan({ artifacts: p.artifacts } as unknown as Plan);
  const finished = !!p.plan && p.plan.stages.length > 0 && p.plan.stages.every((s) => p.artifacts.some((x) => x.name === s.id));
  return { version, finished, reviewed: finished && p.reviewed === version };
};
/** The preview's stand-in for the approval hash: 64 hex characters that change whenever the plan does. */
const hashPlan = (p: Plan) => { let h = 2166136261; for (const ch of JSON.stringify(p)) { h ^= ch.charCodeAt(0); h = Math.imul(h, 16777619) >>> 0; } return h.toString(16).padStart(8, "0").repeat(8); };
const MODELS: Record<string, string[]> = {
  R1: ["gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-sol", "gpt-6-luna"],
  R2: ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"],
  R3: ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"],
  R5: ["gpt-6.1-terra", "gpt-6-luna"],
  R8: ["grok-4.7"],
  R9: ["gemini-3.6-flash"],
  R10: ["deepseek-v4-pro", "deepseek-v4-flash"],
  R11: ["kimi-k3"],
  R12: ["openrouter/free"],
};
/** A small slice of OpenRouter's list with real prices from Nov 10, 2026, for the design preview. */
const MOCK_CATALOG = (() => {
  const m = (id: string, name: string, i: number | null, o: number | null, ctx: number, free = false, img = false) => ({ id, name, maker: id.split("/")[0], context_length: ctx, input_per_mtok: i, output_per_mtok: o, free, sees_images: img });
  return {
    fetched_at: 1794355200, offline: false,
    recommended: { writing: ["anthropic/claude-haiku-5.5", "openai/gpt-6.1-terra"], checking: ["mistralai/mistral-small-2603", "deepseek/deepseek-v4.1-flash"] },
    models: [
      m("anthropic/claude-haiku-5.5", "Anthropic: Claude Haiku 5.5", 0.1, 0.5, 1000000, false, true),
      m("anthropic/claude-sonnet-5.5", "Anthropic: Claude Sonnet 5.5", 2, 10, 1000000, false, true),
      m("deepseek/deepseek-v4.1-flash", "DeepSeek: V4.1 Flash", 0.05, 0.57, 1048576),
      m("google/gemini-3.8-flash", "Google: Gemini 3.8 Flash", 0.83, 4.12, 1048576, false, true),
      m("meta-llama/llama-4-scout", "Meta: Llama 4 Scout", 0.1, 0.3, 1310720),
      m("meta-llama/llama-4-scout:free", "Meta: Llama 4 Scout (free)", 0, 0, 131072, true),
      m("mistralai/mistral-small-2603", "Mistral: Mistral Small", 0.15, 0.6, 262144),
      m("openai/gpt-6.1-terra", "OpenAI: GPT-6.1 Terra", 1, 5, 1050000, false, true),
      m("openrouter/auto", "Auto Router", null, null, 2000000),
      m("openrouter/free", "Free Models Router", 0, 0, 200000, true),
      m("qwen/qwen3.7-flash", "Qwen: Qwen3.7 Flash", 0.03, 0.13, 1000000),
      m("x-ai/grok-4.3", "xAI: Grok 4.3", 1.25, 2.5, 1000000),
    ],
  };
})();
/** The app's default policy for the design preview: writers may change files and must ask to send, delete or install;
 *  the second check only reads. Running commands is never allowed. */
function mockMatrix(p: MockProject): Record<string, Record<string, Permission>> {
  if (p.matrix) return structuredClone(p.matrix);
  return Object.fromEntries((p.plan?.team ?? []).map((t) => {
    const writes = roleInfo(t.role)?.side !== "checking";
    return [t.role, { read: "allow", artifact: "allow", write: writes ? "allow" : "deny", shell: "deny", external_send: writes ? "ask" : "deny", delete: writes ? "ask" : "deny", dependency_install: writes ? "ask" : "deny" }];
  }));
}
function mockPolicy(p: MockProject): ActionPolicy {
  const matrix = mockMatrix(p);
  const writers = Object.entries(matrix).filter(([role]) => roleInfo(role)?.side !== "checking");
  const guard = (tool: string) => { const v = [...new Set(writers.map(([, r]) => r[tool]))]; return v.length !== 1 ? "varies_by_role" : v[0] === "deny" ? "never" : "ask_each_time"; };
  return { matrix, scope_hash: "mock", counts: { allowed: 0, asked: 0, denied: 0 }, pending: [], assurance: "Guarded",
    guardrails: { send_outside: guard("external_send"), delete_files: guard("delete"), install_software: guard("dependency_install"), run_commands: "never", checkers_read_only: true } } as ActionPolicy;
}

/** The app's own sentences for each limit, so the design preview shows exactly what the app would. */
/** A finished writing project in the browser preview has two missing details until the person answers them or finishes
 *  without them. `?gaps=0` turns this off. The real app counts the `[NEEDED: ...]` markers in the finished text. */
function mockGaps(p: MockProject): { what: string }[] {
  const off = typeof location !== "undefined" && new URLSearchParams(location.search).get("gaps") === "0";
  const finished = !!p.plan && p.plan.stages.every((st) => p.artifacts.some((x) => x.name === st.id));
  if (off || !finished || !p.plan?.stages.some((st) => st.id === "finish")) return [];
  const latest = [...p.notes].reverse().find((n) => n.stage === "finish" && (n.text.startsWith("Details from the person:") || n.text.startsWith("Write around these missing details")));
  return latest?.text.startsWith("Details from the person:") ? [] : [{ what: "the price per seat" }, { what: "the start date" }];
}
const mockAccepted = (p: MockProject) => [...p.notes].reverse().find((n) => n.stage === "finish" && (n.text.startsWith("Details from the person:") || n.text.startsWith("Write around these missing details")))?.text.startsWith("Write around") ?? false;

const MOCK_FAILURES: Record<string, string> = {
  per_minute: "OpenRouter's free models allow 20 requests a minute, and this step hit that limit. It was tried 3 times. Try again in a minute, or choose another model.",
  daily_cap: "Today's free allowance on OpenRouter is used up: 50 requests a day, or 1,000 once the account has bought $10 of credit. Choose another model, or add credit to raise the allowance.",
  upstream_busy: "The service running this model is busy right now. It was tried 3 times. Try again in a minute, or choose another model.",
  out_of_credits: "Your OpenRouter account is out of credit. Add credit, or choose another model.",
  key_limit: "This API key has reached the spending limit set for it. Raise the key's limit with the provider, or connect another key.",
  held: "stopped: OpenRouter did not answer within 5 minutes, so SproutCrew cannot tell whether the request was charged. It counted the most the request could have cost and stopped all AI work, so nothing more is spent until you have checked.",
};
const ROLE_TABLE: Record<string, [string, string, "writing" | "checking"]> = {
  web_researcher: ["Web researcher", "Searches the public web using only your brief. It never sees your files.", "writing"],
  analyst: ["Analyst", "Frames the problem and sets out the options from the files, with the evidence for and against each.", "writing"],
  numbers: ["Numbers check", "Recomputes every figure and unit before anyone builds on them, and says which are wrong or not in the files.", "checking"],
  strategist: ["Strategist", "Turns the checked analysis into one recommendation, a 90-day plan, and the measures that show it is working.", "writing"],
  editor: ["Editor", "Turns the checked work into the document the client reads: plain language, no internal notes, every number confirmed.", "writing"],
  presenter: ["Presenter", "Builds the board deck and the hardest questions a director will ask, each with a short answer.", "writing"],
  researcher: ["Researcher", "Sets out the problem, who has it, the constraints, and what is out of scope, from the brief and the files.", "writing"],
  writer: ["Writer", "Writes the document itself, with numbered requirements and the options that were rejected.", "writing"],
  risk: ["Risk analyst", "Reads the draft with fresh eyes and lists its risks, its open questions, and how success will be measured.", "checking"],
  reader: ["Reader", "Reads every file, quotes what matters, and writes each finding with the file it came from.", "writing"],
  architect: ["Architect", "Writes the specification: what the app does, the commands or screens, the data it keeps, and what it will not do.", "writing"],
  builder: ["Coder", "Writes the code, and later fixes what the security and UX checks found.", "writing"],
  security: ["Security reviewer", "Reads the code as an attacker would: input handling, secrets, files, commands, and dependencies.", "checking"],
  ux: ["UX reviewer", "Uses the app as a first-time user would: the first run, the names of things, and the messages when something goes wrong.", "checking"],
  tester: ["Tester", "Writes tests for the main behavior and the edge cases, runs them, and reports what really happened.", "writing"],
  docs: ["Technical writer", "Writes the README with install, usage, tests, and limitations, describing the code as it now is.", "writing"],
  humaniser: ["Humaniser", "Rewrites the finished document and deck so they read as written by a careful person, without changing a number, a name, a date, or a claim.", "writing"],
  copywriter: ["Copywriter", "Drafts the posts from the brief and the files, and lists the ones that passed review for the outbox.", "writing"],
  facilitator: ["Facilitator", "Frames the decision, merges the two sides' ideas into real options, and writes the decision memo.", "writing"],
  market: ["Market analyst", "Sets out the existing products, what they cost, and how people cope today, from the files and pages it was given.", "writing"],
  customer: ["Customer evidence analyst", "Reads the customer feedback, counts what it says, and says when there is too little to go on.", "checking"],
  ideas_a: ["First idea maker", "Forms its own ideas without seeing the other side's, then argues against the other side's best ones.", "writing"],
  ideas_b: ["Second idea maker", "Forms its own ideas without seeing the other side's, then argues against the other side's best ones.", "checking"],
  dissent: ["Dissent check", "Reads what the merge dropped and restores anything dropped without a good reason.", "checking"],
  reviewer: ["Second check", "Reads everything together, checks it against the files and the brief, and says whether it is ready. It lists what to fix and does not rewrite the work.", "checking"],
};
const roleInfo = (role: string): RoleInfo | null => {
  const r = ROLE_TABLE[role];
  return r ? { role, label: r[0], does: r[1], side: r[2] } : null;
};
// id, name, wants_files, promise, then the steps as [id, title, role].
const CASES: [string, string, boolean, string, [string, string, string][]][] = [
  ["writing", "Writing & editing", true, "A complete piece: outline, draft, edit, check, and final revision.", [["outline", "Shape the piece", "writer"], ["draft", "Write the first draft", "writer"], ["edit", "Edit for voice and clarity", "editor"], ["review", "Check the writing and its claims", "reviewer"], ["finish", "Write the finished piece", "writer"]]],
  ["case_study", "Case study", true, "A client-ready Word document and PDF and a board deck, built from a framed problem, an analysis with its numbers rechecked, and a reviewed recommendation, and checked again in a checking pass before you send it.", [
    ["frame", "Frame the problem", "analyst"], ["analyse", "Analyse the evidence and the options", "analyst"], ["numbers", "Recompute every number", "numbers"],
    ["recommend", "Write the recommendation", "strategist"], ["review", "Review the work before it goes to the client", "reviewer"], ["document", "Write the client document", "editor"],
    ["deck", "Build the board deck", "presenter"],
    ["polish_document", "Remove the signs of machine writing from the document", "humaniser"], ["polish_deck", "Remove the signs of machine writing from the deck", "humaniser"], ["final", "Check the finished document and deck", "reviewer"],
    ["finish_deck", "Write the final deck", "presenter"], ["finish_document", "Write the final document", "editor"]]],
  ["proposal", "Proposal or PRD", true, "A proposal or product requirements document with scope, requirements, risks written by a separate analyst, open questions, and success measures, checked in a checking pass.", [
    ["scope", "Summarise the problem and the constraints", "researcher"], ["draft", "Write the proposal", "writer"], ["risks", "List risks, open questions, and success measures", "risk"], ["review", "Review the proposal", "reviewer"], ["finish", "Write the final proposal", "writer"]]],
  ["research_brief", "Research brief", true, "Findings drawn only from your files, each tied to the file it came from, written up as a short brief and checked claim by claim in a checking pass.", [
    ["read", "Read the files and list what each says", "reader"], ["findings", "Write the findings", "reader"], ["brief", "Write the brief", "writer"], ["review", "Check every claim against the files", "reviewer"], ["finish", "Write the final brief", "writer"]]],
  ["app_development", "App development", false, "A specification, the code, a security review and a UX review that the coder then acts on, tests that ran, and a README with install, usage, tests, and limitations, reviewed in a checking pass.", [
    ["spec", "Write the specification", "architect"], ["attack_spec", "Find what is wrong with the specification", "reviewer"], ["build", "Build the app", "builder"], ["security", "Check the code for security problems", "security"], ["ux", "Check the app as a first-time user", "ux"],
    ["fix", "Fix what the security and UX checks found", "builder"], ["test", "Write and run the tests", "tester"], ["document", "Write the README", "docs"], ["review", "Review the build and the README", "reviewer"]]],
  ["social_publishing", "Social posts", false, "Full creative for each post, written for each platform you name (the text within that platform's limit, hashtags, visual direction with image size, and alt text), checked in a separate pass and ready to copy into your scheduling tool or a Notion page. Turn on web research to check each platform's current posting rules first. SproutCrew does not post anything.", [
    ["draft", "Draft the posts", "copywriter"], ["review", "Review the drafts for accuracy and tone", "reviewer"], ["queue", "Write the final posts, ready to copy", "copywriter"]]],
  ["brainstorm", "Brainstorm", true, "A decision memo on a decision or idea: two separate passes form their own ideas without seeing each other's, argue against each other, and a checking pass checks the recommendation against your files and the pages you gave it. Web search is not on yet, so add your market research as files or addresses.", [
    ["frame", "Frame the decision", "facilitator"], ["market", "Set out the market and existing products", "market"], ["customers", "Set out what customers say", "customer"], ["ideas_a", "Form the first side's ideas", "ideas_a"], ["ideas_b", "Form the second side's ideas", "ideas_b"],
    ["attack_b", "Argue against the second side's ideas", "ideas_a"], ["attack_a", "Argue against the first side's ideas", "ideas_b"], ["options", "Merge the ideas into real options", "facilitator"], ["dissent", "Check nothing good was dropped", "dissent"], ["memo", "Write the decision memo", "facilitator"], ["review", "Check the memo against the evidence", "reviewer"], ["finish", "Write the final memo", "facilitator"]]],
  ["quick_brainstorm", "Quick brainstorm", true, "A short decision memo in four steps: the decision framed, ideas from your files, a challenge from a second AI on the checking side, and a recommendation that answers it. Faster and cheaper than the full brainstorm, with one set of ideas instead of two.", [
    ["frame", "Frame the decision", "facilitator"], ["ideas", "Set out the options", "ideas_a"], ["review", "Challenge the options", "reviewer"], ["memo", "Write the decision memo", "facilitator"]]],
];
const USE_CASES: UseCase[] = CASES.map(([id, name, wants_files, promise, steps]) => ({
  id, name, wants_files, promise,
  steps: steps.map((x) => x[1]),
  owners: steps.map((x) => x[2]),
  team: [...new Set(steps.map((x) => x[2]))].map((r) => roleInfo(r)!),
}));
// Steps that wait for something other than the step before them, as the real workflows set them.
const AFTER: Record<string, Record<string, string[]>> = {
  app_development: { security: ["build"], ux: ["build"], fix: ["security", "ux"] },
  case_study: { polish_document: ["deck"], polish_deck: ["deck"], final: ["polish_document", "polish_deck"], finish_deck: ["final"], finish_document: ["final"] },
};
const STEP_IDS: Record<string, string[]> = Object.fromEntries(CASES.map(([id, , , , steps]) => [id, steps.map((x) => x[0])]));
const nextSourceId = (sources: Source[]) => `src-${sources.reduce((max, source) => Math.max(max, Number(source.id.replace(/^src-/, '')) || 0), 0) + 1}`;
const OK_TYPES = ["txt", "md", "csv", "json", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "html", "htm", "png", "jpg", "jpeg", "tif", "tiff", "heic", "gif", "bmp", "webp"];
function mockAddSource(name: string, bytes: number): Added {
  const p = mock.projects[mock.current];
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  if (!OK_TYPES.includes(ext)) return { added: [], skipped: [{ name, reason: `${name}: .${ext} is not a type BFF can read yet. Use txt, md, csv, json, pdf, doc, docx, xls, xlsx, ppt, pptx, html, or a picture (png, jpg, tiff, heic, gif, bmp, webp)` }] };
  const have = p.sources.find((x) => x.name === name && x.bytes === bytes);
  const src = have ?? { id: nextSourceId(p.sources), name, bytes, pages: 1, flags: [], suspicious: /inject|ignore/i.test(name) ? ["tells the AI to ignore or replace its instructions"] : [], origin: "" };
  if (!have) p.sources.push(src);
  return { added: [src], skipped: [] };
}
const mockTokens = { cap: 500000, spent: 320000, held: 0, available: 180000, unknown_charges: 0 };
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const scrub = (t: string) => (mock.presentation ? t.replace(/[\w.+-]+@[\w-]+\.[\w.]+/g, "[email]").replace(/\/Users\/[^/\s]+/g, "~") : t);

const MOCK_STARTERS: TemplateSaved[] = [
  { name: "Case study", from_id: "starter", from_name: "Case study", has_logo: false, look: null, outline: ["Summary", "The situation", "What the evidence shows", "Options", "Recommendation", "Risks and how to manage them", "Next 90 days"], style: "Lead with the answer. Write for a busy executive: short sentences, one idea per paragraph, every number with its source." },
  { name: "Proposal", from_id: "starter", from_name: "Proposal", has_logo: false, look: null, outline: ["Value proposition", "Economics", "Goals", "Non-goals", "Journeys", "Requirements", "Acceptance criteria", "Metrics", "Rollout", "Risks"], style: "Be specific and testable. Write as the team proposing it: we and our." },
  { name: "Research brief", from_id: "starter", from_name: "Research brief", has_logo: false, look: null, outline: ["The answer", "What the sources show", "Where they disagree", "What is still unknown", "Sources"], style: "One page for a busy reader. Answer first." },
];

/** Usage rows for the sample projects: one per finished step, the third with no reported figures. */
function mockUsage(n: number, plan: Plan | null): UsageRow[] {
  const now = Math.floor(Date.now() / 1000);
  return Array.from({ length: n }, (_, i) => {
    const st = plan?.stages[i];
    const unknown = i === 2;
    return { at: now - (n - i) * 3600 * 20, task: `t${i + 1}`, stage: st?.id ?? null, role: st?.owner_role ?? null, route: "R12", model: i % 2 ? "mistralai/mistral-small-2603" : "anthropic/claude-haiku-5.5", tokens: unknown ? null : 1800 + i * 400, dollars: unknown ? null : 0.002 + i * 0.001, dollars_status: unknown ? "unknown" : "reported" };
  });
}

function mockRoutes(): ApiRoute[] {
  const own: ApiRoute = mock.ownAi ? { route: "R13", state: "ready", ...mock.ownAi } : { route: "R13", state: "not_configured" };
  return [...["R3", "R5", "R8", "R9", "R10", "R11", "R12"].map((route): ApiRoute => ({ route, state: mock.keys.has(route) ? "ready" : route === "R10" ? "no_key" : "not_configured" })), own];
}

function mockState(engine: string): State {
  const p = mock.projects[mock.current];
  if (!p) throw "no project is open";
  const gate = !p.plan || !p.approvedHash ? "not_approved" : p.approvedHash === hashPlan(p.plan) ? "allowed" : "plan_changed";
  const n = p.artifacts.length;
  const budget = p.budget
    ? { limit: p.budget.limit, spent: p.budget.spent, available: Math.max(0, p.budget.limit - p.budget.spent), paused: p.budget.spent >= p.budget.limit, unknown_charges: n }
    : null;
  return {
    will_skip: (p.plan?.stages ?? []).filter((st) => (st.after ?? []).length > 0 && (st.after ?? []).every((a) => !p.plan!.stages.some((x) => x.id === a))).map((st) => ({ id: st.id, title: st.title, missing: st.after ?? [] })),
    open: true, name: mock.current, brief: scrub(p.brief), sources: structuredClone(p.sources), plan: structuredClone(p.plan),
    role_info: Object.fromEntries((p.plan?.team ?? []).flatMap((t) => { const r = roleInfo(t.role); return r ? [[t.role, r]] : []; })),
    problems: [], gate,
    web_research: !!p.plan?.stages.some((x) => x.id === "web_research"),
    humaniser: !!p.plan?.stages.some((x) => x.id === "polish_finish"),
    can_humanise: ["finish", "polish_finish"].includes(p.plan?.stages[p.plan.stages.length - 1]?.id ?? ""),
    code_files: p.artifacts.some((x) => x.name === "build") ? [{ path: "README.md", bytes: 4757 }, { path: "app.js", bytes: 16561 }, { path: "index.html", bytes: 2723 }] : [],
    app: p.artifacts.some((x) => x.name === "build") ? { page: true, opens: true, why: null } : { page: false, opens: false, why: null },
    links: { from: (p.links ?? []).filter((n) => mock.projects[n] && !mock.projects[n].archived), to: Object.keys(mock.projects).filter((n) => !mock.projects[n].archived && mock.projects[n].links?.includes(mock.current)) },
    approved_by: p.approvedHash ? "Preview user" : null, approval_hash: p.plan ? hashPlan(p.plan) : null, result_review: mockReview(p), artifacts: structuredClone(p.artifacts),
    steps: p.artifacts.map((a) => ({ step: a.from_task, tokens: "unknown", plan_window: "unknown" })),
    meters: { tokens: n ? "905858" : "unknown", plan_window: n ? "claude 27%, codex 2%" : "unknown", api_dollars: "unknown" }, engine,
    review: n ? p.review : {}, budget,
    usage_rows: mockUsage(p.artifacts.length, p.plan),
    stopped: p.stopped ? { after_stage: null } : null,
    look: p.look ?? null,
    project_limit: { cap: mock.projectCap, spent: n ? 0.0118 : 0, held: 0, available: mock.projectCap - (n ? 0.0118 : 0), unknown_charges: n ? 1 : 0, locked: null },
    timings: Object.fromEntries(p.artifacts.map((a) => [a.from_task, { seconds: 0.5, route: "R1", model: "replay" }])),
    cost_by_route: n ? { R1: { tokens: null, api_dollars: null, unknown_token_steps: n } } : {},
    tokens_by_model: n ? [{ route: "R1", model: "gpt-6.1-terra", tokens: null, unknown_token_steps: n }] : [],
    outbox: structuredClone(p.posts),
    writes: structuredClone(p.writes),
    notes: structuredClone(p.notes),
    rules: p.rules ? { text: p.rules, at: 0, sha256: "mock" } : null,
    policy: p.plan ? mockPolicy(p) : null,
    failures: Object.fromEntries(Object.entries(p.failures ?? {}).map(([k, [why]]) => [k, why])),
    failure_kinds: Object.fromEntries(Object.entries(p.failures ?? {}).filter(([, [, kind]]) => kind).map(([k, [, kind]]) => [k, kind])),
    // The preview's review step asks for a verdict when the plan has a finish step after it, as the real ones do.
    checks: p.plan?.stages.some((x) => x.id === "finish") && p.artifacts.some((x) => x.name === "review")
      ? { review: { verdict: "fix_first", flagged: MOCK_FLAGGED } }
      : {},
    blocked: structuredClone(p.blocked ?? {}),
  };
}

/** What the check of what is missing asks in the preview: one answer found in a file, one not. */
const MOCK_QUESTIONS: Question[] = [
  { ask: "What does the dough class cost?", from: "pricing-notes.txt", suggested: "$45 a person" },
  { ask: "When does the next class start?", from: null, suggested: null },
];

const MOCK_FLAGGED = ["\"Starting next month\" has no date in the brief. Say \"soon\".", "The address is not in the brief. Remove it.", "The price is not in any source. Cut it."];

const SAMPLE_FINDINGS = [
  { id: "a", claim: "The monthly total should be $20, not $25.", evidence: "Line 4: 4 licenses at $5 each.", decision: null, reason: null },
  { id: "b", claim: "The note does not say when the pilot starts.", evidence: "No date appears in the text.", decision: null, reason: null },
] as Finding[];

async function mockCall<T>(cmd: string, a: Record<string, unknown>): Promise<T> {
  const p = mock.projects[mock.current];
  switch (cmd) {
    case "app_info": return { codex_found: false, swept_orphans: 0, built_at: null, version: "2.0.0" } as T;
    case "open_link": return undefined as T;
    case "check_update": return { available: false, set_up: true } as T;
    case "install_update": throw "SproutCrew is up to date";
    case "list_projects": return Object.keys(mock.projects).filter((n) => !mock.projects[n].archived).sort() as T;
    case "archive_project":
      if (!mock.projects[a.name as string]) throw "no such project";
      mock.projects[a.name as string].archived = true;
      if (mock.current === a.name) mock.current = "";
      return undefined as T;
    case "restore_project":
      if (!mock.projects[a.name as string]) throw "no such project";
      mock.projects[a.name as string].archived = false;
      return undefined as T;
    case "create_project_from": {
      const original = mock.projects[String(a.from)];
      const name = String(a.name);
      if (!original) throw "the original project is missing";
      if (!/^[A-Za-z0-9_-]{1,64}$/.test(name) || Object.keys(mock.projects).some((n) => n.toLowerCase() === name.toLowerCase())) throw "choose a new project name";
      const ids = a.sourceIds as string[];
      if (new Set(ids).size !== ids.length || ids.some((id) => !original.sources.some((source) => source.id === id))) throw "a source is missing or repeated";
      const sources = structuredClone(original.sources.filter((source) => ids.includes(source.id)));
      await mockCall("create_project", { name, brief: a.brief, allowKeyShape: a.allowKeyShape });
      mock.projects[name].sources = sources;
      return undefined as T;
    }
    case "create_project":
      if (!a.allowKeyShape && (findKeyShape(String(a.brief ?? "")) || findKeyShape(String(a.name ?? ""), true))) throw `${KEY_IN_BRIEF} This looks like a key. Keys go in the Connect box, not the brief.`;
      mock.projects[a.name as string] = { brief: a.brief as string, plan: null, approvedHash: null, artifacts: [], texts: {}, review: {}, round: {}, budget: null, posts: [], writes: [], sources: [], selected: { skills: [], mcp: [] }, rules: "", notes: [], chats: {}, coord: [] };
      mock.current = a.name as string;
      return undefined as T;
    case "open_project":
      if (!mock.projects[a.name as string]) throw "no such project";
      mock.current = a.name as string;
      return undefined as T;
    case "get_state": return mockState(a.engine as string) as T;
    case "save_plan": {
      const next = a.plan as Plan;
      if (next.privacy === "no_retention") {
        const why = next.stages.flatMap((st) => {
          const route = st.uses?.route ?? next.team.find((t) => t.role === st.owner_role)?.route;
          return [
            ...(route && route !== "R12" ? [`step ${st.id} runs on ${route}, which cannot promise to keep nothing for one request. Move it to OpenRouter, or allow providers to keep requests`] : []),
            ...(st.id === "web_research" ? [`step ${st.id} searches the web, and OpenRouter's promise to keep nothing does not cover its search. Turn web research off, or allow providers to keep requests`] : []),
          ];
        });
        if (why.length) throw why;
      }
      p.plan = next; return undefined as T;
    }
    case "recommended_skills": {
      const roles = a.roles as string[];
      const all: Recommended[] = [
        { skill: "frontend-design", label: "Frontend design", why: "Gives the interface a clear visual direction and avoids a templated look.", roles: ["builder", "ux"], on: true },
        { skill: "minimalist-ui", label: "Minimalist UI", why: "Clean layouts with restrained type, color, and spacing.", roles: ["builder", "ux"], on: true },
        { skill: "high-end-visual-design", label: "High end visual design", why: "A richer, agency-style look. Choose this or the minimalist style, not both.", roles: ["builder"], on: false },
        { skill: "sharp-problem-test", label: "Problem check", why: "Is this problem painful enough to solve?", roles: ["strategist", "analyst", "ideas_a", "ideas_b"], on: true },
        { skill: "five-dim-customer", label: "Customer profile", why: "Who exactly this is for.", roles: ["analyst", "customer"], on: true },
        { skill: "slc-or-mvp", label: "First version scope", why: "What the smallest useful version includes.", roles: ["strategist", "facilitator"], on: true },
        { skill: "vet-a-feature", label: "Feature check", why: "Whether a new feature is worth building.", roles: ["ideas_a", "ideas_b"], on: true },
      ];
      return all.map((r) => ({ ...r, roles: r.roles.filter((x) => roles.includes(x)) })).filter((r) => r.roles.length) as T;
    }
    case "set_web_research": {
      if (!p.plan) throw "there is no plan";
      const stages = p.plan.stages.filter((x) => x.id !== "web_research");
      const team = p.plan.team.filter((x) => x.role !== "web_researcher");
      p.plan = a.on ? { ...p.plan, stages: [{ id: "web_research", title: "Research the public web", owner_role: "web_researcher" }, ...stages], team: [...team, { role: "web_researcher", route: "R12", model: "anthropic/claude-haiku-5.5", effort: "medium" }] } : { ...p.plan, stages, team };
      return undefined as T;
    }
    case "set_stand_ins": if (p.plan) p.plan = { ...p.plan, fallbacks: a.on ? p.plan.team.filter((t) => t.role !== "reviewer" && t.role !== "numbers").map((t) => ({ ...t, route: t.route === "R1" ? "R2" : "R1", model: t.route === "R1" ? "claude-sonnet-5-5" : "gpt-6.1-terra" })) : [] }; return undefined as T;
    case "mark_reviewed": {
      if (a.project !== mock.current) throw `This decision is for ${a.project}, but ${mock.current} is open. Nothing was decided.`;
      const r = mockReview(p);
      if (!r.finished) throw "Nothing is finished yet, so there is nothing to review.";
      if (a.version !== r.version) throw "The results changed after you looked at them. Nothing was marked. Look again, then mark them reviewed.";
      p.reviewed = r.version; return undefined as T;
    }
    case "approve": {
      if (!p.plan) throw "there is no plan to approve";
      if (a.project !== mock.current) throw `This approval is for ${a.project}, but ${mock.current} is open. Nothing was approved.`;
      const now = hashPlan(p.plan);
      if (a.hash !== now) throw `The plan changed after you read it. You read ${a.hash ? "version " + String(a.hash).slice(0, 4) + " " + String(a.hash).slice(4, 8) : "no version"}, and it is now version ${now.slice(0, 4)} ${now.slice(4, 8)}. Nothing was approved and nothing ran. Read it again before you approve.`;
      p.approvedHash = now; return undefined as T;
    }
    case "read_artifact": return scrub((p.texts[a.file as string] ?? (() => { throw "no such artifact"; })())) as T;
    case "check_assignment": {
      if (a.route === "R13") { if (!/^[A-Za-z0-9._:/@+-]{1,128}$/.test(String(a.model))) throw `${JSON.stringify(a.model)} is not a usable model name. Type it exactly as your service lists it, such as llama3.3.`; return undefined as T; }
      const m = MODELS[a.route as string];
      if (!m) throw `Route ${JSON.stringify(a.route)} is not available.`;
      if (/astra|fable/i.test(a.model as string)) throw `${a.model} is excluded and cannot be assigned. Choose another model.`;
      if (a.route !== "R12" && !m.includes(a.model as string)) throw `${a.model} is not a known model on ${a.route}. Available: ${m.join(", ")}.`;
      return undefined as T;
    }
    case "route_models": return (a.route === "R13" ? (mock.ownAi ? [mock.ownAi.model] : []) : MODELS[a.route as string] ?? []) as T;
    case "default_reviewer": return [a.route, a.route === "R3" ? "claude-sonnet-5-5" : a.route === "R5" ? "gpt-6-luna" : "mistralai/mistral-small-2603", "high"] as T;
    case "model_catalog": await new Promise((r) => setTimeout(r, 1000)); return MOCK_CATALOG as T; // a slow first fetch, to see the waiting states
    case "cited_pages": return [] as T;
    case "export_log": return "~/Library/Application Support/app.sproutcrew.mac/log/sproutcrew-log.txt" as T;
    case "plan_preflight": {
      await new Promise((r) => setTimeout(r, 1000)); // the app waits for model prices before it answers
      // The design preview prices each step at 60,000 tokens in and 8,000 out, like the app's OpenRouter bounds.
      const team = p.plan?.team ?? [];
      const stages = p.plan?.stages ?? [];
      const models: [string, number][] = [];
      let max: number | null = 0, free = 0;
      for (const st of stages) {
        const t = st.uses ?? team.find((x) => x.role === st.owner_role);
        if (!t) { max = null; continue; }
        const row = models.find(([m]) => m === t.model);
        if (row) row[1] += 1; else models.push([t.model, 1]);
        const m = MOCK_CATALOG.models.find((x) => x.id === t.model);
        if (t.model === "openrouter/free" || t.model.endsWith(":free")) { free += 1; continue; }
        if (!m || m.input_per_mtok === null || m.output_per_mtok === null) { max = null; continue; }
        if (max !== null) max += (m.input_per_mtok * 60000 + m.output_per_mtok * 8000) / 1e6;
      }
      const left = 8;
      return { steps: stages.length, models, free_requests: free, free_remaining: free ? left : null, free_short: free > left, estimated_usd: max === null ? null : max * 0.4, max_usd: max } as T;
    }
    case "write_stress_report": return undefined as T;
    case "set_presentation": mock.presentation = a.on as boolean; return undefined as T;
    case "set_budget": p.budget = { limit: a.limit as number, perTask: a.perTask as number, spent: p.budget?.spent ?? 0 }; return undefined as T;
    case "template_preview": { const src = p.sources.find((x) => x.id === a.sourceId); if (!src) throw `there is no source "${a.sourceId}"`; const office = /\.(docx|pptx)$/i.test(src.name); return { name: src.name.replace(/\.[^.]+$/, ""), from_id: src.id, from_name: src.name, has_logo: office, look: office ? { font: "Source Sans 3", accent: "1F6F43", heading_font: "Georgia", body_pt: 10.5, margins_mm: [25, 19, 25, 19], page_mm: [216, 279] } : null, outline: ["Executive summary", "Background", "Findings", "Recommendation"], style: "Sentences average about 18 words; keep them of medium length. Write as the organisation: we and our. Use US spelling (color, organize, center)." } as T; }
    case "use_template": { const t = await mockCall<TemplateSaved>("template_preview", { sourceId: a.sourceId }); if (!a.look) { t.look = null; t.has_logo = false; } if (!a.outline) t.outline = []; if (!a.style) t.style = ""; p.look = t; return structuredClone(t) as T; }
    case "edit_template": { if (!p.look) throw "there is no template to change"; p.look = { ...p.look, name: a.name as string, outline: (a.outline as string[]).map((x) => x.trim()).filter(Boolean), style: a.style as string, look: a.keepLook ? p.look.look : null, has_logo: a.keepLook ? p.look.has_logo : false }; return structuredClone(p.look) as T; }
    case "templates": return { starters: MOCK_STARTERS.map((t) => ({ id: t.from_name.toLowerCase().replace(/ /g, "_"), template: t })), shared: mock.sharedTemplates.map((t) => ({ id: t.name.toLowerCase().replace(/[^a-z0-9]+/g, "-"), template: t })) } as T;
    case "use_starter": { const t = MOCK_STARTERS.find((x) => x.from_name.toLowerCase().replace(/ /g, "_") === a.id); if (!t) throw "there is no such starter"; p.look = structuredClone(t); return structuredClone(t) as T; }
    case "share_template": { if (!p.look) throw "there is no template to save"; if (!p.look.name) throw "give the template a name first"; mock.sharedTemplates = [...mock.sharedTemplates.filter((x) => x.name !== p.look!.name), structuredClone(p.look)]; return p.look.name as T; }
    case "use_shared_template": { const t = mock.sharedTemplates.find((x) => x.name.toLowerCase().replace(/[^a-z0-9]+/g, "-") === a.id); if (!t) throw "there is no such template"; p.look = structuredClone(t); return structuredClone(t) as T; }
    case "delete_shared_template": mock.sharedTemplates = mock.sharedTemplates.filter((x) => x.name.toLowerCase().replace(/[^a-z0-9]+/g, "-") !== a.id); return undefined as T;
    case "clear_template_look": p.look = null; return undefined as T;
    case "notify_run": return false as T;
    case "save_result_to_folder": { if (!(a.folder as string).trim()) throw "type a folder"; return `${(a.folder as string).replace(/\/$/, "")}/${mock.current} - result.md` as T; }
    case "reliability": return Object.keys(mock.projects).length ? [{ outcome: "Writing & editing", writer: "anthropic/claude-haiku-5.5", checker: "mistralai/mistral-small-2603", projects: Object.keys(mock.projects).length, finished: 0, stopped: 0, findings: 3, accepted: 1, stand_ins: 0 }] as T : [] as T;
    case "set_notification_names": return { enabled: true, count: 0, errors: 0, last_error: null, open_inbox: false, names: !!a.on } as T;
    case "memory_list": return structuredClone(mock.memory) as T;
    case "memory_save": { const t = (a.text as string).trim(); if (!t) throw "a note cannot be empty"; const id = a.replaceId as string | null; if (id) { const n = mock.memory.find((x) => x.id === id); if (!n) throw `there is no note "${id}"`; n.text = t; } else mock.memory.push({ id: `g${mock.memory.length + 1}`, stage: "", text: t, at: Date.now() }); return undefined as T; }
    case "memory_remove": mock.memory = mock.memory.filter((x) => x.id !== a.id); return undefined as T;
    case "set_humaniser": {
      if (!p.plan) throw ["there is no plan"];
      const on = a.on as boolean;
      const has = p.plan.stages.some((x) => x.id === "polish_finish");
      if (on && !has) {
        const writer = p.plan.team.find((t) => t.role === p.plan!.stages.find((x) => x.id === "finish")?.owner_role);
        if (!writer) throw ["This plan does not end with a step that writes the finished text, so there is nothing to polish."];
        p.plan.stages.push({ id: "polish_finish", title: "Remove the signs of machine writing", owner_role: "humaniser", guide: "", after: [] });
        if (!p.plan.team.some((t) => t.role === "humaniser")) p.plan.team.push({ ...writer, role: "humaniser" });
      }
      if (!on) { p.plan.stages = p.plan.stages.filter((x) => x.id !== "polish_finish"); p.plan.team = p.plan.team.filter((t) => t.role !== "humaniser"); }
      return undefined as T;
    }
    case "export_usage_csv": return "~/Library/Application Support/app.sproutcrew.mac/projects/sample/exports/usage-1.csv" as T;
    case "policy_overview": {
      const rows = Object.entries(mock.projects).filter(([, x]) => !x.archived).map(([name, x]) => {
        if (!x.plan) return { name, plan: false };
        const g = mockPolicy(x).guardrails!;
        const d = mockDefaults;
        const looser: string[] = [], stricter: string[] = [];
        for (const k of ["send_outside", "delete_files", "install_software"] as const) {
          const want = d[k], has = g[k];
          if (want === "never" && has !== "never") looser.push(k); else if (want === "ask" && has !== "ask_each_time") stricter.push(k);
        }
        const pv = x.plan.privacy ?? "standard";
        if (pv === "standard" && d.privacy === "no_retention") looser.push("privacy"); else if (pv === "no_retention" && d.privacy === "standard") stricter.push("privacy");
        const approved = !!x.approvedHash && x.approvedHash === hashPlan(x.plan);
        return { name, plan: true, guardrails: g, privacy: pv, fit: { looser, stricter }, approved, hash: approved ? `${x.approvedHash!.slice(0, 4)} ${x.approvedHash!.slice(4, 8)}` : null };
      });
      return { defaults: { ...mockDefaults }, projects: rows, errors: [] } as T;
    }
    case "set_policy_defaults": {
      const names = a.tighten as string[];
      for (const n of names) if (!mock.projects[n]) throw `There is no project named ${n}. Nothing was saved.`;
      mockDefaults = { ...(a.defaults as typeof mockDefaults) };
      const changed: string[] = [];
      for (const n of names) {
        const x = mock.projects[n];
        if (!x.plan) continue;
        const m = mockMatrix(x);
        let did = false;
        for (const [k, tool] of [["send_outside", "external_send"], ["delete_files", "delete"], ["install_software", "dependency_install"]] as const) {
          if (mockDefaults[k] !== "never") continue;
          for (const role of Object.keys(m)) if (roleInfo(role)?.side !== "checking" && m[role][tool] !== "deny") { m[role][tool] = "deny"; did = true; }
        }
        if (did) { x.matrix = m; x.approvedHash = null; changed.push(n); }
      }
      return changed as T;
    }
    case "usage_everywhere": return { projects: Object.entries(mock.projects).map(([name, x]) => ({ project: name, rows: mockUsage(x.artifacts.length, x.plan) })), unreadable: [] } as T;
    case "set_project_limit": case "raise_project_limit": {
      const d = a.dollars as number;
      if (!Number.isFinite(d) || d <= 0) throw "Enter an amount in dollars, like 0.50.";
      const floor = p.artifacts.length ? 0.0118 : 0;
      if (d < floor) throw `You've already spent ${Math.ceil(floor * 100)}¢ on this project, so the limit can't go below that.`;
      mock.projectCap = d;
      return undefined as T;
    }
    case "raise_budget": if (!p.budget || (a.newLimit as number) <= p.budget.limit) throw "the new limit must be above the current one"; p.budget.limit = a.newLimit as number; return undefined as T;
    case "decide": {
      const f = p.review[a.artifact as string]?.findings.find((x) => x.id === a.finding);
      if (!f) throw "there is no such finding";
      if (a.decision === "reject" && !String(a.reason).trim()) throw `a rejection needs a reason (${a.finding})`;
      f.decision = a.decision as "accept" | "reject";
      f.reason = a.decision === "reject" ? (a.reason as string) : null;
      return undefined as T;
    }
    case "preflight":
      return {
        can_start: mock.keys.size > 0,
        checks: [
          { name: "API connection preview", status: mock.keys.size ? "pass" : "fail", detail: "Design preview only. No provider request or Keychain check was made." },
          { name: "Subscription connections", status: "pass", detail: "Disabled in the public beta." },
          { name: "Export tools and disk", status: "warn", detail: "These checks require the native Mac app." },
        ],
      } as T;
    case "tools":
      return [
        { name: "claude", path: "/Users/you/.local/bin/claude", version: "2.1.316 (Claude Code)", chosen: null },
        { name: "codex", path: null, version: null, chosen: null },
      ] as T;
    case "choose_tool":
      if (!String(a.path ?? "").trim()) return [{ name: "claude", path: "/Users/you/.local/bin/claude", version: "2.1.316 (Claude Code)", chosen: null }, { name: "codex", path: null, version: null, chosen: null }] as T;
      if (!String(a.path).startsWith("/")) throw "Give the full path, starting with /";
      return [{ name: "claude", path: "/Users/you/.local/bin/claude", version: "2.1.316 (Claude Code)", chosen: null }, { name: "codex", path: a.path, version: "codex-cli 0.166.0", chosen: a.path }] as T;
    case "attention_status":
      // The sample queue under ?board has seven items and one project it could not read, as the board shows.
      return { enabled: false, count: new URLSearchParams(window.location.search).has("board") ? 7 : 0, errors: 0, last_error: null, open_inbox: false } as T;
    case "set_notifications":
    case "test_notification":
      throw "Mac notifications are available in the native app; this is a browser preview.";
    case "openrouter_limits":
      throw "Check allowances in the native app. The browser preview does not contact your account.";
    case "api_routes":
      return mockRoutes() as T;
    case "save_own_ai": {
      const addr = String(a.address ?? "").trim().replace(/\/+$/, "").replace(/\/chat\/completions$/, "");
      if (!/^https:\/\/[a-z0-9.-]+(:\d+)?(\/[A-Za-z0-9/._~-]*)?$/i.test(addr) && !/^http:\/\/(localhost|127\.0\.0\.1):\d+(\/[A-Za-z0-9/._~-]*)?$/i.test(addr)) throw "The address must start with https://, so what is sent is encrypted. Plain http:// is allowed only for an app on this Mac, such as http://localhost:11434. It usually looks like https://api.together.xyz/v1 or, for an app on this Mac, http://localhost:11434/v1.";
      if (!/^[A-Za-z0-9._:/@+-]{1,128}$/.test(String(a.model ?? "").trim())) throw "Type the model's name exactly as the service lists it, such as llama3.3 or qwen3:8b.";
      mock.ownAi = { address: addr, model: String(a.model).trim(), has_key: !!String(a.key ?? "").trim() };
      return { routes: mockRoutes(), models: [...new Set([String(a.model).trim(), "llama3.3", "qwen3:8b"])] } as T;
    }
    case "remove_own_ai":
      mock.ownAi = null;
      return mockRoutes() as T;
    case "save_key":
      if (!/^[A-Za-z0-9._-]{16,512}$/.test(String(a.key ?? "").trim())) throw "That does not look like an API key. Paste the whole key, with no spaces.";
      if (String(a.key).includes("bad")) throw "Anthropic refused this key. Check you copied all of it, and that it has not been revoked.";
      mock.keys.add(String(a.route));
      return mockRoutes() as T;
    case "remove_key":
      mock.keys.delete(String(a.route));
      return mockRoutes() as T;
    case "discover_tools":
      return {
        skills: [
          { name: "review", description: "Review a change for correctness", source: "claude-user" },
          { name: "plan", description: "Make a plan before writing code", source: "codex-user" },
        ],
        mcp: [{ name: "github", source: "codex", transport: "unknown" }],
        truncated: false,
        selected: { skills: p.selected.skills.map((name) => ({ name })), mcp: p.selected.mcp },
        drift: [],
      } as T;
    case "select_tool": (a.kind === "skill" ? p.selected.skills : p.selected.mcp).push(a.name as string); return undefined as T;
    case "deselect_tool":
      p.selected.skills = p.selected.skills.filter((n) => n !== a.name);
      p.selected.mcp = p.selected.mcp.filter((n) => n !== a.name);
      return undefined as T;
    case "decided_list": {
      // The preview's Decided: the approvals and reviews made in it, newest first. Under ?board, a sample.
      if (new URLSearchParams(window.location.search).has("board")) {
        const t = Math.floor(Date.now() / 1000);
        return { items: [
          { project: "larkspur-case-study", at: t - 1500, kind: "plan.approved", what: "Approved the plan", by: "Preview user", version: "4be077c1" },
          { project: "harbor-lane-article", at: t - 7200, kind: "result.reviewed", what: "Marked the results reviewed", by: "Preview user", version: "2a6e4471" },
          { project: "tide-chart-app", at: t - 86400, kind: "review.decided", what: "Rejected finding f2 on build (No source)", by: null, version: null },
        ], errors: [] } as T;
      }
      const items = Object.entries(mock.projects).filter(([, pr]) => !pr.archived).flatMap(([name, pr]) => [
        ...(pr.approvedHash ? [{ project: name, at: Math.floor(Date.now() / 1000) - 60, kind: "plan.approved", what: "Approved the plan", by: "Preview user", version: pr.approvedHash }] : []),
        ...(pr.reviewed ? [{ project: name, at: Math.floor(Date.now() / 1000), kind: "result.reviewed", what: "Marked the results reviewed", by: "Preview user", version: pr.reviewed }] : []),
      ]).sort((x, y) => y.at - x.at);
      return { items, errors: [] } as T;
    }
    case "decision_inbox": {
      // Opened with ?board, the preview seeds sample projects and lists what waits in them, so each decision can be made.
      if (!new URLSearchParams(window.location.search).has("board")) return { items: [], errors: [] } as T;
      await seedBoard();
      const items: unknown[] = [];
      for (const [name, pr] of Object.entries(mock.projects)) {
        if (pr.archived || !pr.plan) continue;
        const done = pr.plan.stages.filter((st) => pr.artifacts.some((x) => x.name === st.id)).length;
        const approved = !!pr.approvedHash && pr.approvedHash === hashPlan(pr.plan);
        if (!approved && done < pr.plan.stages.length) items.push({ project: name, stage: pr.plan.stages[0].id, id: "plan", tier: "consequential", kind: "plan", title: "Review and approve the current plan and permissions", anchor: "plan-approval", step_title: "" });
        for (const [st, [why]] of Object.entries(pr.failures ?? {})) items.push({ project: name, stage: st, id: `failed-${st}`, tier: "consequential", kind: "failure", title: `A step stopped: ${why}`, anchor: `step-${st}`, step_title: pr.plan.stages.find((x) => x.id === st)?.title ?? "" });
        if (pr.stopped && approved) items.push({ project: name, stage: "", id: "stopped", tier: "reversible", kind: "stopped", title: `Stopped by you after step ${done} of ${pr.plan.stages.length}`, anchor: "run-status", step_title: "" });
        for (const [art, r] of Object.entries(pr.review)) for (const f of r.findings.filter((x) => !x.decision)) items.push({ project: name, stage: art, id: `${art}:${f.id}`, tier: "reversible", kind: "finding", title: f.claim, anchor: `finding-${art}-${f.id}`, step_title: "" });
        if (done === pr.plan.stages.length && pr.plan.stages.length > 0 && !mockReview(pr).reviewed) items.push({ project: name, stage: "", id: "finished", tier: "reversible", kind: "finished", title: "Review the finished work", anchor: "final-files", step_title: "" });
      }
      items.push({ project: "tide-chart-app", stage: "build", id: "build-1", tier: "destructive", kind: "code", title: "Review code diff (pending)", anchor: "code-build-1", step_title: "Build the chart" });
      return { items, errors: [{ project: "old-pitch-deck", error: "its journal could not be read" }] } as T;
    }
    case "export_run_report":
    case "export_run_report_raw":
      throw "Run reports require the native Rust app. This is a browser layout preview.";
    case "stage_code_draft":
    case "decide_code":
    case "reconcile_code":
    case "code_bundle":
    case "request_dependency_install":
    case "escalate_finding":
    case "set_guardrail": {
      if (!p.plan) throw "There is no plan yet";
      const m = mockMatrix(p);
      for (const role of Object.keys(m)) if (roleInfo(role)?.side !== "checking") m[role][a.tool as string] = a.ask ? "ask" : "deny";
      p.matrix = m; p.approvedHash = null;
      return undefined as T;
    }
    case "set_action_policy": p.matrix = a.matrix as Record<string, Record<string, Permission>>; p.approvedHash = null; return undefined as T;
    case "decide_actions":
      throw "Action policy decisions require the native Rust app. This browser preview does not enforce policy.";
    case "export_package":
      if (!p.artifacts.length) throw "there is nothing to export yet";
      return { path: "~/Library/Application Support/app.sproutcrew.mac/projects/export/package-1", files: 8, final: p.plan?.stages.find((x) => x.id === "polish_document")?.title ?? null, notes: [], warnings: [], broken_links: [], missing: ["sources"], incomplete: [] } as T;
    case "start_preview": throw "This project has no prototype yet. Add a step for the Builder that asks for one, in Shape the steps before you approve the plan, or ask for a change to add it.";
    case "use_cases": return USE_CASES.filter((c) => mock.runningCode || c.id !== "app_development") as T;
    case "google_docs_status": return { available: false, connected: false } as T;
    case "connect_google_docs": case "disconnect_google_docs": case "read_google_doc":
      throw "Google Docs works in the native app; this is a browser preview.";
    case "path_info": // The browser has no disk to look at, so a name with a dot is a 12 KB file and any other is a folder.
      return (a.paths as string[]).map((v) => { const name = v.replace(/\/+$/, "").split("/").pop() ?? v; const folder = !name.includes("."); return { name, size: folder ? null : 12288, is_dir: folder }; }) as T;
    case "keynote_installed": return true as T;
    case "set_keynote": {
      if (!p.plan) throw ["there is no plan"];
      if (a.on && !p.plan.stages.some((x) => x.id.includes("deck"))) throw ["This plan makes no slides, so there is no deck for Keynote to build."];
      p.plan = { ...p.plan, keynote: !!a.on || undefined };
      return undefined as T;
    }
    case "running_code_allowed": return mock.runningCode as T;
    case "set_running_code": mock.runningCode = !!a.on; return mock.runningCode as T;
    case "set_prototype": {
      if (!p.plan) throw ["there is no plan"];
      if (a.on && !mock.runningCode) throw ["Projects that write running code are off. Turn on \"Allow projects that write running code\" in Settings to build an app or add a clickable prototype."];
      p.plan = { ...p.plan, stages: p.plan.stages.filter((x) => x.id !== "prototype") };
      if (a.on) {
        const last = p.plan.stages[p.plan.stages.length - 1];
        const w = p.plan.team.find((t) => t.role === p.plan!.stages[0]?.owner_role);
        p.plan.stages.push({ id: "prototype", title: "Build a clickable prototype", owner_role: "builder", guide: "", after: last ? [last.id] : [] });
        if (w && !p.plan.team.some((t) => t.role === "builder")) p.plan.team.push({ ...w, role: "builder" });
      } else p.plan.team = p.plan.team.filter((t) => t.role !== "builder");
      return undefined as T;
    }
    case "add_source_path": {
      const raw = String(a.path ?? "").trim();
      if (!raw) throw "type or paste the path of a file or a folder";
      return mockAddSource(raw.split("/").pop() || raw, raw.length) as T;
    }
    case "propose_notion": {
      const id = `w${p.writes.length + 1}`;
      if (!/notion\.so\/.*[0-9a-f]{32}$/i.test(String(a.parent).trim()) && !/^[0-9a-f]{32}$/i.test(String(a.parent).trim())) throw "paste the address of a Notion page, or its 32-character id";
      if (!String(a.title).trim() || !String(a.body).trim()) throw "the page needs a title and text";
      p.writes.push({ id, kind: "notion", parent: String(a.parent), title: String(a.title), body: String(a.body), append: !!a.append, status: "proposed", note: "", url: "" });
      return id as T;
    }
    case "propose_github": {
      const id = `w${p.writes.length + 1}`;
      if (!/^[\w.-]+\/[\w.-]+$/.test(String(a.repo).trim())) throw `"${a.repo}" is not a repository. Use the form owner/name`;
      p.writes.push({ id, kind: "github", repo: String(a.repo), branch: String(a.branch) || `bff/${mock.current}-1700000000`, folder: String(a.folder) || mock.current, files: [{ path: `${String(a.folder) || mock.current}/README.md`, bytes: 120 }, { path: `${String(a.folder) || mock.current}/src/timer.py`, bytes: 900 }], status: "proposed", note: "", url: "" });
      return id as T;
    }
    case "approve_write": p.writes.find((x) => x.id === a.id)!.status = "approved"; return undefined as T;
    case "cancel_write": p.writes.find((x) => x.id === a.id)!.status = "cancelled"; return undefined as T;
    case "retry_write": p.writes.find((x) => x.id === a.id)!.status = "proposed"; return undefined as T;
    case "resolve_write": {
      const w = p.writes.find((x) => x.id === a.id)!;
      if (w.status !== "sending") throw `${a.id} is not waiting for a check`;
      if (a.arrived == null) w.status = "proposed";
      else if (!String(a.arrived).trim()) throw "give the address of what was written";
      else { w.status = "done"; w.url = String(a.arrived).trim(); }
      return undefined as T;
    }
    case "write_file": return `Mock text of ${String(a.path)}\n` as T;
    case "send_write": {
      const w = p.writes.find((x) => x.id === a.id)!;
      if (w.status !== "approved") throw `${w.id} is not approved`;
      w.status = "done";
      w.url = w.kind === "notion" ? "https://www.notion.so/mock-page" : `https://github.com/${w.repo}/tree/${w.branch}`;
      return w.url as T;
    }
    case "export_client_files": {
      const d = a.documentStep as string | null;
      const k = a.deckStep as string | null;
      if (!d && !k) throw "choose a step for the document or for the deck";
      const folder = `/Users/you/Documents/SproutCrew/${mock.current}`;
      const base = `${folder}/${mock.current}`;
      return { folder, files: [...(d ? [{ kind: "word", path: `${base}-document.docx` }, { kind: "pdf", path: `${base}-document.pdf` }] : []), { kind: "deck", path: `${base}-deck.pptx` }], notes: [] } as T;
    }
    case "final_files": {
      const done = !!p.plan && p.plan.stages.every((st) => p.artifacts.some((x) => x.name === st.id));
      const folder = `/Users/you/Documents/SproutCrew/${mock.current}`;
      const base = `${folder}/${mock.current}`;
      const files = done ? [{ kind: "word", path: `${base}-document.docx` }, { kind: "pdf", path: `${base}-document.pdf` }, { kind: "deck", path: `${base}-deck.pptx` }] : [];
      const gaps = mockGaps(p);
      const held = gaps.length > 0 && !mockAccepted(p);
      const accepted = gaps.length > 0 && !held;
      return { files: held ? [] : files, folder: held || !done ? null : folder, notes: [], open: "", gaps: accepted ? [] : gaps, left_out: accepted ? gaps : [], accepted, note_steps: gaps.length ? ["finish"] : [], rerun: gaps.length ? ["finish"] : [] } as T;
    }
    case "open_export": return undefined as T;
    case "reveal_export": return undefined as T;
    case "projects_overview":
      return Object.entries(mock.projects).map(([name, pr]) => ({
        name,
        brief: pr.brief.replace(/\s+/g, " ").slice(0, 160),
        steps_done: new Set(pr.artifacts.map((x) => x.name)).size,
        reviewed: mockReview(pr).reviewed,
        steps_total: pr.plan?.stages.length ?? 0,
        approved: pr.approvedHash !== null,
        ...(mockGaps(pr).length && !mockAccepted(pr) ? { state: "needs_details", details_missing: mockGaps(pr).length } : {}),
        outcome: pr.plan ? (pr.plan.stages.some((x) => x.id === "outline") ? "Writing & editing" : pr.plan.stages.some((x) => x.id === "frame") ? "Brainstorm" : "Case study") : null,
        last_at: Math.floor(Date.now() / 1000) - 600,
        archived: !!pr.archived,
      })) as T;
    case "link_project":
      if (a.from === mock.current) throw "a project cannot be linked to itself";
      if (mock.projects[String(a.from)]?.links?.includes(mock.current)) throw `${a.from} is already linked as built from ${mock.current}`;
      if (!p.links?.includes(String(a.from))) p.links = [...(p.links ?? []), String(a.from)];
      return undefined as T;
    case "unlink_project": p.links = (p.links ?? []).filter((n) => n !== a.from); return undefined as T;
    case "reveal_code": return undefined as T;
    case "project_results": {
      const other = mock.projects[String(a.name)];
      if (!other) throw `no project named ${a.name}`;
      return other.artifacts.map((x) => ({ stage: x.name, title: other.plan?.stages.find((s) => s.id === x.name)?.title ?? x.name, chars: 1200 })) as T;
    }
    case "add_source_results": {
      const added: Source[] = [];
      const skipped: { name: string; reason: string }[] = [];
      for (const stage of a.stages as string[]) {
        const name = `${a.from}-${stage}.md`;
        if (!mock.projects[String(a.from)]?.artifacts.some((x) => x.name === stage)) { skipped.push({ name, reason: `${a.from} has no result for the step ${stage}` }); continue; }
        const have = p.sources.find((x) => x.name === name);
        const src = have ?? { id: nextSourceId(p.sources), name, bytes: 1200, pages: 1, flags: [], suspicious: [], origin: `project ${a.from}, step ${stage}` };
        if (!have) p.sources.push(src);
        added.push(src);
      }
      if (added.length && !p.links?.includes(String(a.from))) p.links = [...(p.links ?? []), String(a.from)];
      return { added, skipped } as T;
    }
    case "add_source_url": {
      const raw = String(a.url ?? "").trim();
      const host = raw.replace(/^https?:\/\//i, "").split(/[/?#]/)[0].toLowerCase();
      if (!raw) return { added: [], skipped: [{ name: raw, reason: "type or paste the address of a web page" }] } as T;
      if (!/^[a-z0-9-]+(\.[a-z0-9-]+)*\.[a-z]{2,}$/.test(host) || /\.(local|internal|lan)$/.test(host)) {
        return { added: [], skipped: [{ name: raw, reason: `"${host}" is not a public web site name, or is an address on a private network` }] } as T;
      }
      if (/docs\.google\.com/.test(host) && /private/.test(raw)) {
        return { added: [], skipped: [{ name: raw, reason: "This Google document is not shared by link, so BFF cannot read it (BFF does not sign in). In Google, choose Share, set General access to Anyone with the link as Viewer, and add it again. Or download the document and add the file" }] } as T;
      }
      const url = `https://${host}/${raw.replace(/^https?:\/\/[^/]+\/?/i, "").split("#")[0]}`;
      const name = `${host.replace(/\./g, "-")}-page.md`;
      const have = p.sources.find((x) => x.origin === url);
      const src = have ?? { id: nextSourceId(p.sources), name, bytes: 2048, pages: 1, flags: /hidden/.test(raw) ? ["2 hidden pieces of text on the page were left out"] : [], suspicious: /inject|ignore/i.test(raw) ? ["tells the AI to ignore or replace its instructions"] : [], origin: url };
      if (!have) p.sources.push(src);
      return { added: [src], skipped: [] } as T;
    }
    case "set_source_role": {
      const f = p.sources.find((x) => x.id === a.id);
      if (!f) throw `${a.id} is not a file in this project`;
      f.role = a.role as SourceRole; f.role_chosen = true;
      return undefined as T;
    }
    case "remove_source":
      if (p.artifacts.length) throw "a step has already been used these files. Start a new project to change them";
      p.sources = p.sources.filter((x) => x.id !== a.id);
      return undefined as T;
    case "add_note": {
      if (a.replaceId) {
        const note = p.notes.find((n) => n.id === a.replaceId);
        if (!note) throw "There is no such note";
        if (!String(a.text).trim()) throw "A note cannot be empty";
        note.text = String(a.text).trim(); return note as T;
      }
      if (!(a.text as string).trim()) throw "a note cannot be empty";
      if (a.stage && !p.plan?.stages.some((x) => x.id === a.stage)) throw `there is no step "${a.stage}" in the plan`;
      const note = { id: `n${p.notes.length + 1}`, stage: a.stage as string, text: (a.text as string).trim(), at: Date.now() };
      p.notes.push(note);
      return structuredClone(note) as T;
    }
    case "set_exercise_rules": {
      const t = (a.text as string).trim();
      if (t.length > 6000) throw "the rules can be at most 6000 characters. Put the long instructions in a file and mark it as requirements";
      p.rules = t;
      return (t ? { text: t, at: Date.now(), sha256: "mock" } : null) as T;
    }
    case "export_decision_log": {
      const text = `# Decision log\n\nBuilt just now from this project's own records. No model wrote this log.\n\n**Brief.** ${p.brief}\n${p.rules ? `\n**Rules for this exercise:**\n\n> ${p.rules}\n` : ""}\n## Part 1. What the second check said and what changed after\n\nNo checking step has run yet.\n`;
      return { path: "~/Library/Application Support/app.sproutcrew.mac/projects/export/decision-log-1.md", text } as T;
    }
    case "remove_note": p.notes = p.notes.filter((x) => x.id !== a.id); return undefined as T;
    case "step_inputs": {
      const st = a.stage as string;
      if (!p.artifacts.some((x) => x.name === st)) return null as T;
      return structuredClone({
        task: "t1", stage: st, route: "R1", model: "gpt-6.1-terra", effort: "medium", at: 0,
        guide: p.plan?.stages.find((x) => x.id === st)?.guide ?? "",
        notes: p.notes.filter((x) => !x.stage || x.stage === st),
        inputs: [
          ...p.sources.map((x) => ({ kind: "source", id: x.id, label: `${x.id} (${x.name})`, file: "", sha256: "0".repeat(64) })),
          ...p.plan!.stages.filter((x) => x.id !== st && p.artifacts.some((r) => r.name === x.id)).map((x) => ({ kind: "earlier", id: x.id, label: `${x.id} (${x.title}), by ${x.owner_role}`, file: "x.md", sha256: "0".repeat(64) })),
        ],
      }) as T;
    }
    case "step_chat": return structuredClone(p.chats[a.stage as string] ?? []) as T;
    case "coordinator_chat": return structuredClone(p.coord) as T;
    case "coordinator_apply": {
      const prop = p.coord.map((t) => t.proposal).find((x) => x?.id === a.id);
      if (!prop || (prop.status !== "pending" && prop.status !== "blocked")) throw "that proposal is not waiting";
      if (prop.problems.length) throw prop.problems.join("; ");
      for (const c of prop.changes) {
        if (c.op === "add_note") p.notes.push({ id: `n${p.notes.length + 1}`, stage: c.step, text: c.text, at: Date.now() });
        if (c.op === "edit_step") { const st = p.plan!.stages.find((x) => x.id === c.step); if (st && c.text) st.guide = c.text; }
      }
      if (prop.changes.some((c) => c.op !== "add_note")) p.approvedHash = hashPlan(p.plan!);
      prop.status = "applied";
      return structuredClone(prop.summary) as T;
    }
    case "coordinator_dismiss": {
      const prop = p.coord.map((t) => t.proposal).find((x) => x?.id === a.id);
      if (!prop) throw "there is no such proposal";
      prop.status = "dismissed";
      return undefined as T;
    }
    case "queue_to_drafts": {
      if (!p.artifacts.some((x) => x.name === a.stage)) throw `${a.stage} has not produced a result yet`;
      const at = a.start as number, gap = a.spacing as number;
      const made = [{ id: `p${p.posts.length + 1}`, title: "Post 1" }, { id: `p${p.posts.length + 2}`, title: "Post 2" }];
      made.forEach((m, i) => p.posts.push({ id: m.id, destination: "file", text: `${m.title}: practice text.\n\n#practice`, scheduled_at: at + i * gap, status: "draft", receipt: null }));
      return { created: made, skipped: [] } as T;
    }
    case "ready_stages": {
      const stages = p.plan?.stages ?? [];
      return readySteps(stages, p.artifacts.map((x) => x.name)).filter((id) => !p.blocked?.[id]) as T;
    }
    case "fact_questions": return (Object.values(p.blocked ?? {}).includes(WAITING) ? structuredClone(MOCK_QUESTIONS) : []) as T;
    case "answer_facts": {
      const answers = a.answers as (string | null)[];
      if (!Object.values(p.blocked ?? {}).includes(WAITING)) throw "There are no questions waiting for answers.";
      if (answers.length !== MOCK_QUESTIONS.length) throw "The answers do not match the questions. Close this and open it again.";
      if (answers.some((x) => x && /password|secret:|sk-[a-z0-9]{8,}/i.test(x))) throw "This answer looks like it holds a password, so it wasn't saved. Please reword it.";
      const lines = MOCK_QUESTIONS.flatMap((q, i) => (answers[i]?.trim() ? [`- Question: ${q.ask} Answer: ${answers[i]!.trim()}`] : []));
      if (lines.length) {
        const later = (p.plan?.stages ?? []).filter((x) => x.id !== "facts" && x.id !== "web_research").map((x) => x.id);
        for (const stage of later) p.notes.push({ id: `n${p.notes.length + 1}`, stage, text: `Facts from the person, given before the draft was written. Use them where the piece needs them, and do not mark them [NEEDED]:\n${lines.join("\n")}`, at: Date.now() });
      }
      p.blocked = {};
      return undefined as T;
    }
    case "begin_run": case "end_run": return undefined as T;
    case "stop_run": { if (a.steps) mock.projects[mock.current].stopped = true; return undefined as T; }
    case "use_workflow": {
      const author = a.author as string[];
      const reviewer = a.reviewer as string[];
      const u = USE_CASES.find((x) => x.id === a.name);
      if (!u) throw `${a.name} is not a use case`;
      if (author[0] === reviewer[0] && !["R3", "R5", "R8", "R9", "R10", "R11", "R12"].includes(author[0])) throw `the author and the reviewer are both on ${author[0]}. Put the reviewer on a different route`;
      const checksFacts = ["writing", "proposal", "research_brief"].includes(u.id);
      p.plan = {
        brief: p.brief,
        stages: [...(checksFacts ? [{ id: "facts", title: "Check what's missing", owner_role: "writer", guide: "Read the brief and the files. List up to five facts that the piece needs and only the person can know." }] : []), ...u.steps.map((title, i) => {
          const id = STEP_IDS[u.id][i];
          const after = AFTER[u.id]?.[id] ?? (checksFacts && i === 0 ? ["facts"] : undefined);
          return { id, title, owner_role: u.owners[i], guide: "Guide text.", ...(after ? { after } : {}) };
        })],
        team: u.team.map((r) => {
          const c = r.side === "writing" ? author : reviewer;
          return { role: r.role, route: c[0], model: c[1], effort: c[2] };
        }),
      };
      p.approvedHash = null;
      p.blocked = {};
      return undefined as T;
    }
    case "outbox_draft": {
      const id = `p${p.posts.length + 1}`;
      if (p.posts.some((x) => x.text === a.text && x.scheduled_at === a.at && x.status !== "cancelled")) throw `Duplicate("${p.posts[0].id}")`;
      p.posts.push({ id, destination: "fake-file", text: a.text as string, scheduled_at: a.at as number, status: "draft", receipt: null });
      return id as T;
    }
    case "outbox_approve": p.posts.find((x) => x.id === a.id)!.status = "approved"; return undefined as T;
    case "outbox_cancel": p.posts.find((x) => x.id === a.id)!.status = "cancelled"; return undefined as T;
    case "outbox_send_due": {
      const now = Math.floor(Date.now() / 1000);
      return p.posts.filter((x) => x.status === "approved" && x.scheduled_at <= now).map((x) => {
        x.status = "sent";
        x.receipt = { remote_id: `fake-${x.id}`, sent_at: now };
        return { id: x.id, ok: true, remote_id: x.receipt.remote_id };
      }) as T;
    }
    case "spend_status": return { tokens: { ...mockTokens }, dollars: null } as T;
    case "spend_raise": {
      const c = Number(a.newCap);
      if (a.unit !== "tokens") throw "no route has prices, so there is no dollar cap to raise";
      if (!(c > mockTokens.cap)) throw `the new cap ${c} tokens is not higher than the current cap ${mockTokens.cap} tokens`;
      mockTokens.cap = c;
      mockTokens.available = c - mockTokens.spent;
      return { tokens: { ...mockTokens }, dollars: null } as T;
    }
    case "connector_read": throw "NoCredential";
    case "unblock_connector": return true as T;
    default: throw `unknown command ${cmd}`;
  }
}

async function mockWave<T>(a: Record<string, unknown>, onEvent: (e: RunEvent) => void): Promise<T> {
  const p = mock.projects[mock.current];
  const ids = a.stageIds as string[];
  const ready = readySteps(p.plan?.stages ?? [], p.artifacts.map((x) => x.name)).filter((id) => !p.blocked?.[id]);
  const bad = ids.find((i) => !ready.includes(i));
  onEvent({ event: "started", engine: a.engine as string, what: ids.map((i) => `stage ${i}`).join(" and ") });
  await sleep(700);
  if (bad) { onEvent({ event: "finished", ok: false, note: `${bad} is not ready` }); throw `${bad} is not ready. A step is ready when everything it waits for is done`; }
  const rows = [];
  for (const id of ids) {
    await mockRun("run_stage", { engine: a.engine, stageId: id }, () => undefined);
    rows.push({ stage: id, ok: true, note: "replay" });
  }
  onEvent({ event: "finished", ok: true, note: "" });
  return rows as T;
}

/** The preview's policy defaults, which start as every project's policy until now. */
let mockDefaults = { send_outside: "ask", delete_files: "ask", install_software: "ask", privacy: "standard" } as { send_outside: "ask" | "never"; delete_files: "ask" | "never"; install_software: "ask" | "never"; privacy: "standard" | "no_retention" };

/** The ?board preview's sample projects, each waiting on a different decision. Made once, through the same mock
 *  calls the screens use. */
let seeded: Promise<void> | null = null;
function seedBoard(): Promise<void> {
  return seeded ??= (async () => {
    const quiet = () => undefined;
    const make = async (name: string, brief: string) => {
      await mockCall("create_project", { name, brief });
      await mockRun("propose_plan", { engine: "live" }, quiet);
      return mock.projects[name];
    };
    await make("q3-roadmap-brief", "Write a Q3 roadmap brief for the leadership team from the planning notes.");
    const pr = await make("pricing-research", "Research how three competitors price their team plans.");
    pr.approvedHash = hashPlan(pr.plan!);
    (pr.failures ??= {})[pr.plan!.stages[0].id] = ["the account is out of credit", "out_of_credits"];
    const ob = await make("onboarding-posts", "Draft five onboarding posts for new customers.");
    ob.approvedHash = hashPlan(ob.plan!);
    ob.stopped = true;
    await make("tide-chart-app", "Build a small tide chart app.");
    mock.current = "";
  })();
}

async function mockRun<T>(cmd: string, a: Record<string, unknown>, onEvent: (e: RunEvent) => void): Promise<T> {
  const p = mock.projects[mock.current];
  const title = (id: unknown) => p.plan?.stages.find((x) => x.id === id)?.title ?? "the step";
  const what = cmd === "ask_step" ? `a question about ${title(a.stage)}` : cmd === "coordinator_say" ? "your request for a change" : cmd === "propose_plan" ? "planning the steps" : cmd === "review" ? `a second check of ${title(a.artifact)}` : cmd === "revise" ? `a revision of ${title(a.artifact)}` : `stage ${a.stageId}`;
  onEvent({ event: "started", engine: a.engine as string, what });
  if (cmd === "run_stage") p.stopped = false;
  await sleep(500);
  const finish = (ok: boolean, note: string) => onEvent({ event: "finished", ok, note });
  // ?fail=per_minute (or daily_cap, upstream_busy, out_of_credits) makes the next step fail the way the app reports it.
  const failing = typeof location !== "undefined" ? new URLSearchParams(location.search).get("fail") : null;
  if (cmd === "run_stage" && failing && MOCK_FAILURES[failing]) {
    if (failing === "per_minute" || failing === "upstream_busy") {
      for (const n of [2, 1]) { onEvent({ event: "progress", line: `${MOCK_FAILURES[failing].split(". ")[0]}. Retrying in ${n} second${n === 1 ? "" : "s"}.` }); await sleep(1000); }
    }
    (p.failures ??= {})[a.stageId as string] = [MOCK_FAILURES[failing], failing === "held" ? "" : failing];
    finish(false, MOCK_FAILURES[failing]);
    const title = p.plan?.stages.find((x) => x.id === a.stageId)?.title ?? (a.stageId as string);
    throw `“${title}” did not finish. ${plainWhy(MOCK_FAILURES[failing])}`;
  }
  if (cmd === "ask_step") {
    const st = a.stage as string;
    if (!p.artifacts.some((x) => x.name === st)) { finish(false, "this step has not produced a result yet"); throw `${st} has not produced a result yet`; }
    const src = p.sources[0];
    const msg: ChatMessage = {
      role: "assistant", at: Date.now(), by: `${(a.role as string) || "writer"} (practice)`, basis: "recorded",
      text: "This is a practice answer. No model was asked, so it explains nothing about the step. Turn on Live to ask a real model.",
      citations: src ? [{ source: `${src.id}#p1`, quote: "Manual intake costs", found: true, note: "" }, { source: `${src.id}#p1`, quote: "a figure that is not in the file", found: false, note: `those words are not in ${src.id}#p1` }] : [],
    };
    (p.chats[st] ??= []).push({ role: "user", text: a.question as string, at: Date.now(), by: "", citations: [], basis: "" }, msg);
    finish(true, "done");
    return structuredClone(msg) as T;
  }
  if (cmd === "coordinator_say") {
    if (!p.plan) { finish(false, "there is no plan yet"); throw "there is no plan yet"; }
    const text = (a.message as string).trim();
    const wantsShort = /shorter|short|brief/i.test(text);
    const wantsNote = /date|use |cut|add a note/i.test(text);
    const first = p.plan.stages[0];
    const changes: Change[] = wantsShort
      ? [{ op: "edit_step", step: first.id, text: "Keep it to three short paragraphs.", title: "", id: "", role: "", after: "", route: "", model: "", effort: "" }]
      : wantsNote
        ? [{ op: "add_note", step: first.id, text: text, title: "", id: "", role: "", after: "", route: "", model: "", effort: "" }]
        : [];
    const done = p.plan.stages.filter((x) => p.artifacts.some((r) => r.name === x.id)).map((x) => x.id);
    const prop: Proposal | null = changes.length
      ? {
          id: `p${p.coord.filter((t) => t.proposal).length + 1}`, changes, status: "pending", problems: [], plan_hash: "x",
          summary: [wantsShort ? `Change the instructions of the step "${first.title}" (${first.id})` : `Add a note for the step "${first.title}": ${text}`],
          stale: done.filter((id) => p.plan!.stages.findIndex((x) => x.id === id) >= 0), rerun: done.slice(0, 1),
        }
      : null;
    for (const t of p.coord) if (t.proposal && (t.proposal.status === "pending" || t.proposal.status === "blocked")) t.proposal.status = "superseded";
    p.coord.push({ role: "user", text, at: Date.now(), by: "", proposal: null });
    const turn: Turn = { role: "assistant", at: Date.now(), by: "practice", proposal: prop,
      text: prop ? "This is a practice reply. I would make this change and then run the step again." : "This is a practice reply. No model was asked, so no change is proposed. Turn on Live to ask for a change." };
    p.coord.push(turn);
    finish(true, "done");
    return structuredClone(turn) as T;
  }
  if (cmd === "propose_plan") {
    p.plan = {
      brief: p.brief,
      stages: [{ id: "draft", title: "Draft the note", owner_role: "writer" }],
      team: [
        { role: "writer", route: "R1", model: "gpt-6.1-terra", effort: "medium" },
        { role: "critic", route: "R2", model: "claude-sonnet-5-5", effort: "high" },
      ],
    };
    finish(true, "");
    return undefined as T;
  }
  if (cmd === "review") {
    const r = p.review[a.artifact as string];
    if (!r) { finish(false, "no such artifact"); throw "no such artifact"; }
    const round = (p.round[a.artifact as string] = (p.round[a.artifact as string] ?? 0) + 1);
    r.reviewers[a.reviewer as string] = "valid";
    if (round === 1) {
      r.findings = SAMPLE_FINDINGS.map((f) => ({ ...f }));
      finish(true, "done");
      return { outcome: "open", findings: r.findings.map((f) => f.id) } as T;
    }
    r.findings = [];
    finish(true, "done");
    return { outcome: "reviewed_no_change", findings: [] } as T;
  }
  if (cmd === "revise") {
    const r = p.review[a.artifact as string];
    if (!r?.findings.some((f) => f.decision === "accept")) { finish(false, "there are no accepted findings to address"); throw "there are no accepted findings to address"; }
    r.version += 1;
    for (const k of Object.keys(r.reviewers)) r.reviewers[k] = "invalidated";
    finish(true, "done");
    return r.version as T;
  }
  if (!p.plan || !p.approvedHash || p.approvedHash !== hashPlan(p.plan)) {
    finish(false, "the plan is not approved");
    throw "the plan is not approved";
  }
  const file = `${a.stageId}-${(p.artifacts.length + 1).toString().padStart(8, "0")}.md`;
  p.texts[file] = `# ${p.plan.stages.find((x) => x.id === a.stageId)?.title ?? "Result"}

This file came from the replay engine, not from a live run. It shows how a real result will read.

## What it found

Manual intake costs **4.2 million** a year across *three sites*, and the average claim takes 11 days (src-1).

- Intake has 40 heads at 1.5 hours per claim.
- Adjusters have 120 heads at 6 hours per claim.
  - The two figures are not on the same basis.

| Team | Heads | Hours per claim |
|---|---:|---:|
| Intake | 40 | 1.5 |
| Adjusters | 120 | 6 |

> Files it was shown: ${p.sources.map((x) => x.name).join(", ") || "none"}.

\`\`\`
hours = heads * hours_per_claim
\`\`\`
`;
  if (a.stageId === "facts") {
    p.texts[file] = `# Check what's missing\n\n${MOCK_QUESTIONS.map((q) => `Q: ${q.ask} | From: ${q.from ?? "nothing"} | Suggested: ${q.suggested ?? "nothing"}`).join("\n")}\n`;
    // The steps that wait for this one are held until the person answers, as the real run holds them.
    p.blocked = Object.fromEntries(p.plan.stages.filter((x) => x.id !== "facts" && (x.after ?? []).includes("facts")).map((x) => [x.id, WAITING]));
  }
  if (a.stageId === "review") p.texts[file] += `\n${MOCK_FLAGGED.map((f) => `- ${f}`).join("\n")}\n\nVerdict: Fix first\n`;
  const stage = p.plan.stages.find((s) => s.id === a.stageId);
  p.artifacts.push({ name: a.stageId as string, sha256: "0".repeat(64), file, from_task: `t${p.artifacts.length + 1}` });
  if (!p.review[a.stageId as string]) p.review[a.stageId as string] = { owner: stage?.owner_role ?? "writer", version: 1, head: "0".repeat(64), findings: [], reviewers: {} };
  if (p.budget) p.budget.spent += p.budget.perTask;
  finish(true, "replay");
  return "replay" as T;
}

export const api = {
  mocked: !inTauri,
  inTauri,
  async addFile(file: File): Promise<Added> {
    if (inTauri) return tauriBytes<Added>("add_source_bytes", new Uint8Array(await file.arrayBuffer()), file.name);
    return mockAddSource(file.name, file.size);
  },
  /** Paths dropped on the window in the app. Outside the app there are no paths, only files. */
  async onDrop(f: (paths: string[], over: boolean) => void): Promise<() => void> {
    if (!inTauri) return () => undefined;
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    return getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "drop") f(p.paths, false);
      else if (p.type === "enter" || p.type === "over") f([], true);
      else f([], false);
    });
  },
  call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    return inTauri ? tauri<T>(cmd, args) : mockCall<T>(cmd, args);
  },
  async run<T>(cmd: string, args: Record<string, unknown>, onEvent: (e: RunEvent) => void): Promise<T> {
    const result = await (!inTauri && cmd === "run_wave" ? mockWave<T>(args, onEvent) : inTauri ? tauriRun<T>(cmd, args, onEvent) : mockRun<T>(cmd, args, onEvent));
    const why = failedRun(cmd, result);
    if (why) throw why;
    return result;
  },
};
