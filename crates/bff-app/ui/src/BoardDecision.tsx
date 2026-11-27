import { useCallback, useEffect, useRef, useState } from "react";
import { api, State } from "./api";
import { costText, Estimate, tokenCapWarning } from "./Preflight";
import { modelOf } from "./graph";
import Questions from "./Questions";
import { kindOf, primaryOf, riskWord, DecisionItem } from "./queue";
import { runner, stopFlag } from "./runner";
import { Ctx, roleName, Section, setRoleLabels, shortHash, text } from "./ui";

type Tab = "work" | "material" | "team" | "share" | "usage";
type Fit = { looser: string[]; stricter: string[] };
const FIELD: Record<string, string> = { send_outside: "sending outside this Mac", delete_files: "deleting files", install_software: "installing software", privacy: "what providers keep" };

/** The project an item belongs to, opened so the board shows the version an approval would cover (DESIGN_SPEC.md
 *  §14). Opening is refused while another project runs, and then the board says so and decides nothing. */
function useProject(name: string) {
  const [s, setS] = useState<State | null>(null);
  const [why, setWhy] = useState("");
  const load = useCallback(async () => {
    if (runner.project && runner.project !== name) { setS(null); setWhy(`${runner.project} is running. Decide this when that run ends, or stop it.`); return; }
    try {
      await api.call("open_project", { name });
      const st = await api.call<State>("get_state", { engine: "live" });
      setRoleLabels(st.role_info);
      setS(st); setWhy("");
    } catch (e) { setS(null); setWhy(text(e)); }
  }, [name]);
  useEffect(() => { setS(null); setWhy(""); void load(); }, [load]);
  // A run that starts or ends anywhere changes what can be decided here, so the project is read again.
  useEffect(() => runner.subscribe({ change: () => void load() }), [load]);
  return { s, why, load };
}

const guardWord = (v: string | undefined, ask: string, never: string) => (v === "never" ? never : v === "ask_each_time" ? ask : `${ask}, for some roles`);

/** One decision from the board, made here with the same calls its project page makes. Kinds that need the whole
 *  diff, the exact text or the plan's chat to decide are opened in their project instead. */
