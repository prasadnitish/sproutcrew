import { useEffect, useState } from "react";
import { api, ProjectLimit, Spend, SpendStatus } from "./api";
import { planWindows } from "./Gauge";
import { useSpring } from "./spring";
import { Chip, Ctx, Empty, n, provider, roleName, secs, Tabs, text, SectionIntro } from "./ui";
import { amount } from "./amount";
import { dollarsLine, group, GroupBy, projectSpendLine, total } from "./usage";

const dollars = (v: number) => `$${v.toFixed(2)}`;
const tokens = (v: number) => `${n(v)} token${v === 1 ? "" : "s"}`;

/** A cap on API use. Past it nothing is sent until the person approves a higher cap. */
export function CapCard({ unit, sp, busy, onRaise }: { unit: "tokens" | "dollars"; sp: Spend; busy: string; onRaise: (unit: string, cap: number) => Promise<void> }) {
  const [custom, setCustom] = useState("");
  const show = unit === "tokens" ? tokens : dollars;
  const step = unit === "tokens" ? 500000 : 5;
  const used = sp.spent + sp.held;
  const pct = Math.min(100, Math.round((used / sp.cap) * 100));
  const out = sp.available <= 0;
  return (
    <>
      <HBar value={used / sp.cap} tone={out ? "bad" : pct >= 80 ? "warn" : "ok"} />
      <p className="budget-line">
        <strong>{show(sp.spent)}</strong> {unit === "tokens" ? "used" : "spent"} of {show(sp.cap)}, {show(sp.available)} left{sp.held > 0 ? `, ${show(sp.held)} held for requests in flight` : ""}{" "}
        {out && <Chip tone="bad">Cap reached</Chip>}
        {sp.unknown_charges > 0 && <Chip tone="quiet">{sp.unknown_charges} charged at their worst case</Chip>}
      </p>
      <div className="actions tight">
        <button disabled={!!busy} onClick={() => void onRaise(unit, sp.cap + step)}>Approve {show(step)} more (cap {show(sp.cap + step)})</button>
        <input aria-label={`New ${unit} cap`} inputMode="decimal" value={custom} onChange={(e) => setCustom(e.target.value)} placeholder={`Or a new cap in ${unit}`} />
        <button disabled={amount(custom) === null || !!busy} onClick={() => void onRaise(unit, amount(custom)!).then(() => setCustom(""))}>Set the cap</button>
      </div>
    </>
  );
}

