import { useState } from "react";
import { DiagramStep, layout, wrap } from "./diagram";

type Status = "done" | "next" | "waiting" | "running" | "failed";

/** The plan as a picture: the writers on the upper lane, the checkers on the lower, and a line from each step to the
 *  next. Every step runs after the one before it, and the picture says so. Click a step to read its result. */
export default function Diagram({ steps, status, who, focus, onPick, detail, runs }: {
  steps: DiagramStep[];
  status: (id: string) => Status;
  who: (id: string) => string;
  focus: string;
  onPick: (id: string) => void;
  /** Model, time and cost of the step's latest run, for hovering. */
  detail?: (id: string) => string;
  /** How many results the step has made; more than one means it was run again. */
  runs?: (id: string) => number;
}) {
  const [fit, setFit] = useState(false);
  const l = layout(steps);
  const parallel = l.nodes.some((n, i) => l.nodes.some((m, j) => i < j && m.x === n.x));
  const at = (id: string) => l.nodes.find((n) => n.id === id)!;
  return (
    <figure className="diagram">
      <div className="diagram-tools">
        <ul className="diagram-legend" aria-label="What the marks mean">
          <li><span className="lg done" aria-hidden="true" /> Done</li>
          <li><span className="lg running" aria-hidden="true" /> Running</li>
          <li><span className="lg failed" aria-hidden="true" /> Did not finish</li>
          <li><span className="lg waiting" aria-hidden="true" /> Waiting</li>
        </ul>
        <label className="check"><input type="checkbox" checked={fit} onChange={(e) => setFit(e.target.checked)} /> Fit to width</label>
      </div>
      <div className="diagram-scroll">
        <svg viewBox={`0 0 ${l.width} ${l.height}`} width={fit ? "100%" : l.width} height={fit ? undefined : l.height} preserveAspectRatio="xMinYMin meet" role="group" aria-label="The steps and who does each">
          <text x="8" y={l.lanes.writing + 4} className="lane-label">Writers</text>
          <text x="8" y={l.lanes.checking + 4} className="lane-label">Checkers</text>
          <line x1="0" x2={l.width} y1={l.lanes.checking - 46} y2={l.lanes.checking - 46} className="lane-rule" />
          {l.edges.map((e) => {
            const mid = (e.x1 + e.x2) / 2;
            const side = at(e.from).side;
            return <path key={e.from + e.to} d={`M${e.x1 + 20},${e.y1} C${mid},${e.y1} ${mid},${e.y2} ${e.x2 - 20},${e.y2}`} className={`edge ${side} ${status(e.from) === "done" ? "done" : ""}`} fill="none" />;
          })}
          {steps.map((s) => {
            const n = at(s.id);
            const st = status(s.id);
            const times = runs?.(s.id) ?? 0;
            const more = detail?.(s.id) ?? "";
            return (
              <g key={s.id} className={`dnode ${s.side} ${st}${s.id === focus ? " sel" : ""}`} transform={`translate(${n.x},${n.y})`} tabIndex={0} role="button" aria-label={`${s.title}. ${who(s.id)}. ${st}`} aria-current={s.id === focus ? "step" : undefined}
                onClick={() => onPick(s.id)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onPick(s.id); } }}>
                <title>{[s.title, more, times > 1 ? `Run ${times} times` : ""].filter(Boolean).join("\n")}</title>
                <circle r="20" className="dcircle" />
                {st === "done" && <path d="M-7,1 L-2,6 L8,-5" className="dcheck" fill="none" />}
                {st === "running" && <circle r="6" className="dpulse" />}
                {st === "failed" && <path d="M-6,-6 L6,6 M6,-6 L-6,6" className="dcross" fill="none" />}
                {st !== "done" && st !== "running" && st !== "failed" && <circle r="4" className="ddot" />}
                {times > 1 && <text x="22" y="-14" className="druns">×{times}</text>}
                {wrap(s.title, 15).map((t, i) => (
                  <text key={i} y={(s.side === "writing" ? 38 : 38) + i * 14} textAnchor="middle" className="dlabel">{t}</text>
                ))}
                <text y={-30} textAnchor="middle" className="dwho">{who(s.id)}</text>
              </g>
            );
          })}
        </svg>
      </div>
      <figcaption className="muted">{parallel ? "Steps stacked in one column do not wait for each other, so they run at the same time." : "Each step runs after the one before it."}</figcaption>
    </figure>
  );
}
