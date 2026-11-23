const MAKERS: Record<string, string> = { mistralai: "Mistral", anthropic: "Anthropic", openai: "OpenAI", google: "Google", deepseek: "DeepSeek", "meta-llama": "Meta" };

/** The company that makes a model: "Mistral" for "mistralai/mistral-small-2603". A model on a direct route has no
 *  prefix, so its family says who made it. An id it cannot place gives an empty string. */
export function makerName(modelId: string): string {
  const slash = modelId.indexOf("/");
  if (slash > 0) {
    const prefix = modelId.slice(0, slash);
    return MAKERS[prefix] ?? prefix.charAt(0).toUpperCase() + prefix.slice(1);
  }
  const m = modelId.toLowerCase();
  if (m.startsWith("claude")) return "Anthropic";
  if (/^(gpt|o\d)/.test(m)) return "OpenAI";
  if (m.startsWith("gemini")) return "Google";
  return "";
}

/** The one name for the second check, everywhere it appears: "Second check by Mistral, a different company". It is
 *  the same company when any of the models that wrote is made by the second check's maker. Empty when nothing checks. */
export function secondCheckBy(checkers: string[], writers: string[]): string {
  if (checkers.length === 0) return "";
  const made = checkers.map(makerName);
  const makers = [...new Set(made.filter(Boolean))];
  if (makers.length === 0 || made.some((m) => !m)) return makers.length === 0 ? "Second check by another AI" : `Second check by ${makers.join(" and ")}`;
  const writing = new Set(writers.map(makerName));
  const same = makers.filter((m) => writing.has(m));
  const by = `Second check by ${makers.join(" and ")}`;
  return same.length === 0 ? `${by}, a different company` : same.length === makers.length ? `${by}, the same company` : by;
}
