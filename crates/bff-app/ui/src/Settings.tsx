import { pitch } from "./positioning";
import { useEffect, useState } from "react";
import { api, ApiRoute, AttentionStatus, Check, Info, OpenRouterLimits } from "./api";
import LiquidSwitch from "./Switch";
import { KEYS_CHANGED } from "./Pick";
import { applyScheme, DEFAULT_SCHEME, loadScheme, SCHEMES, SchemeId } from "./scheme";
import { Chip, Section, text, radioKeys, useConfirm } from "./ui";
import { applyTheme, loadTheme, Theme } from "./theme";

function Preflight({ busy, guard }: { busy: string; guard: (f: () => Promise<unknown>) => Promise<void> }) {
  const [res, setRes] = useState<{ can_start: boolean; checks: Check[] } | null>(null);
  const [err, setErr] = useState("");
  const [running, setRunning] = useState(false);
  const [at, setAt] = useState("");
  return (
    <Section title="Connection checks" value={res ? `${res.can_start ? "Passed" : "Failed"} · ${at}` : "not run yet"} flag={res && !res.can_start ? "Failed" : undefined} tone="bad">
      <p className="muted">Checks your API setup, the document export tools and the free disk space. Subscription logins are never used.</p>
      <div className="actions">
        <button disabled={!!busy || running} onClick={() => { setRunning(true); void guard(async () => { try { setRes(await api.call("preflight")); setAt(`today at ${new Date().toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`); setErr(""); } catch (e) { setErr(text(e)); throw e; } }).finally(() => setRunning(false)); }}>{running ? "Checking" : "Run the checks"}</button>

      </div>
      {err && <p className="field-error" role="alert">{err}</p>}
      {res && (
        <ul className="checks">
          {res.checks.map((c) => (
            <li key={c.name}>
              <span className={`check-word ${c.status}`}><span aria-hidden="true">{c.status === "pass" ? "✓ " : c.status === "warn" ? "! " : "× "}</span>{c.status === "pass" ? "Passed" : c.status === "warn" ? "Warning" : "Failed"}</span>
              <span>{c.name}</span>
              <span className="muted">{c.detail}</span>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

/** Account setup is always completed on the provider's own website. */
const KEY_CARDS = [
  { route: "R12", name: "OpenRouter · recommended", short: "OpenRouter", site: "OpenRouter", where: "openrouter.ai/keys", limit: "New projects use Claude Haiku 5.5 to write and Mistral Small to check. Both are paid models, and they stop at the project's dollar limit. Free models are an option in the model picker; they have daily and per-minute limits, and availability varies. No paid fallback is selected automatically. OpenRouter forwards requests to the inference provider; both services' data policies apply." },
  { route: "R3", name: "Anthropic (Claude) API", short: "Anthropic", site: "Anthropic", where: "platform.claude.com/settings/keys", limit: "API billing is separate from a Claude subscription. Set a spend limit in a dedicated Anthropic workspace before making its key." },
  { route: "R5", name: "OpenAI API", short: "OpenAI", site: "OpenAI", where: "platform.openai.com/api-keys", limit: "API billing is separate from ChatGPT. Project budgets can be alerts rather than hard spending caps. This app's default limit counts tokens, not dollars." },
  { route: "R9", name: "Google Gemini API", short: "Google Gemini", site: "Google AI Studio", where: "aistudio.google.com/app/apikey", limit: "Billing is set up in Google AI Studio and is separate from a Gemini app subscription. A key on the free tier has low request limits, and Google may use free-tier requests to improve its products. This app's default limit counts tokens, not dollars." },
];

function OpenRouterAllowance() {
  const [limits, setLimits] = useState<OpenRouterLimits | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState("");
  const check = async () => {
    setChecking(true); setLimits(null); setError("");
    try { setLimits(await api.call<OpenRouterLimits>("openrouter_limits")); }
    catch (e) { setError(text(e)); } finally { setChecking(false); }
  };
  return <div>
    <button className="ghost" disabled={checking} onClick={() => void check()}>{checking ? "Checking allowance" : "Check OpenRouter allowance"}</button>
    <p className="muted">Reads account limits without generating text or changing billing.</p>
    {limits && <div role="status">
      <p>{limits.daily ? `${limits.daily.remaining.toLocaleString()} daily free requests remaining of ${limits.daily.limit.toLocaleString()} reported by OpenRouter (${limits.daily.used.toLocaleString()} used).` : "Daily free-request allowance is unknown; OpenRouter did not return a complete, consistent counter."}</p>
      <p className="muted">Checked {new Date(limits.checked_at * 1000).toLocaleString()}. This tier counter does not confirm per-minute limits or upstream provider availability; provider exceptions may apply.</p>
      {limits.daily?.remaining === 0 && <p className="notice">OpenRouter reports no daily free requests remaining. Check its limits before retrying. A higher app token cap does not raise the provider allowance.</p>}
    </div>}
    {error && <p className="field-error" role="alert">{error}</p>}
  </div>;
}

/** Any service that answers in the common chat format: a hosted one, or an app such as Ollama on this Mac. */
function OwnService({ routes, onRoutes }: { routes: ApiRoute[]; onRoutes: (r: ApiRoute[]) => void }) {
  const own = routes.find((r) => r.route === "R13");
  const ready = own?.state === "ready";
  const [address, setAddress] = useState("");
  const [model, setModel] = useState("");
  const [key, setKey] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [open, setOpen] = useState(false);
  useEffect(() => { if (own?.address) setAddress(own.address); if (own?.model) setModel(own.model); }, [own?.address, own?.model]);
  const save = async () => {
    setBusy(true); setErr("");
    try {
      const r = await api.call<{ routes: ApiRoute[]; models: string[] }>("save_own_ai", { address, model, key });
      onRoutes(r.routes); setModels(r.models); setKey("");
    } catch (e) { setErr(text(e)); } finally { setBusy(false); }
  };
  const remove = async () => {
    setBusy(true); setErr("");
    try { onRoutes(await api.call<ApiRoute[]>("remove_own_ai")); setModels([]); } catch (e) { setErr(text(e)); } finally { setBusy(false); }
  };
  return (
    <div className="own-service">
      <Chip tone={ready ? "ok" : "quiet"}>{ready ? "Ready" : "Not set up"}</Chip>
      <span> Your own AI service</span>
      <p className="muted">{ready ? `Requests go to ${own?.address}, using ${own?.model}${own?.has_key ? ", with the key saved in the Keychain" : ", with no key"}.` : "Use a service SproutCrew does not list, or an AI app running on this Mac, such as Ollama or LM Studio."}</p>
      <button className="link" aria-expanded={open} onClick={() => setOpen(!open)}>{open ? "Hide the help" : "What do I type here?"}</button>
      {open && (
        <div className="help-box">
          <p>Many AI services accept requests in the same format as OpenAI. Their help pages call it an “OpenAI-compatible API” and give a <strong>base URL</strong>. You need three things from the service:</p>
          <ol>
            <li><strong>Address</strong>, the base URL. For example <span className="mono">https://api.together.xyz/v1</span>, <span className="mono">https://api.groq.com/openai/v1</span>, or for Ollama on this Mac <span className="mono">http://localhost:11434/v1</span> (LM Studio uses <span className="mono">http://localhost:1234/v1</span>).</li>
            <li><strong>Model name</strong>, exactly as the service lists it, such as <span className="mono">llama3.3</span>. After you check the address, the models it offers are listed below to pick from.</li>
            <li><strong>Key</strong>, if the service gives you one. Apps on this Mac usually need none.</li>
          </ol>
          <p>Your brief, sources and results are sent to this address, so use one you trust. It must start with https://, which encrypts what is sent; plain http:// works only for an app on this Mac. SproutCrew cannot see this service’s prices, so cost shows as unknown and the token limit in Usage &amp; limits applies. Changing the address later needs a new plan approval. A small model on a laptop may write less well than a large hosted one.</p>
        </div>
      )}
      <div className="own-fields">
        <label>Address<input value={address} spellCheck={false} placeholder="https://… or http://localhost:11434/v1" onChange={(e) => setAddress(e.target.value)} /></label>
        <label>Model name
          <input value={model} spellCheck={false} list="own-models" placeholder="such as llama3.3" onChange={(e) => setModel(e.target.value)} />
          <datalist id="own-models">{models.map((m) => <option key={m} value={m} />)}</datalist>
        </label>
        <label>Key (if the service has one)<input type="password" autoComplete="off" spellCheck={false} value={key} placeholder={own?.has_key ? "Saved. Paste again to keep it, or leave empty to remove it" : "Leave empty if there is none"} onChange={(e) => setKey(e.target.value)} /></label>
      </div>
      <div className="install-line">
        <button disabled={busy || !address.trim() || !model.trim()} onClick={() => void save()}>{busy ? "Checking the address" : "Check and save"}</button>
        {ready && <button className="ghost" disabled={busy} onClick={() => void remove()}>Remove</button>}
      </div>
      <p className="muted">Check and save asks the service for its list of models, which costs nothing.</p>
      {models.length > 0 && <p className="muted" role="status">It answered and offers: {models.slice(0, 12).join(", ")}{models.length > 12 ? " and more" : ""}.</p>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </div>
  );
}

function Routes() {
  const [routes, setRoutes] = useState<ApiRoute[]>([]);
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState("");
  const [connectionRevision, setConnectionRevision] = useState(0);
  const [err, setErr] = useState<Record<string, string>>({});
  const [ask, asking] = useConfirm();
  useEffect(() => { api.call<ApiRoute[]>("api_routes").then(setRoutes).catch(() => setRoutes([])); }, []);
  const state = (route: string) => routes.find((r) => r.route === route)?.state;
  const act = (route: string, cmd: "save_key" | "remove_key") => {
    setSaving(route);
    api.call<ApiRoute[]>(cmd, cmd === "save_key" ? { route, key: keys[route] ?? "" } : { route })
      .then((r) => { setRoutes(r); setConnectionRevision(n => n + 1); window.dispatchEvent(new Event(KEYS_CHANGED)); setErr((e) => ({ ...e, [route]: "" })); setKeys((k) => ({ ...k, [route]: "" })); })
      .catch((e) => setErr((x) => ({ ...x, [route]: text(e) })))
      .finally(() => setSaving(""));
  };
  return (
    <Section title="AI connections" value={routes.length ? `${KEY_CARDS.filter((c) => state(c.route) === "ready").map((c) => `${c.short} ready`).join(", ") || "none ready"} · ${KEY_CARDS.filter((c) => state(c.route) !== "ready").length} not set up` : "reading"} flag={routes.length && !KEY_CARDS.some((c) => state(c.route) === "ready") ? "Connect one" : undefined} tone="info">
      <p className="muted">Connect OpenRouter to start, or use a direct Anthropic, OpenAI or Google key. Keys stay in this Mac’s Keychain. Your brief and the context a step needs are sent to the selected service.</p>
      <ul className="checks keys">
        {KEY_CARDS.map((c) => {
          const ready = state(c.route) === "ready";
          return (
            <li key={c.route}>
              <Chip tone={ready ? "ok" : "quiet"}>{ready ? "Ready" : "Not set up"}</Chip>
              <span>{c.name}</span>
              <span className="muted">{ready ? "Saved in the Keychain. Requests count against the token limit in Usage & limits." : `Create your account and API key at ${c.where}.`}</span>
              <div className="tool-more">
                {c.route === "R12" && ready && <OpenRouterAllowance key={connectionRevision} />}
                <p className="muted">{c.limit}</p>
                <button className="ghost" onClick={() => void api.call("open_link", { link: `${c.route}-key` }).catch((e) => setErr((x) => ({...x, [c.route]: text(e)})))}>Open {c.site} key settings ↗</button>
                <div className="install-line">
                  <input type="password" autoComplete="off" spellCheck={false} aria-label={`${c.short} API key`} placeholder={ready ? "Paste a new key to replace it" : "Paste the key"} value={keys[c.route] ?? ""} onChange={(e) => setKeys((k) => ({ ...k, [c.route]: e.target.value }))} />
                  <button disabled={!!saving || !(keys[c.route] ?? "").trim()} onClick={() => act(c.route, "save_key")}>{saving === c.route ? "Checking the key" : "Check and save"}</button>
                  {ready && <button className="ghost" disabled={!!saving} onClick={() => ask({ title: `Remove the ${c.short} key?`, line: `SproutCrew forgets it on this Mac. It still works at ${c.site} until you revoke it there.`, action: "Remove", run: () => act(c.route, "remove_key") })}>Remove</button>}
                </div>
                {ready && <p className="muted">Removing it here does not cancel it. Revoke it at the provider too if you are done with it.</p>}
                {err[c.route] && <p className="field-error" role="alert">{err[c.route]}</p>}
              </div>
            </li>
          );
        })}
      </ul>
      {asking}
      <OwnService routes={routes} onRoutes={(r) => { setRoutes(r); window.dispatchEvent(new Event(KEYS_CHANGED)); }} />
      <p className="notice">ChatGPT and Claude subscriptions are not connected by this beta. You approve execution plans before tasks run. AI-assisted planning also uses provider allowance. Paid API models can cost money; token limits are not a dollar spending guarantee. Check your provider’s billing controls.</p>
    </Section>
  );
}


/** Google Docs: sign in once in the browser, view-only. */
function GoogleDocs() {
  const [st, setSt] = useState<{ available: boolean; connected: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => { api.call<{ available: boolean; connected: boolean }>("google_docs_status").then(setSt).catch(() => setSt(null)); }, []);
  const run = async (cmd: "connect_google_docs" | "disconnect_google_docs") => {
    setBusy(true); setErr("");
    try { setSt(await api.call<{ available: boolean; connected: boolean }>(cmd)); } catch (e) { setErr(text(e)); } finally { setBusy(false); }
  };
  return <Section title="Google Docs" value={st && !st.available ? "not available yet" : st?.connected ? "connected" : "not connected"}>
    <p className="muted">Read a Google Doc into a project's sources. SproutCrew can only view documents, never edit them.</p>
    {st && !st.available ? <p className="muted">Not available in this version yet.</p> : <>
      <p className="muted">{st?.connected ? "Your Google sign-in is kept in this Mac’s Keychain. Disconnecting forgets it here; you can also remove SproutCrew’s access in your Google account." : "Connect opens Google’s sign-in page in your browser. Choose your account and allow viewing your documents, then come back here. It waits up to three minutes."}</p>
      <div className="install-line">
        {st?.connected
          ? <button className="ghost" disabled={busy} onClick={() => void run("disconnect_google_docs")}>Disconnect</button>
          : <button disabled={busy || !st} onClick={() => void run("connect_google_docs")}>{busy ? "Waiting for Google…" : "Connect Google Docs"}</button>}
      </div>
    </>}
    {err && <p className="field-error" role="alert">{err}</p>}
  </Section>;
}

/** Off until the person turns it on: app building and clickable prototypes write code that can run. */
function RunningCode() {
  const [on, setOn] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => { api.call<boolean>("running_code_allowed").then(setOn).catch(() => setOn(false)); }, []);
  const change = async (next: boolean) => {
    setBusy(true); setErr("");
    try { setOn(await api.call<boolean>("set_running_code", { on: next })); } catch (e) { setErr(text(e)); } finally { setBusy(false); }
  };
  return <Section title="Projects that write running code" value={on ? "on" : "off"}>
    <p className="muted">App building, and a clickable prototype at the end of a brainstorm or proposal. Off unless you turn it on.</p>
    <div className="toggle-row"><LiquidSwitch checked={on} disabled={busy} label="Allow projects that write running code" onChange={(v) => void change(v)} /><span>Allow projects that write running code</span></div>
    <p className="muted">When this is on, New project offers App development, and brainstorms and proposals can end with a few clickable screens. Code is shown to you for review before any later step uses it. Prototypes open only in SproutCrew’s own preview, which loads nothing from the internet. SproutCrew never runs commands on this Mac, and the choices under While it works still apply.</p>
    {err && <p className="field-error" role="alert">{err}</p>}
  </Section>;
}

function Notifications() {
  const [status, setStatus] = useState<AttentionStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [note, setNote] = useState("");
  useEffect(() => { api.call<AttentionStatus>("attention_status").then(setStatus).catch(() => setError("Notification settings could not be read.")); }, []);
  const change = async (on: boolean) => {
    setBusy(true); setError(""); setNote("");
    try { setStatus(await api.call<AttentionStatus>("set_notifications", { on })); }
    catch (e) { setError(text(e)); } finally { setBusy(false); }
  };
  return <Section title="Notifications" value={!status ? "reading" : status.enabled ? `on · project names ${status.names ? "shown" : "hidden"}` : "off"}>
    <p className="muted">The Board keeps approvals, action requests, findings and stopped steps together. Its count updates while the app is open.</p>
    <div className="toggle-row"><LiquidSwitch checked={status?.enabled ?? false} disabled={busy || !status || api.mocked} label="Notify me when input is needed" onChange={on => void change(on)} /><span>Notify me when input is needed</span></div>
    <p className="muted">Mac alerts are optional and off until you enable them. New items are grouped while SproutCrew is in the background, and you are also told when a run finishes, stops, or waits for a provider's rate limit. Document text and keys never appear in these alerts, and project names only if you allow it below. Clicking one opens SproutCrew; it never approves work.</p>
    <div className="toggle-row"><LiquidSwitch checked={status?.names ?? false} disabled={busy || !status?.enabled || api.mocked} label="Name the project in run alerts" onChange={(on) => void (async () => { setBusy(true); try { setStatus(await api.call<AttentionStatus>("set_notification_names", { on })); } catch (e) { setError(text(e)); } finally { setBusy(false); } })()} /><span>Name the project in run alerts</span></div>
    <p className="muted">Keep SproutCrew open or minimized. No alerts are sent after you quit. macOS notification permissions and Focus control delivery, so a requested alert may be silenced. If a test does not appear, check System Settings → Notifications → SproutCrew.</p>
    <button className="ghost" disabled={busy || !status?.enabled || api.mocked} onClick={() => { setBusy(true); setError(""); setNote(""); void api.call("test_notification").then(() => setNote("Test requested. Check for the SproutCrew notification; macOS controls whether it appears.")).catch(e => setError(text(e))).finally(() => setBusy(false)); }}>Send a test notification</button>
    {api.mocked && <p className="notice">Browser preview. Test Mac alerts in the native app.</p>}
    {(error || status?.last_error) && <p className="field-error" role="alert">{error || status?.last_error}</p>}
    {note && <p role="status">{note}</p>}
  </Section>;
}

const MODES: { id: Theme; label: string }[] = [{ id: "system", label: "Match this Mac" }, { id: "light", label: "Light" }, { id: "dark", label: "Dark" }];

function Appearance() {
  const [scheme, setScheme] = useState<SchemeId>(loadScheme());
  const [theme, setTheme] = useState<Theme>(loadTheme());
  return (
    <Section title="Appearance" value={`${SCHEMES.find((c) => c.id === scheme)?.name ?? "Electric"}, ${MODES.find((m) => m.id === theme)?.label.toLowerCase() ?? "match this Mac"}`} open>
      <p className="muted">Choose light or dark, and one of five color schemes. The app icon and the start screen keep the SproutCrew colors.</p>
      <h3 id="mode-label">Mode</h3>
      <div className="seg" role="radiogroup" aria-labelledby="mode-label" onKeyDown={radioKeys}>
        {MODES.map((m) => (
          <button key={m.id} role="radio" aria-checked={theme === m.id} tabIndex={theme === m.id ? 0 : -1} onClick={() => { applyTheme(m.id); setTheme(m.id); }}>{m.label}</button>
        ))}
      </div>
      <h3 id="scheme-label">Scheme</h3>
      <div className="schemes" role="radiogroup" aria-labelledby="scheme-label" onKeyDown={radioKeys}>
        {SCHEMES.map((c) => (
          <button key={c.id} role="radio" aria-checked={scheme === c.id} tabIndex={scheme === c.id ? 0 : -1} className="scheme" onClick={() => { applyScheme(c.id); setScheme(c.id); }}>
            <span className="scheme-swatches" aria-hidden="true">{c.swatches.map((hex, i) => <span key={i} style={{ background: hex }} />)}</span>
            <strong>{c.name}{c.id === DEFAULT_SCHEME && <span className="muted"> default</span>}</strong>
            <span className="muted">{c.blurb}</span>
          </button>
        ))}
      </div>
    </Section>
  );
}

type UpdateInfo = { available: boolean; set_up: boolean; version?: string };

function Updates() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [state, setState] = useState<"" | "checking" | "installing">("");
  const [err, setErr] = useState("");
  const check = () => { setState("checking"); api.call<UpdateInfo>("check_update").then((i) => { setInfo(i); setErr(""); }).catch((e) => setErr(text(e))).finally(() => setState("")); };
  useEffect(check, []);
  const install = () => { setState("installing"); api.call("install_update").catch((e) => { setErr(text(e)); setState(""); }); };
  return (
    <div className="update-line">
      {info && !info.set_up && <span className="muted">This build does not update itself. Download new versions from sproutcrew.app.</span>}
      {info?.set_up && !info.available && <span className="muted">SproutCrew is up to date.</span>}
      {info?.available && <span>Version {info.version} is ready.</span>}
      {info?.available
        ? <button className="primary" disabled={!!state} onClick={install}>{state === "installing" ? "Installing" : "Install and restart"}</button>
        : info?.set_up && <button className="ghost" disabled={!!state} onClick={check}>{state === "checking" ? "Checking" : "Check for updates"}</button>}
      {err && <p className="field-error" role="alert">{err}</p>}
    </div>
  );
}

export default function Settings({ busy, guard, presenting, setPresenting, onBack }: { busy: string; guard: (f: () => Promise<unknown>) => Promise<void>; presenting: boolean; setPresenting: (b: boolean) => void; onBack: () => void }) {
  const [built, setBuilt] = useState<number | null>(null);
  const [version, setVersion] = useState("");
  useEffect(() => { api.call<Info>("app_info").then((i) => { setBuilt(i.built_at ?? null); setVersion(i.version ?? ""); }).catch(() => undefined); }, []);
  return (
    <div className="page-body settings-page">
      <button className="link" onClick={onBack}>Back</button>
      <div className="page-sections"><div>
      <Appearance />
      <Routes />
      <Preflight busy={busy} guard={guard} />
      <Notifications />
      <RunningCode />
      <GoogleDocs />
      <Section title="Privacy while recording" value={presenting ? "on · personal details hidden" : "off"}>
        <p className="muted">Hide your email address, home folder, your name and text that looks like a secret key from the app display. Stored data stays unchanged.</p>
        <div className="toggle-row">
          <LiquidSwitch checked={presenting} label="Hide personal details while I record" onChange={(on) => guard(async () => { await api.call("set_presentation", { on }); setPresenting(on); })} />
          <span>Hide personal details while I record</span>
        </div>
      </Section>
      <Section title="About SproutCrew" value={version || "this Mac"}>
          <p className="muted">SproutCrew is free and runs on this Mac. Project files and results are stored on this Mac. The selected API service receives the context needed for each request; OpenRouter also sends it to its inference provider.</p>
          <p>{pitch}</p>
          <p className="muted">API keys only in this build. Source fencing reduces steering risk; it does not guarantee model immunity. Token-only limits do not cap provider bills.</p>
          <Updates />
          <div className="actions about-links">
            {([["privacy", "Privacy"], ["terms", "Terms"], ["home", "Website"], ["contact", "Email hello@sproutcrew.app"], ["licenses", "Open-source licenses"]] as const).map(([id, label]) => (
              <button key={id} className="ghost" onClick={() => void api.call("open_link", { link: id }).catch(() => undefined)}>{label}</button>
            ))}
          </div>
          <BugLog />
          <p className="muted">Word files use docx-rs and PowerPoint decks use PptxGenJS. PDFs use the bundled Typst helper. Conversion runs locally; the licenses and notices are in Open-source licenses.</p>
      </Section>
      </div></div>
      <p className="muted about">SproutCrew{version ? ` ${version}` : ""}, built by Nitish Prasad.{built ? ` This copy was built on ${new Date(built * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}.` : ""}</p>
    </div>
  );
}

/** Saves the local log as a file to attach to a bug report. Nothing is sent; the person chooses where it goes. */
function BugLog() {
  const [msg, setMsg] = useState("");
  return (
    <div className="bug-log">
      <button className="ghost" onClick={() => api.call<string>("export_log").then(() => setMsg("Saved and shown in Finder. Attach it to your email.")).catch((e) => setMsg(text(e)))}>Export a log for a bug report</button>
      <p className="muted">The log holds app versions, which services are connected, and when steps stopped and why in one word. It never holds your briefs, results, file names or keys, and nothing is sent.</p>
      {msg && <p role="status">{msg}</p>}
    </div>
  );
}
