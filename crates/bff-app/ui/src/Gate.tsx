import { ReactNode, useEffect, useRef, useState } from "react";
import { STANDARD, useSpring } from "./spring";

/** The approval gate (docs/DESIGN.md §5): an overlay sheet for a decision a person makes about something an agent
 *  is about to do. What will happen is in plain words, the exact payload is in mono, Approve is the one accent
 *  button and sits at the far end from Reject, ⌘⏎ approves and Esc rejects. It never closes by itself. */
export default function Gate({ title, what, payload, approveLabel, rejectLabel = "Cancel", onApprove, onReject, busy, children }: {
  title: string;
  what: ReactNode;
  payload?: string;
  approveLabel: string;
  rejectLabel?: string;
  onApprove: () => void;
  onReject: () => void;
  busy?: boolean;
  children?: ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [shown, setShown] = useState(false);
  const k = useSpring(shown ? 1 : 0, STANDARD).value;
  useEffect(() => {
    setShown(true);
    const before = document.activeElement as HTMLElement | null;
    box.current?.focus();
    return () => before?.focus?.();
  }, []);
  const key = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onReject(); }
    else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); if (!busy) onApprove(); }
    else if (e.key === "Tab" && box.current) {
      const f = [...box.current.querySelectorAll<HTMLElement>("button:not(:disabled), summary, [tabindex='0']")];
      if (f.length === 0) return;
      const first = f[0], last = f[f.length - 1];
      if (e.shiftKey && (document.activeElement === first || document.activeElement === box.current)) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    }
  };
  return (
    <div className="scrim" style={{ opacity: Math.min(k, 1) }}>
      <div ref={box} className="gate" role="dialog" aria-modal="true" aria-labelledby="gate-title" tabIndex={-1} onKeyDown={key}
        style={{ opacity: Math.min(k, 1), transform: `translateY(${(1 - k) * -12}px) scale(${0.98 + 0.02 * k})` }}>
        <p className="eyebrow">Your approval is needed</p>
        <h2 id="gate-title" className="gate-title">{title}</h2>
        <div className="gate-what">{what}</div>
        {payload !== undefined && <pre className="payload" tabIndex={0} aria-label="Exactly what will be sent">{payload}</pre>}
        {children}
        <div className="gate-actions">
          <button className="ghost" onClick={onReject} disabled={false}>{rejectLabel} <kbd>Esc</kbd></button>
          <button className="primary" onClick={onApprove} disabled={busy}>{approveLabel} <kbd>⌘⏎</kbd></button>
        </div>
      </div>
    </div>
  );
}

/** A labeled placeholder for something that is loading. Never a blank wait, never an unlabeled spinner. */
export function Skeleton({ label, lines = 3 }: { label: string; lines?: number }) {
  return (
    <div className="skeleton" role="status" aria-label={label}>
      {Array.from({ length: lines }, (_, i) => <span key={i} className="skeleton-line" style={{ width: `${92 - i * 14}%` }} />)}
      <span className="meta">{label}</span>
    </div>
  );
}
