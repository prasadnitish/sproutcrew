import { useCallback, useEffect, useState } from "react";
import { api, SpendStatus } from "./api";
import { CapCard } from "./Cost";
import { dollarsLine, group, GroupBy, total, UsageRow } from "./usage";
import { Section, text } from "./ui";

type Everywhere = { projects: { project: string; rows: UsageRow[] }[]; unreadable: string[] };
type By = "project" | GroupBy;
const n = (v: number) => v.toLocaleString("en-US");
const LABEL: Record<By, string> = { project: "Project", day: "Day", month: "Month", model: "Model", role: "Role" };

/** What every project has used today, in words, for the foot of the sidebar. Unknown figures are never zero. */
export function todayLine(u: Everywhere | null): string {
  if (!u) return "";
  const start = new Date(); start.setHours(0, 0, 0, 0);
  const rows = u.projects.flatMap((p) => p.rows).filter((r) => r.at * 1000 >= start.getTime());
  const t = total(rows);
  return t.requests === 0 ? "$0 today" : `${dollarsLine(t.dollars, t.unknownDollars, t.requests)} today`;
}

/** Usage (DESIGN_SPEC.md §10): the limits shared by every project, and what each request cost. A project's own limit
 *  stays on its Usage & limits tab. */
export default function UsagePage({ onOpen }: { onOpen: (name: string) => void }) {
  const [u, setU] = useState<Everywhere | null>(null);
  const [st, setSt] = useState<SpendStatus | null>(null);
  const [err, setErr] = useState("");
  const [by, setBy] = useState<By>("project");
  const load = useCallback(() => {
    api.call<Everywhere>("usage_everywhere").then(setU).catch((e) => setErr(text(e)));
    api.call<SpendStatus>("spend_status").then(setSt).catch((e) => setErr(text(e)));
  }, []);
  useEffect(() => { load(); }, [load]);
  const raise = async (unit: string, cap: number) => {
    try { setSt(await api.call<SpendStatus>("spend_raise", { unit, newCap: cap })); setErr(""); } catch (e) { setErr(text(e)); }
  };
  if (!u || !st) return <div className="page-body">{err ? <p className="notice error" role="alert">{err}</p> : <p className="muted" role="status">Reading what every project used…</p>}</div>;
  const tagged = u.projects.flatMap((p) => p.rows.map((r) => ({ ...r, project: p.project })));
  const all = total(tagged);
  const unpriced = tagged.filter((r) => r.dollars === null);
  const groups = by === "project"
    ? u.projects.filter((p) => p.rows.length).map((p) => ({ key: p.project, label: p.project, ...total(p.rows) })).sort((a, b) => b.dollars - a.dollars || a.label.localeCompare(b.label))
    : group(tagged, by);
  const dollars = st.dollars;
  return (
    <div className="page-body">
      {err && <p className="notice error" role="alert">{err}</p>}
      {u.unreadable.length > 0 && <p className="note" role="status">Not counted, because {u.unreadable.length === 1 ? "it" : "they"} could not be read: {u.unreadable.join(", ")}.</p>}
      {all.requests === 0 && <p className="muted">Nothing has run yet.</p>}
      <div className="page-sections"><div>
        <Section title="Priced requests" value={`${all.requests - unpriced.length} of ${all.requests}`} flag={unpriced.length ? `${unpriced.length} never priced` : undefined}>
          {unpriced.length === 0 ? <p>Every request has a price from its provider.</p> : <>
            <p>The provider never reported a price for these, so each counts against the limit at the most it could have cost.</p>
            <ul className="plain">{unpriced.slice(0, 20).map((r, i) => <li key={i}><button className="link" onClick={() => onOpen(r.project)}>{r.project}</button>{r.stage ? `, ${r.stage}` : ""}{r.model ? <> · <span className="mono">{r.model}</span></> : null}</li>)}</ul>
          </>}
        </Section>
        <Section title="Dollars" value={dollars ? `$${(dollars.spent + dollars.held).toFixed(2)} of $${dollars.cap.toFixed(2)}` : "no dollar limit"} flag={dollars && dollars.available <= 0 ? "Limit reached" : undefined} tone="bad">
          {dollars ? <CapCard unit="dollars" sp={dollars} busy="" onRaise={raise} /> : <p>No connection has prices, so there is no dollar limit across projects. Each project keeps its own limit for paid models, and your provider's billing controls still apply.</p>}
        </Section>
        <Section title="Tokens" value={`${n(st.tokens.spent + st.tokens.held)} of ${n(st.tokens.cap)}`} flag={st.tokens.available <= 0 ? "Limit reached" : (st.tokens.spent + st.tokens.held) / st.tokens.cap >= 0.8 ? "80% used" : undefined} tone={st.tokens.available <= 0 ? "bad" : "warn"}>
          <CapCard unit="tokens" sp={st.tokens} busy="" onRaise={raise} />
          <p className="muted">Marked here when 80% is used. Past the limit, nothing is sent until you approve a higher one.</p>
        </Section>
        <Section title="Plan window" value="in each project">
          <p>A plan login's window is reported per project, on its Usage & limits tab, while a step uses it.</p>
        </Section>
        <Section title="Breakdown" value={`${u.projects.filter((p) => p.rows.length).length} projects, ${all.requests} requests`}>
          <div className="seg" role="group" aria-label="Group by">
            {(Object.keys(LABEL) as By[]).map((k) => <button key={k} aria-pressed={by === k} onClick={() => setBy(k)}>{LABEL[k]}</button>)}
          </div>
          <div className="table-wrap"><table className="table usage-table">
            <thead><tr><th scope="col">{LABEL[by]}</th><th scope="col" className="num">Requests</th><th scope="col" className="num">Tokens</th><th scope="col" className="num">Dollars</th></tr></thead>
            <tbody>{groups.map((g) => (
              <tr key={g.key}>
                <th scope="row">{by === "project" ? <button className="link" onClick={() => onOpen(g.label)}>{g.label}</button> : g.label}</th>
                <td className="num mono">{n(g.requests)}</td>
                <td className="num mono">{g.unknownTokens === g.requests ? "unknown" : `${n(g.tokens)}${g.unknownTokens ? ` + ${g.unknownTokens} unknown` : ""}`}</td>
                <td className="num mono">{dollarsLine(g.dollars, g.unknownDollars, g.requests)}</td>
              </tr>
            ))}</tbody>
            <tfoot><tr><th scope="row">All</th><td className="num mono">{n(all.requests)}</td><td className="num mono">{n(all.tokens)}{all.unknownTokens ? ` + ${all.unknownTokens} unknown` : ""}</td><td className="num mono">{dollarsLine(all.dollars, all.unknownDollars, all.requests)}</td></tr></tfoot>
          </table></div>
        </Section>
      </div></div>
    </div>
  );
}
