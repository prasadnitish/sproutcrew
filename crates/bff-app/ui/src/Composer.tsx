import { ReactNode } from "react";

/** The composer (docs/DESIGN.md §5): a multi-line field on a raised plane, one quiet menu for who answers, and one
 *  accent button to send. ⌘⏎ sends. When sending is not possible it says why, in words, next to the button. */
export default function Composer({ label, value, onChange, onSend, sendLabel, placeholder, menu, extra, blocked, max = 2000, rows = 3 }: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  onSend: () => void;
  sendLabel: string;
  placeholder: string;
  /** The one menu, such as who answers. */
  menu?: ReactNode;
  /** More actions next to the send button. */
  extra?: ReactNode;
  /** Why sending is not possible right now, if it is not. */
  blocked?: string;
  max?: number;
  rows?: number;
}) {
  const why = blocked || (!value.trim() ? "Type something to send." : "");
  return (
    <div className="composer">
      <label className="composer-label" htmlFor={`c-${label}`}>{label}</label>
      <textarea id={`c-${label}`} rows={rows} value={value} maxLength={max} spellCheck={false} placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !why) { e.preventDefault(); onSend(); } }} />
      <div className="composer-bar">
        {menu && <div className="composer-menu">{menu}</div>}
        <span className="meta composer-why" role="status">{why}</span>
        {extra}
        <button className="primary" disabled={!!why} onClick={onSend}>{sendLabel} <kbd>⌘⏎</kbd></button>
      </div>
    </div>
  );
}
