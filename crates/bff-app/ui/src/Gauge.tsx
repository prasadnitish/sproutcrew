import { Spend, State } from "./api";
import { useSpring } from "./spring";
import { costParts } from "./failures";

/** One meter (docs/DESIGN.md §5): a label, a thin bar that moves on a spring, and the number in tabular mono. It
 *  is present but never the loudest thing. The tone changes only when the number is worth noticing, and the number
 *  and the words say the same thing. */
export function Meter({ value, center, label, sub, warnAt = 0.8, tick, onClick, unknown, tone: forced }: {
  value: number; center: string; label: string; sub?: string; warnAt?: number; tick?: number; onClick?: () => void; unknown?: boolean; tone?: string;
}) {
  const v = unknown ? 0 : Math.max(0, Math.min(1, value));
  const tone = unknown ? "quiet" : forced ?? (v >= 1 ? "bad" : v >= warnAt ? "warn" : "ok");
  const w = useSpring(v).value;
  return (
    <button className={`meter ${tone}`} onClick={onClick} type="button" aria-label={`${label}: ${unknown ? "unknown" : center}${sub ? `. ${sub}` : ""}`}>
      <span className="meter-label">{label}</span>
      <span className="meter-num">{unknown ? "unknown" : center}</span>
      <span className="meter-track" aria-hidden="true">
        <span className="meter-fill" style={{ width: `${Math.max(0, Math.min(1, w)) * 100}%` }} />
        {tick !== undefined && <span className="meter-tick" style={{ left: `${tick * 100}%` }} />}
      </span>
      {sub && <span className="meter-sub">{sub}</span>}
    </button>
  );
}

/** The windows a plan login reports, such as `claude 27%, codex 2%`. */
export function planWindows(text: string): { name: string; pct: number }[] {
  return [...text.matchAll(/([a-z]+)\s+(\d+(?:\.\d+)?)%/gi)].map((m) => ({ name: m[1][0].toUpperCase() + m[1].slice(1), pct: Number(m[2]) }));
}

/** Progress and cost, always in view while a project is open. Each opens Usage, where the token allowance is. */
export function Hud({ s, onOpen }: { s: State; tokens?: Spend | null; onOpen: () => void }) {
  const total = s.plan?.stages.length ?? 0;
  const done = s.plan ? s.plan.stages.filter((st) => s.artifacts.some((a) => a.name === st.id)).length : 0;

  return (
    <div className="hud" role="group" aria-label="Progress and use">
      <Meter value={total ? done / total : 0} center={`${done}/${total}`} label="Progress" sub={total ? (done === total ? "All steps done" : `${total - done} to go`) : "No plan yet"} tone="info" onClick={onOpen} unknown={total === 0} />
      {s.artifacts.length === 0 && s.meters.api_dollars === "unknown"
        ? <Meter value={0} center="–" label="Cost" sub="No runs yet" tone="quiet" onClick={onOpen} />
        : <CostMeter text={s.meters.api_dollars} onOpen={onOpen} />}
    </div>
  );
}

function CostMeter({ text, onOpen }: { text: string; onOpen: () => void }) {
  if (text === "unknown") return <Meter value={0} center="unknown" label="Cost" sub="Not reported or priced yet" onClick={onOpen} unknown />;
  const { center, sub } = costParts(text);
  return <Meter value={0} center={center} label="Cost" sub={sub} onClick={onOpen} />;
}
