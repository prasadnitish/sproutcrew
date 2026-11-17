/** One request's usage, as the app records it. `null` is a figure that was never reported, which is never zero. */
export type UsageRow = {
  at: number;
  task: string;
  stage: string | null;
  role: string | null;
  route: string | null;
  model: string | null;
  tokens: number | null;
  dollars: number | null;
  dollars_status: string;
};

export type GroupBy = "day" | "month" | "role" | "model";

/** A group's totals. Known figures are summed; unknown ones are counted beside them, never added as zero. */
export type Group = { key: string; label: string; requests: number; tokens: number; unknownTokens: number; dollars: number; unknownDollars: number };

const pad = (n: number) => String(n).padStart(2, "0");

/** The group a row falls in, in local time for days and months. */
export function keyOf(r: UsageRow, by: GroupBy, roleName: (role: string) => string = (x) => x): [string, string] {
  const d = new Date(r.at * 1000);
  if (by === "day") {
    const k = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    return [k, d.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric", year: "numeric" })];
  }
  if (by === "month") {
    const k = `${d.getFullYear()}-${pad(d.getMonth() + 1)}`;
    return [k, d.toLocaleDateString("en-US", { month: "long", year: "numeric" })];
  }
  if (by === "role") {
    if (r.task === "plan") return ["~plan", "Coordinator plan"];
    return r.role ? [r.role, roleName(r.role)] : ["~none", "Role not recorded"];
  }
  return r.model ? [r.model, r.model] : ["~none", "Model not recorded"];
}

/** Rows grouped and totalled. Days and months come newest first; roles and models by cost, then by tokens. */
export function group(rows: UsageRow[], by: GroupBy, roleName?: (role: string) => string): Group[] {
  const m = new Map<string, Group>();
  for (const r of rows) {
    const [key, label] = keyOf(r, by, roleName);
    const g = m.get(key) ?? { key, label, requests: 0, tokens: 0, unknownTokens: 0, dollars: 0, unknownDollars: 0 };
    g.requests += 1;
    if (r.tokens === null) g.unknownTokens += 1; else g.tokens += r.tokens;
    if (r.dollars === null) g.unknownDollars += 1; else g.dollars += r.dollars;
    m.set(key, g);
  }
  const out = [...m.values()];
  if (by === "day" || by === "month") return out.sort((a, b) => b.key.localeCompare(a.key));
  return out.sort((a, b) => b.dollars - a.dollars || b.tokens - a.tokens || a.label.localeCompare(b.label));
}

/** Totals of every row. */
export function total(rows: UsageRow[]): Omit<Group, "key" | "label"> {
  const [g] = group(rows.map((r) => ({ ...r, model: "all" })), "model");
  return g ?? { requests: 0, tokens: 0, unknownTokens: 0, dollars: 0, unknownDollars: 0 };
}

/** A dollar total with its unknown part in words: "$0.42", "$0.42 + 2 unknown", or "unknown". */
export function dollarsLine(dollars: number, unknown: number, requests: number): string {
  if (requests > 0 && unknown === requests) return "unknown";
  const d = dollars === 0 ? "$0" : dollars < 0.01 ? "under $0.01" : `$${dollars.toFixed(2)}`;
  return unknown ? `${d} + ${unknown} unknown` : d;
}
