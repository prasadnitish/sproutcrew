import { useEffect, useState } from "react";
import { api, ApiRoute } from "./api";
import ModelPicker, { useModelName } from "./ModelPicker";
import { EFFORTS, PROVIDERS, text } from "./ui";

export type Choice = [route: string, model: string, effort: string];

/** Sent on window after a key is saved or removed, so every provider list reloads. */
export const KEYS_CHANGED = "sproutcrew-keys-changed";

/** New crews start on these OpenRouter models; Nitish chose them on Nov 10, 2026. */
export const DEFAULT_WRITING = "anthropic/claude-haiku-5.5";
export const DEFAULT_CHECKING = "mistralai/mistral-small-2603";

const PREFERRED: Record<string, string> = { R3: "claude-sonnet-5-5", R5: "gpt-6-luna", R12: DEFAULT_WRITING };

/** Providers a person can pick: the two logins, plus any API provider that is set up. */
export function useProviders(): [string, string][] {
  const [ready, setReady] = useState<string[]>([]);
  useEffect(() => {
    const load = () => api.call<ApiRoute[]>("api_routes").then((r) => setReady(r.filter((x) => x.state === "ready").map((x) => x.route))).catch(() => setReady([]));
    void load();
    // A key saved anywhere in the app shows up here without a reload.
    window.addEventListener(KEYS_CHANGED, load);
    return () => window.removeEventListener(KEYS_CHANGED, load);
  }, []);
  return PROVIDERS.filter(([id]) => ready.includes(id));
}

/** Provider, model, and effort for one role. A provider with no model chosen gets its usual one. */
export function ProviderPick({ value, onChange, label, side }: { value: Choice; onChange: (c: Choice) => void; label: string; side?: "writing" | "checking" }) {
  const providers = useProviders();
  const [picking, setPicking] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [why, setWhy] = useState("");
  const [route, model, eff] = value;
  useEffect(() => {
    let live = true;
    api.call<string[]>("route_models", { route }).then((m) => {
      if (!live) return;
      setModels(m);
      if (route !== "R12" && route !== "R13" && !m.includes(model) && m.length) onChange([route, m.includes(PREFERRED[route]) ? PREFERRED[route] : m[0], eff]);
      if (route === "R13" && !model && m.length) onChange([route, m[0], eff]);
    }).catch(() => live && setModels([]));
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [route]);
  useEffect(() => {
    if (!model) return;
    api.call("check_assignment", { route, model, effort: eff }).then(() => setWhy("")).catch((e) => setWhy(text(e)));
  }, [route, model, eff]);
  const name = useModelName(model);
  const which = side ?? (/check/i.test(label) ? "checking" : "writing");
  const all = providers.some(([id]) => id === route) ? providers : [...providers, [route, route] as [string, string]];
  return (
    <div className="pick">
      <label>{label ? `${label} provider` : "Provider"}
        <select value={route} onChange={(e) => onChange([e.target.value, "", eff])}>
          {all.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
        </select>
      </label>
      <label>Model
        {route === "R12" ? (
          <button type="button" className="model-button" onClick={() => setPicking(true)} aria-haspopup="dialog">
            <span>{model ? name : "Choose a model"}</span>{model && name !== model && <span className="meta mono">{model}</span>}
          </button>
        ) : route === "R13" ? (
          <input value={model} spellCheck={false} placeholder="Model name, such as llama3.3" onChange={(e) => onChange([route, e.target.value.trim(), eff])} />
        ) : (
          <select value={model} onChange={(e) => onChange([route, e.target.value, eff])}>
            {[...new Set([model, ...models].filter(Boolean))].map((m) => <option key={m}>{m}</option>)}
          </select>
        )}
      </label>
      <label>Effort
        <select value={eff} onChange={(e) => onChange([route, model, e.target.value])}>
          {EFFORTS.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
        </select>
      </label>
      {picking && <ModelPicker side={which} value={model} onClose={() => setPicking(false)} onPick={(id) => { setPicking(false); onChange([route, id, eff]); }} />}
      {why && <p className="field-error" role="alert">{why}</p>}
    </div>
  );
}