function SpendCap({ busy }: { busy: string }) {
  const [st, setSt] = useState<SpendStatus | null>(null);
  const [err, setErr] = useState("");
  const load = () => api.call<SpendStatus>("spend_status").then(setSt).catch((e) => setErr(text(e)));
  useEffect(() => { void load(); }, [busy]);
  const raise = async (unit: string, cap: number) => {
    try { setSt(await api.call<SpendStatus>("spend_raise", { unit, newCap: cap })); setErr(""); } catch (e) { setErr(text(e)); }
  };
  if (!st) return null;
  return (
    <section className="sheet">
      <header className="sheet-head"><div><h2>API spending limits</h2><p className="muted">Applies across all projects. Tokens measure the text AI reads and writes. Requests that could exceed the limit are blocked until you approve a higher limit. Token-only connections do not estimate or cap dollar charges; your provider’s billing controls still apply.</p></div></header>
      <CapCard unit="tokens" sp={st.tokens} busy={busy} onRaise={raise} />
      {st.dollars && <><h3 className="cap-sub">Dollars</h3><CapCard unit="dollars" sp={st.dollars} busy={busy} onRaise={raise} /></>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </section>
  );
}


/** A horizontal bar. The optional mark is where a stop rule sits. */
function HBar({ value, mark, tone = "ok" }: { value: number; mark?: number; tone?: string }) {
  const v = Math.max(0, Math.min(1, useSpring(Math.max(0, Math.min(1, value))).value));
  return (
    <span className={`hbar ${tone}`} aria-hidden="true">
      <span className="hbar-fill" style={{ width: `${v * 100}%` }} />
      {mark !== undefined && <span className="hbar-mark" style={{ left: `${mark * 100}%` }} />}
    </span>
  );
}

/** The project's usage grouped by day, month, role or model, with a CSV export of every request. Unknown
 *  figures are counted beside the sums, never added as zero. */
function UsageBreakdown({ ctx }: { ctx: Ctx }) {
  const [by, setBy] = useState<GroupBy>("day");
  const [saved, setSaved] = useState("");
  const [err, setErr] = useState("");
  const rows = ctx.s.usage_rows ?? [];
  const groups = group(rows, by, roleName);
  const t = total(rows);
  const save = () => ctx.guard(async () => {
    try { const path = await api.call<string>("export_usage_csv"); setSaved(path); setErr(""); } catch (e) { setErr(text(e)); throw e; }
  });
  return (
    <section className="sheet">
      <header className="sheet-head">
        <div>
          <h2>Cost and tokens</h2>
          <p className="muted">{rows.length === 0 ? "Nothing has run yet." : `${t.requests} request${t.requests === 1 ? "" : "s"}, ${dollarsLine(t.dollars, t.unknownDollars, t.requests)} as reported by the providers.`}</p>
        </div>
        {rows.length > 0 && <button className="ghost" disabled={!!ctx.busy} onClick={() => void save()}>Export as CSV</button>}
      </header>
      {rows.length > 0 && (
        <>
          <Tabs label="Group usage by" panelId="usage-groups" value={by} onChange={setBy} items={[{ id: "day", label: "Day" }, { id: "month", label: "Month" }, { id: "role", label: "Role" }, { id: "model", label: "Model" }]} />
          <table className="table usage-table" id="usage-groups" role="tabpanel">
            <thead><tr><th>{by === "day" ? "Day" : by === "month" ? "Month" : by === "role" ? "Role" : "Model"}</th><th className="num">Requests</th><th className="num">Tokens</th><th className="num">Cost</th></tr></thead>
            <tbody>
              {groups.map((g) => (
                <tr key={g.key}>
                  <td>{g.label}</td>
                  <td className="num">{g.requests}</td>
                  <td className={`num${g.unknownTokens === g.requests ? " unknown" : ""}`}>{g.unknownTokens === g.requests ? "unknown" : `${n(g.tokens)}${g.unknownTokens ? ` + ${g.unknownTokens} unknown` : ""}`}</td>
                  <td className={`num${g.unknownDollars === g.requests ? " unknown" : ""}`}>{dollarsLine(g.dollars, g.unknownDollars, g.requests)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
      {saved && <p className="muted" role="status">Saved to the project's exports folder. <button className="link" onClick={() => void api.call("reveal_export", { path: saved }).catch(() => undefined)}>Show in Finder</button></p>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </section>
  );
}

/** The field and button that set the project's dollar limit, higher or lower. It is also on the plan card. */
export function LimitForm({ busy, guard, button }: { busy: boolean; guard: Ctx["guard"]; button: string }) {
  const [raise, setRaise] = useState("");
  const [err, setErr] = useState("");
  return (
    <>
      <div className="actions tight">
        <input aria-label="New limit in dollars" inputMode="decimal" value={raise} onChange={(e) => setRaise(e.target.value)} placeholder="New limit, such as 10" />
        <button disabled={amount(raise) === null || busy} onClick={() => guard(async () => { try { await api.call("set_project_limit", { dollars: amount(raise)! }); setRaise(""); setErr(""); } catch (e) { setErr(text(e)); throw e; } })}>{button}</button>
      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
    </>
  );
}

/** What paid models have cost this project against its dollar limit, and a way to raise the limit. Raising it is
 *  the approval to spend more; a cost that was never reported counts at the most it could have been. */
function ProjectLimitStat({ l, busy, guard }: { l: ProjectLimit; busy: boolean; guard: Ctx["guard"] }) {
  const used = l.spent + l.held;
  const full = used >= l.cap;
  return (
    <section className="stat">
      <p className={`big-number${l.locked ? " unknown" : ""}`}>{projectSpendLine(l)}</p>
      {!l.locked && <HBar value={l.cap ? used / l.cap : 0} tone={full ? "bad" : used / l.cap >= 0.8 ? "warn" : "ok"} />}
      <p className="muted">
        {l.locked ? "The record of this project's spending could not be read, so nothing paid runs until it is fixed." : full ? "The limit is reached. Paid steps pause until you raise it." : "Steps on paid OpenRouter models pause when this limit is reached. Free models do not count."}
        {l.unknown_charges > 0 ? ` ${l.unknown_charges} request${l.unknown_charges === 1 ? " was" : "s were"} never priced, so ${l.unknown_charges === 1 ? "it counts" : "they count"} at the most ${l.unknown_charges === 1 ? "it" : "they"} could have cost.` : ""}
      </p>
      <LimitForm busy={busy} guard={guard} button="Change the limit" />
    </section>
  );
}

export default function CostTab({ ctx }: { ctx: Ctx }) {
  const { s, busy, guard } = ctx;
  const [limit, setLimit] = useState("300000");
  const [per, setPer] = useState("100000");
  const [raise, setRaise] = useState("");
  const [err, setErr] = useState("");
  const b = s.budget;
  const pct = b ? Math.min(100, Math.round((b.spent / b.limit) * 100)) : 0;
  const win = planWindows(s.meters.plan_window);
  const models = s.tokens_by_model;
  const biggest = models.reduce((m, x) => Math.max(m, x.tokens ?? 0), 0);
  const label = (task: string) => {
    const id = s.artifacts.find((a) => a.from_task === task)?.name;
    return id ? (s.plan?.stages.find((x) => x.id === id)?.title ?? id) : task === "plan" ? "Planning" : task;
  };
  const set = () => guard(async () => {
    try { await api.call("set_budget", { limit: amount(limit)!, perTask: amount(per)! }); setErr(""); } catch (e) { setErr(text(e)); throw e; }
  });
  const tokensKnown = !s.meters.tokens.startsWith("unknown");
  return (
    <>
      <SectionIntro title="Usage & limits">What this project has used and the limits on it. An unknown amount is never counted as zero.</SectionIntro>
      {s.project_limit ? <div className="stat-row"><ProjectLimitStat l={s.project_limit} busy={!!busy} guard={guard} /></div> : (
        <div className="stat-row">
          <section className="stat">
            <p className="muted">This project</p>
            <p className="big-number unknown">Cost not available</p>
            <p className="muted">Usage counts are not a complete billing total. Check your API limits and provider billing.</p>
          </section>
        </div>
      )}

      {/* The limits that stop work stay in view: a cap that is reached is not something to find in a closed section. */}
      <SpendCap busy={busy} />

      <details className="usage-details" open={b?.paused || undefined}>
        <summary>Details: tokens, plan usage and limits for all projects</summary>
      <div className="stat-row">
        <section className="stat">
          <p className="muted">Tokens in this project</p>
          <p className={`big-number${tokensKnown ? "" : " unknown"}`}>{tokensKnown ? n(Number(s.meters.tokens)) : "Unknown"}</p>
          <p className="muted">{models.length > 0 ? `across ${models.filter((m) => m.model !== "planning").length || 1} ${models.length === 1 ? "model" : "models"}` : "No step has run yet."}</p>
        </section>
      </div>

      <section className="sheet">
        <header className="sheet-head"><div><h2>Plan usage</h2><p className="muted">How much of each plan your providers say is used. SproutCrew stops starting new work when one reaches 50 percent.</p></div></header>
        {win.length === 0 ? <p className="empty-note">Unknown until a step has run on a plan.</p> : (
          <ul className="bars">
            {win.map((w) => (
              <li key={w.name}>
                <span className="bars-label">{w.name}</span>
                <HBar value={w.pct / 100} mark={0.5} tone={w.pct >= 50 ? "bad" : w.pct >= 40 ? "warn" : "ok"} />
                <span className="bars-num">{Math.round(w.pct)}%</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <UsageBreakdown ctx={ctx} />

      {models.length > 0 && (
        <section className="sheet">
          <header className="sheet-head"><div><h2>Tokens by model</h2></div></header>
          <ul className="bars">
            {models.map((m) => (
              <li key={`${m.route}-${m.model}`}>
                <span className="bars-label">{m.model === "planning" ? "Planning" : m.model}<span className="muted"> {m.route ? provider(m.route) : ""}</span></span>
                <HBar value={biggest ? (m.tokens ?? 0) / biggest : 0} tone="info" />
                <span className={`bars-num${m.tokens === null ? " unknown" : ""}`}>{m.tokens === null ? "unknown" : n(m.tokens)}{m.unknown_token_steps > 0 && m.tokens !== null ? ` + ${m.unknown_token_steps} unknown` : ""}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="sheet">
        <details>
          <summary>By step</summary>
          {s.steps.length === 0 ? <Empty>No step has run yet.</Empty> : (
            <table className="table">
              <thead><tr><th>Step</th><th>Provider</th><th className="num">Time</th><th className="num">Tokens</th><th>Plan quota after</th></tr></thead>
              <tbody>
                {s.steps.map((r) => (
                  <tr key={r.step}>
                    <td>{label(r.step)}</td>
                    <td>{s.timings[r.step] ? provider(s.timings[r.step].route) : ""}</td>
                    <td className="num">{s.timings[r.step] ? secs(s.timings[r.step].seconds) : ""}</td>
                    <td className={`num ${r.tokens === "unknown" ? "unknown" : ""}`}>{r.tokens}</td>
                    <td className={r.plan_window === "unknown" ? "unknown" : ""}>{r.plan_window}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </details>
        <details open={b?.paused || undefined}>
          <summary>A limit for this project</summary>
          <p className="muted">Each step holds back its share before it starts, so two steps cannot spend the same allowance. When the limit is reached, steps pause until you raise it.</p>
          {b ? (
            <>
              <div className="hbar-row"><HBar value={b.spent / b.limit} tone={b.paused ? "bad" : "ok"} /></div>
              <p className="budget-line">
                <strong>{n(b.spent)}</strong> of {n(b.limit)} tokens used, {n(b.available)} left{" "}
                {b.paused && <Chip tone="bad">Paused</Chip>}
                {b.unknown_charges > 0 && <Chip tone="quiet">{b.unknown_charges} charged at their reservation</Chip>}
                <span className="muted">{pct}%</span>
              </p>
              <div className="actions tight">
                <input aria-label="New limit" inputMode="numeric" value={raise} onChange={(e) => setRaise(e.target.value)} placeholder="New, higher limit in tokens" />
                <button disabled={amount(raise) === null || !!busy} onClick={() => guard(async () => { try { await api.call("raise_budget", { newLimit: amount(raise)! }); setRaise(""); setErr(""); } catch (e) { setErr(text(e)); throw e; } })}>Raise the limit</button>
              </div>
            </>
          ) : (
            <div className="grid-row budget">
              <label>Limit in tokens<input inputMode="numeric" value={limit} onChange={(e) => setLimit(e.target.value)} /></label>
              <label>Held back per step<input inputMode="numeric" value={per} onChange={(e) => setPer(e.target.value)} /></label>
              <button className="primary" disabled={!!busy || amount(limit) === null || amount(per) === null} onClick={set}>Set the limit</button>
            </div>
          )}
          {err && <p className="field-error" role="alert">{err}</p>}
        </details>
      </section>
      </details>
    </>
  );
}