export default function BoardDecision({ item, onOpen, onDecided, back, queue, showQueue }: {
  item: DecisionItem; onOpen: (i: DecisionItem, tab?: Tab) => void; onDecided: (said: string) => void; back: () => void; queue: boolean; showQueue: () => void;
}) {
  const { s, why, load } = useProject(item.project);
  const [working, setWorking] = useState("");
  const [err, setErr] = useState("");
  const [refused, setRefused] = useState("");
  const [rejecting, setRejecting] = useState(false);
  const [reason, setReason] = useState("");
  const [estimate, setEstimate] = useState<Estimate | null>(null);
  const [fit, setFit] = useState<Fit | null>(null);
  const k = kindOf(item.kind);
  const primary = useRef<HTMLButtonElement>(null);
  useEffect(() => { setErr(""); setRefused(""); setRejecting(false); setReason(""); }, [item.project, item.kind, item.id]);
  // The cost and how the policy compares with the defaults, for a plan only.
  useEffect(() => {
    if (item.kind !== "plan" || !s?.plan) { setEstimate(null); setFit(null); return; }
    let live = true;
    api.call<Estimate>("plan_preflight").then((e) => live && setEstimate(e)).catch(() => live && setEstimate(null));
    api.call<{ projects: { name: string; fit?: Fit }[] }>("policy_overview").then((v) => live && setFit(v.projects.find((p) => p.name === item.project)?.fit ?? null)).catch(() => undefined);
    return () => { live = false; };
  }, [item.kind, item.project, s?.approval_hash]);

  const act = async (label: string, f: () => Promise<unknown>, said: string) => {
    setWorking(label); setErr(""); setRefused("");
    try { await f(); onDecided(said); }
    catch (e) {
      const m = text(e);
      // A plan that changed after it was read is refused by the core; the board reads it again and says so.
      if (/changed after you/.test(m)) setRefused(m); else setErr(m);
    } finally { setWorking(""); await load(); }
  };
  const ctx: Ctx | null = s && {
    s, engine: "live", busy: working, askBusy: working, running: runner.project !== null, stopping: false, stopFlag,
    guard: async (f) => { setWorking("Saving"); setErr(""); try { await f(); } catch (e) { setErr(text(e)); } finally { setWorking(""); await load(); } },
    track: () => undefined, refresh: load, dirty: false, goTo: (tab) => onOpen(item, tab), onContinue: () => onOpen(item), openProject: () => onOpen(item),
  };
  const start = () => { void runner.start(item.project, "live").catch((e) => setErr(text(e))).finally(() => onDecided("")); };

  // What each kind shows, and its one primary. `off` is why the primary cannot be pressed, said in the footer.
  let body: React.ReactNode = null;
  let main: { label: string; run: () => void; off?: string } | null = null;
  let other: React.ReactNode = null;
  if (s) {
    const plan = s.plan;
    const finding = item.kind === "finding" || (item.kind === "escalation" && item.id.includes(":"));
    if (item.kind === "plan" && plan) {
      const hash = s.approval_hash ?? "";
      const g = s.policy?.guardrails;
      const looser = (fit?.looser ?? []).length > 0;
      const fresh = s.sources.some((x) => !x.role_chosen);
      body = (
        <article className="approval" aria-label="Approval">
          <p className="approval-band"><b>Approval</b> Plan <span className="mono">{shortHash(hash)}</span></p>
          <div className="approval-body">
            <h3 className="approval-title">Run {plan.stages.length} {plan.stages.length === 1 ? "step" : "steps"} of {item.project}</h3>
            <Section title="What will run" value={`${plan.stages.length} ${plan.stages.length === 1 ? "step" : "steps"}`} open={!looser && !fresh}>
              <ol className="bd-steps">{plan.stages.map((st) => { const m = modelOf(plan.team, st); return <li key={st.id}><strong>{st.title}</strong> <span className="muted">{roleName(st.owner_role)}{m ? <> · <span className="mono">{m.model}</span></> : null}</span></li>; })}</ol>
            </Section>
            <Section title="Cost" flag={estimate && tokenCapWarning(estimate) ? "Over the token cap" : undefined} tone="warn" open={!!estimate && !!tokenCapWarning(estimate)} value={estimate ? (estimate.max_usd === null ? "unknown" : `at most $${estimate.max_usd.toFixed(2)}${s.project_limit ? ` of $${s.project_limit.cap.toFixed(2)}` : ""}`) : "reading"}>
              <p>{estimate ? `This plan costs ${costText(estimate)}` : "Reading the prices."}{s.project_limit ? ` Paid models stop at $${s.project_limit.cap.toFixed(2)} for this project.` : ""}</p>
              {estimate && tokenCapWarning(estimate) && <p className="notice warn" role="alert">{tokenCapWarning(estimate)}</p>}
            </Section>
            <Section title="What it reads" value={`${s.sources.length} ${s.sources.length === 1 ? "source" : "sources"}`} flag={fresh ? "New" : undefined} tone="info" open={fresh}>
              {s.sources.length ? <ul className="plain">{s.sources.map((x) => <li key={x.id}>{x.title || x.name}</li>)}</ul> : <p className="muted">No sources. The crew works from the brief.</p>}
            </Section>
            <Section title="What it may do" value={looser ? "looser than your defaults" : "as approved"} flag={looser ? "Looser than your defaults" : undefined} open={looser}>
              <ul className="plain">
                <li>{guardWord(g?.send_outside, "Asks before sending outside this Mac", "Never sends outside this Mac")}</li>
                <li>{guardWord(g?.delete_files, "Asks before deleting", "Never deletes")}</li>
                <li>{guardWord(g?.install_software, "Asks before installing software", "Never installs software")}</li>
                <li>Never runs commands</li>
              </ul>
              {looser && <p>Looser than your defaults on {fit!.looser.map((f) => FIELD[f] ?? f).join(" and ")}.</p>}
            </Section>
            <Section title="What the approval covers" value={shortHash(hash)}>
              <p>The plan, its policy, its sources, the files it reads, its budget and the API settings, as this version: <span className="mono bd-hash">{hash}</span>. If any of them changes before you approve, the approval is refused and nothing runs.</p>
            </Section>
          </div>
        </article>
      );
      const off = s.problems.length ? s.problems[0] : !hash ? "This plan has no version to approve yet." : s.gate === "allowed" ? "This plan is already approved." : undefined;
      const approve = () => api.call("approve", { project: item.project, hash });
      const others = runner.project !== null;
      main = others
        ? { label: "Approve only", run: () => void act("Approving", approve, `Approved ${item.project}'s plan. It starts when you choose Run the rest.`), off }
        : { label: "Approve and start", run: () => void act("Approving", async () => { await approve(); start(); }, `Approved ${item.project}'s plan. It is running.`), off };
      if (!others) other = <button disabled={!!working || !!off} onClick={() => void act("Approving", approve, `Approved ${item.project}'s plan. Nothing runs until you start it.`)}>Approve only</button>;
    } else if (item.kind === "questions" && ctx) {
      body = <Questions ctx={ctx} onAnswered={() => { start(); onDecided(`Answers saved. ${item.project} carries on.`); }} />;
    } else if (finding) {
      const cut = item.id.lastIndexOf(":");
      const artifact = item.id.slice(0, cut), fid = item.id.slice(cut + 1);
      const f = s.review?.[artifact]?.findings.find((x) => x.id === fid);
      body = (
        <Section title="The evidence" value={f?.escalated ? "escalated to you" : "a second check"} open>
          <p className="prose">{f?.claim ?? item.title}</p>
          {f?.evidence && <p className="muted">{f.evidence}</p>}
          {rejecting && <label className="bd-reason">Why are you rejecting it?<input autoFocus value={reason} onChange={(e) => setReason(e.target.value)} placeholder="A reason is kept with the decision" maxLength={2000} /></label>}
        </Section>
      );
      const decide = (decision: "accept" | "reject") => act(decision === "accept" ? "Accepting" : "Rejecting", () => api.call("decide", { project: item.project, artifact, finding: fid, decision, reason: decision === "reject" ? reason : "" }), decision === "accept" ? "Accepted the finding." : "Rejected the finding, with your reason.");
      main = rejecting
        ? { label: "Reject this point", run: () => void decide("reject"), off: reason.trim() ? undefined : "A rejection needs a reason." }
        : { label: "Accept", run: () => void decide("accept"), off: f ? undefined : "This finding is no longer open." };
      other = rejecting ? <button onClick={() => { setRejecting(false); setReason(""); }}>Cancel</button> : <button disabled={!f || !!working} onClick={() => setRejecting(true)}>Reject</button>;
    } else if (item.kind === "action") {
      const a = s.policy?.pending.find((x) => x.id === item.id);
      body = (
        <Section title="What it wants to do" value={riskWord(item.tier)} open>
          {a ? <p>{roleName(a.action.stage)} wants to {a.action.tool.replace(/_/g, " ")}: <span className="mono">{a.action.target}</span>. Only this exact action is allowed.</p> : <p className="muted">This action is no longer waiting.</p>}
        </Section>
      );
      const decide = (approve: boolean) => act(approve ? "Allowing" : "Refusing", () => api.call("decide_actions", { project: item.project, ids: [item.id], approve }), approve ? "Allowed the action, once." : "Refused the action.");
      main = { label: "Allow this action", run: () => void decide(true), off: a ? undefined : "This action is no longer waiting." };
      other = <button disabled={!a || !!working} onClick={() => void decide(false)}>Refuse</button>;
    } else if (item.kind === "failure") {
      body = <Section title="What stopped it" value="failed" open><p>{item.title.replace(/^A step stopped: /, "")}</p></Section>;
      main = { label: "Try again", run: () => void act("Running", () => api.run("run_stage", { engine: "live", stageId: item.stage }, () => undefined), "Ran the step again."), off: s.gate !== "allowed" ? "The plan is not approved, so the step cannot run." : runner.project ? "A run is going. Try again when it ends." : undefined };
      other = <button onClick={() => onOpen(item, "work")}>Choose another model</button>;
    } else if (item.kind === "stopped") {
      const done = plan ? plan.stages.filter((st) => s.artifacts.some((x) => x.name === st.id)).length : 0;
      body = <Section title="What has not run" value={`${(plan?.stages.length ?? 0) - done} ${(plan?.stages.length ?? 0) - done === 1 ? "step" : "steps"}`} open><p>{done} of {plan?.stages.length ?? 0} steps have results. You stopped the run, and nothing has run since.</p></Section>;
      main = { label: "Run the rest", run: () => { start(); onDecided(`${item.project} is running the rest.`); }, off: s.gate !== "allowed" ? "The plan is not approved, so nothing can run." : runner.project ? "A run is going. Run the rest when it ends." : undefined };
    } else if (item.kind === "finished" && s.result_review) {
      const rr = s.result_review;
      body = <Section title="The final version" value={`${s.artifacts.length} results`} open><p>Every step has a result. Read them in Results & sharing before you mark them reviewed.</p><button className="link" onClick={() => onOpen(item, "share")}>Open the results</button></Section>;
      main = { label: "Mark reviewed", run: () => void act("Marking", () => api.call("mark_reviewed", { project: item.project, version: rr.version }), `Marked ${item.project}'s results reviewed.`), off: rr.reviewed ? "These results are already reviewed." : undefined };
    } else {
      // Code diffs, external writes, plan changes and missing details need the whole of what they decide on screen.
      body = <Section title="Where to decide it" value={`in ${item.project}`} open><p>{k.word === "Code diff" ? "A diff is approved file by file, after you have read each one." : k.word === "External write" ? "A write is approved as its exact text, then sent as a separate press." : k.word === "Plan change" ? "A plan change is read with the conversation that made it." : "The missing details are filled in on the draft itself."} It opens in {item.project}.</p></Section>;
      main = { label: `Open in ${item.project}`, run: () => onOpen(item) };
    }
  }

  // ⌘⏎ is the open item's primary.
  const now = useRef(main);
  now.current = main;
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (!(e.metaKey && e.key === "Enter") || document.querySelector("dialog[open]")) return;
      const m = now.current;
      if (m && !m.off) { e.preventDefault(); m.run(); }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  const off = working ? `${working}…` : main?.off;
  const approval = item.kind === "plan";
  return (
    <>
      <div className="bd-scroll">
        <div className="bd-crumb"><button className="link bd-back" onClick={back}>Back to the queue</button>{!queue && <button className="link" onClick={showQueue}>Show the queue <kbd>[</kbd></button>}<span>{item.project}{item.step_title ? ` · ${item.step_title}` : ""}</span></div>
        <h2 className="bd-h">{item.title}</h2>
        {refused && <div className="refused" role="alert"><p><strong>Nothing was approved.</strong> {refused}</p><p>The board has read the new version. Look at it again before you approve.</p></div>}
        {err && <p className="notice error" role="alert">{err}</p>}
        {!s && why && <p className="note">{why}</p>}
        {!s && !why && <p className="muted" role="status">Opening {item.project}…</p>}
        <div className="bd-sections">{body}</div>
      </div>
      <div className={`bd-foot${approval ? " approval-foot" : ""}`}>
        {main && <button ref={primary} className="primary" disabled={!s || !!off} aria-describedby={off ? "bd-why" : undefined} onClick={main.run}>{main.label} {!off && <kbd>⌘⏎</kbd>}</button>}
        {!main && item.kind !== "questions" && <button className="primary" disabled aria-describedby="bd-why">{primaryOf(item.kind)}</button>}
        {other}
        {main?.label !== `Open in ${item.project}` && <button className="quiet" onClick={() => onOpen(item)}>Open in {item.project} <kbd>O</kbd></button>}
        {(off || (!s && why)) && <p className="bd-why" id="bd-why">{off ?? why}</p>}
      </div>
    </>
  );
}
