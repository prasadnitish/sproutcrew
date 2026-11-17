import { useState } from "react";
import { api } from "./api";
import { Ctx, Panel } from "./ui";
export default function CodeReview({ ctx }: { ctx: Ctx }) {
  const code = ctx.s.code_review;
  const [editing, setEditing] = useState<string | null>(null);
  const [bundle, setBundle] = useState("");
  if (!code || code.items.length === 0) return null;
  return <Panel title="Code review board" help="Writers draft in separate git worktrees. Code stays in review until you approve the exact diff. Generated commands, dependency installs and tests never execute here.">
    {code.items.map(d => <article className="sheet" key={d.task} id={`code-${d.task}`}><h3>{d.stage} · {d.status}</h3><p>{d.summary}</p><p className="mono">Base: {d.base}<br />Patch: {d.patch_hash || "not staged"}</p>
      {d.patch && <details open={d.status === "pending" || d.status === "stale"}><summary>Read the exact diff</summary><pre className="log-text" tabIndex={0}>{d.patch}</pre></details>}
      {["stale", "needs_permission", "drafting"].includes(d.status) && <div className="actions"><button className="ghost" disabled={!!ctx.busy} onClick={() => ctx.guard(() => api.call("decide_code", { task: d.task, patchHash: d.patch_hash, approve: false, by: "Local user" }))}>Reject code draft and keep current code</button></div>}
      {d.status === "needs_permission" && <button disabled={!!ctx.busy} onClick={() => ctx.guard(() => api.call("stage_code_draft", { task: d.task }))}>Prepare diff after file permission</button>}
      {d.status === "pending" && <><div className="actions"><button disabled={!!ctx.busy || ctx.s.gate !== "allowed"} onClick={() => ctx.guard(() => api.call("decide_code", { task: d.task, patchHash: d.patch_hash, approve: true, by: "Local user" }))}>Approve this exact diff</button></div><div className="actions"><button className="ghost" disabled={!!ctx.busy} onClick={() => ctx.guard(() => api.call("decide_code", { task: d.task, patchHash: d.patch_hash, approve: false, by: "Local user" }))}>Reject code draft</button></div></>}
      {d.status === "stale" && <><p className="notice">The code changed after this draft began. Reconcile against the current base; the old draft cannot overwrite it.</p><p className="mono">Current base: {code.head}</p><button disabled={!!ctx.busy} onClick={() => ctx.guard(async () => { setBundle(JSON.stringify(await api.call("code_bundle", { task: d.task }), null, 2)); setEditing(d.task); })}>Open explicit reconciliation</button></>}
      {editing === d.task && <div><label>Resolved file bundle against the current base<textarea rows={12} value={bundle} onChange={e => setBundle(e.target.value)} spellCheck={false} /></label><p>The replacement is a new draft and needs its own diff approval.</p><button disabled={!!ctx.busy || !code.head} onClick={() => ctx.guard(async () => { await api.call("reconcile_code", { task: d.task, base: code.head, bundle: JSON.parse(bundle) }); setEditing(null); })}>Create reconciled draft</button><button className="ghost" onClick={() => setEditing(null)}>Cancel reconciliation</button></div>}
      {d.status === "applying" && <p className="notice error">This apply was interrupted. Inspect the local code and journal before continuing; no automatic retry is allowed.</p>}
    </article>)}
  </Panel>;
}
