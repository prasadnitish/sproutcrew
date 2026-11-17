import { useEffect, useState } from "react";
import { api } from "./api";
import { text } from "./ui";
export type DecisionItem = { project: string; stage: string; id: string; tier: string; kind: string; title: string; anchor: string };
type Inbox = { items: DecisionItem[]; errors: { project: string; error: string }[] };
export default function DecisionInbox({ busy, onOpen, onProjects }: { busy: boolean; onOpen: (item: DecisionItem) => void; onProjects: () => void }) {
  const [data, setData] = useState<Inbox | null>(null);
  const [error, setError] = useState("");
  const load = async () => { try { setData(await api.call<Inbox>("decision_inbox")); setError(""); } catch (e) { setError(text(e)); } };
  useEffect(() => {
    let disposed = false, checking = false;
    const refresh = async () => {
      if (checking) return;
      checking = true;
      try { const next = await api.call<Inbox>("decision_inbox"); if (!disposed) { setData(next); setError(""); } }
      catch (e) { if (!disposed) setError(text(e)); }
      finally { checking = false; }
    };
    void refresh(); const timer = window.setInterval(() => void refresh(), 5000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, []);
  return <section aria-label="Pending decisions across projects">
    <p className="lede">Every pending plan, action, review finding, stopped step and blocked judgment call, across your projects.</p>
    <button className="ghost" disabled={busy} onClick={() => void load()}>Refresh decisions</button>
    {api.mocked && <p className="notice">Browser layout preview. Open the native app for real pending decisions.</p>}
    {error && <p className="notice error" role="alert">{error}</p>}
    {!data && !error && <p role="status">Reading project decisions…</p>}
    {data?.errors.map(e => <p className="notice error" role="alert" key={e.project}>{e.project}: {e.error}. Its pending decisions could not be loaded.</p>)}
    {data && data.items.length === 0 && data.errors.length === 0 && <div className="hero"><h2>Your board is clear.</h2><p>Your agents have no decisions waiting for you.</p><button onClick={onProjects}>Open your projects</button></div>}
    {data && data.items.length > 0 && <ol className="decision-list">{data.items.map(item => <li key={`${item.project}:${item.kind}:${item.id}`}><article className="panel"><p className="eyebrow">{item.tier} · {item.kind} · {item.project}</p><h2>{item.title}</h2><p className="muted">Step: {item.stage}</p><button disabled={busy} onClick={() => onOpen(item)}>Review in project</button></article></li>)}</ol>}
  </section>;
}
