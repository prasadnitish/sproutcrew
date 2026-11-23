/** The board's queue (DESIGN_SPEC.md §10): what the core's decision inbox sends, grouped and ordered by risk. */

export type DecisionItem = { project: string; stage: string; id: string; tier: string; kind: string; title: string; anchor: string; step_title?: string };
export type Inbox = { items: DecisionItem[]; errors: { project: string; error: string }[] };

export type GroupId = "approve" | "answer" | "failed" | "review";
export type Tone = "warn" | "info" | "bad" | "quiet";
export const GROUPS: { id: GroupId; label: string; tone: Tone }[] = [
  { id: "approve", label: "Needs your approval", tone: "warn" },
  { id: "answer", label: "Needs an answer", tone: "info" },
  { id: "failed", label: "Failed or stopped", tone: "bad" },
  { id: "review", label: "Finished, not reviewed", tone: "quiet" },
];

type Kind = { group: GroupId; word: string; glyph: string };
const KINDS: Record<string, Kind> = {
  plan: { group: "approve", word: "Plan", glyph: "◆" },
  action: { group: "approve", word: "Action", glyph: "◆" },
  code: { group: "approve", word: "Code diff", glyph: "◆" },
  write: { group: "approve", word: "External write", glyph: "◆" },
  proposal: { group: "approve", word: "Plan change", glyph: "◆" },
  questions: { group: "answer", word: "Questions", glyph: "?" },
  finding: { group: "answer", word: "Finding", glyph: "?" },
  escalation: { group: "answer", word: "Judgment call", glyph: "?" },
  details: { group: "answer", word: "Missing details", glyph: "?" },
  failure: { group: "failed", word: "Failed step", glyph: "×" },
  stopped: { group: "failed", word: "Stopped by you", glyph: "■" },
  finished: { group: "review", word: "Finished", glyph: "✓" },
};

/** A kind the core adds later is still shown, as something that needs an answer. */
export function kindOf(kind: string): Kind {
  return KINDS[kind] ?? { group: "answer", word: "Decision", glyph: "?" };
}

const RISK: Record<string, number> = { destructive: 0, consequential: 1, reversible: 2 };
/** The risk in words, for the detail pane. */
export const riskWord = (tier: string) => ({ destructive: "Changes or deletes something", consequential: "Spends money or leaves this Mac", reversible: "Can be undone" } as Record<string, string>)[tier] ?? tier;

export type Group = { id: GroupId; label: string; tone: Tone; items: DecisionItem[] };

/** Groups in the spec's order, each ordered by risk. The core sends no time yet, so within one risk its own order stays. */
export function boardGroups(items: DecisionItem[]): Group[] {
  return GROUPS.map((g) => ({
    ...g,
    items: items
      .map((item, at) => ({ item, at }))
      .filter(({ item }) => kindOf(item.kind).group === g.id)
      .sort((a, b) => (RISK[a.item.tier] ?? 3) - (RISK[b.item.tier] ?? 3) || a.at - b.at)
      .map(({ item }) => item),
  })).filter((g) => g.items.length > 0);
}

export const keyOf = (i: DecisionItem) => `${i.project}:${i.kind}:${i.id}`;

/** The item the board opens on: the first one in the first group that has any. */
export function firstOpen(groups: Group[]): DecisionItem | null {
  return groups[0]?.items[0] ?? null;
}

/** J and K: the next or previous item in board order, staying put at either end. */
export function moveFrom(groups: Group[], from: DecisionItem | null, by: 1 | -1): DecisionItem | null {
  const all = groups.flatMap((g) => g.items);
  if (!all.length) return null;
  if (!from) return all[0];
  const at = all.findIndex((i) => keyOf(i) === keyOf(from));
  return all[Math.max(0, Math.min(all.length - 1, (at < 0 ? 0 : at + by)))];
}

/** The decision's own primary action (DESIGN_SPEC.md §10). On the board it stays off until §14's core work is done. */
export function primaryOf(kind: string): string {
  return ({
    plan: "Approve and start", action: "Allow this action", code: "Approve this exact diff", write: "Approve this text",
    proposal: "Apply and approve", questions: "Save answers and go on", details: "Save answers and go on", finding: "Accept",
    escalation: "Decide", failure: "Try again", stopped: "Run the rest", finished: "Mark reviewed",
  } as Record<string, string>)[kind] ?? "Decide";
}
