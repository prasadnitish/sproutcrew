/** The code the backend puts at the front of the error for a key in the brief or the name. */
export const KEY_IN_BRIEF = "key-in-brief:";
/** How long "Connected to OpenRouter" stays on screen. */
export const CONNECTED_MS = 4000;

export const isKeyInBrief = (error: string) => error.startsWith(KEY_IN_BRIEF);
/** The plain sentence, without the code. */
export const keyInBriefText = (error: string) => error.slice(KEY_IN_BRIEF.length).trim();

/** The same shapes the backend refuses (provider, GitHub, Slack, AWS and Google keys, long mixed tokens). */
function keyShaped(t: string, asName: boolean): boolean {
  const body = /^[A-Za-z0-9_-]+$/;
  if (t.startsWith("sk-ant-") && t.length >= 20) return true;
  if (t.startsWith("sk-") && t.length >= 32 && body.test(t.slice(3))) return true;
  if (["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"].some((p) => t.startsWith(p)) && t.length >= 30) return true;
  if (t.length >= 15 && /^xox[baprs]-/.test(t)) return true;
  if (/^AKIA[A-Z0-9]{16}$/.test(t)) return true;
  if (t.length >= 39 && t.startsWith("AIza") && body.test(t.slice(4))) return true;
  if (t.startsWith("eyJ") && t.length >= 30) {
    const parts = t.split(".");
    if (parts.length === 3 && parts.every(Boolean)) return true;
  }
  // A project name is only refused for a named shape above. A long name with a digit is not a key.
  if (asName) return false;
  const digest = [32, 40, 64, 128].includes(t.length) && /^[0-9a-fA-F]+$/.test(t);
  return t.length >= 32 && body.test(t) && !digest && !/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(t) && /\d/.test(t) && /[A-Za-z]/.test(t);
}

/** The backend's placeholder test, which it applies to the token after "Bearer". */
function placeholder(v: string): boolean {
  const l = v.toLowerCase();
  return /^[<${[%]/.test(v) || ["your", "example", "xxxx", "redacted", "changeme", "placeholder", "****", "..."].some((p) => l.includes(p)) || [...v].every((c) => c === v[0]);
}

const TRIM = /^[,;()<>"']+|[,;()<>"']+$/g;

/**
 * The first key-shaped piece of `text`, or null. Like the backend scan it also looks at the value after "=" and
 * at the token after "Bearer". It is only ever handed to `save_key`, never logged or kept. The lines in
 * crates/bff-core/tests/fixtures/key_shapes.json are checked here and in Rust so the two cannot drift. With
 * `asName` the text is a project name, which is refused only for a named key shape (not the long-mixed-token rule).
 */
export function findKeyShape(text: string, asName = false): { key: string; openrouter: boolean } | null {
  const words = text.split(/\s+/).filter(Boolean);
  for (const [i, word] of words.entries()) {
    const trimmed = word.replace(TRIM, "");
    const pieces = [trimmed];
    const eq = trimmed.indexOf("=");
    if (eq >= 0) pieces.push(trimmed.slice(eq + 1).replace(TRIM, ""));
    for (const t of pieces) {
      const afterBearer = i > 0 && words[i - 1] === "Bearer" && t.length >= 20 && !placeholder(t);
      if (t && (keyShaped(t, asName) || afterBearer)) return { key: t, openrouter: t.startsWith("sk-or-") };
    }
  }
  return null;
}

/** `text` with the first occurrence of `key` taken out, and the gap closed. */
export function withoutKey(text: string, key: string): string {
  return text.replace(key, "").replace(/[ \t]{2,}/g, " ").replace(/^[ \t]+|[ \t]+$/gm, "").trim();
}
