import { useEffect, useState } from "react";
import { api } from "./api";
import { useModelName } from "./ModelPicker";
import { money } from "./models";

/** What a plan would ask of the providers before it is approved. Dollar figures are null when unknown. */
export type Estimate = {
  steps: number;
  models: [string, number][];
  free_requests: number;
  free_remaining: number | null;
  free_short: boolean;
  estimated_usd: number | null;
  max_usd: number | null;
  /** Set when the model prices could not be loaded, which is why the cost is unknown. */
  prices_error?: string | null;
};

/** Reads the plan's estimate once; `revision` re-reads it after the plan changes. */
export function useEstimate(revision: unknown): Estimate | null {
  const [e, setE] = useState<Estimate | null>(null);
  useEffect(() => {
    let live = true;
    // An estimate for the old plan is never shown as if it were for the new one.
    setE(null);
    api.call<Estimate>("plan_preflight").then((x) => live && setE(x)).catch(() => live && setE(null));
    return () => { live = false; };
  }, [revision]);
  return e;
}

/** "about $0.03, at most $0.12", or "unknown". Unknown is never shown as $0. */
export function costText(e: Estimate): string {
  if (e.max_usd === null && e.prices_error) return `unknown. ${e.prices_error}`;
  if (e.max_usd === null) return "unknown, because at least one model's price is not known.";
  if (e.max_usd === 0) return "free. Every step uses OpenRouter's free allowance.";
  return `about ${money(e.estimated_usd)}, at most ${money(e.max_usd)}.`;
}

export const costLine = (e: Estimate): string => `Cost: ${costText(e)}`;

function ModelCount({ id, n }: { id: string; n: number }) {
  return <li>{useModelName(id)} <span className="muted">· {n} step{n === 1 ? "" : "s"}</span></li>;
}

/** The plan in numbers: steps, models, cost and, when it applies, a warning that today's free allowance is too small.
 *  `compact` is for the plan card, which names the steps and the models itself: only the cost and the warnings. */
export default function Preflight({ e, compact }: { e: Estimate | null; compact?: boolean }) {
  if (!e) return <p className="muted">Checking prices…</p>;
  const cost = costText(e);
  return (
    <div className="preflight">
      {!compact && <p><strong>{e.steps} steps</strong>, one request each at least; a revision or a second opinion adds more.</p>}
      {!compact && <ul className="plain models-used">{e.models.map(([id, n]) => <ModelCount key={id} id={id} n={n} />)}</ul>}
      <p>{compact ? `${cost.charAt(0).toUpperCase()}${cost.slice(1)}` : `Cost: ${cost}`}</p>
      {e.free_short && (
        <p className="notice warn" role="alert">
          This plan needs about {e.free_requests} requests on OpenRouter's free allowance, and {e.free_remaining} {e.free_remaining === 1 ? "is" : "are"} left today. It is likely to stop part way. Choose a paid model for some steps, or add $10 of credit to raise the allowance to 1,000 a day.
        </p>
      )}
      {e.free_requests > 0 && !e.free_short && e.free_remaining === null && (
        <p className="muted">{e.free_requests} steps use OpenRouter's free allowance (50 requests a day, or 1,000 with $10 of credit). How much is left today could not be checked.</p>
      )}
    </div>
  );
}
