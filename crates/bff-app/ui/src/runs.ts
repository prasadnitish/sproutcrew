/** A step that ran but did not produce a result, such as one stopped or refused, must not look like it finished. */
export function failedRun(cmd: string, result: unknown): string {
  if (cmd === "run_wave" && Array.isArray(result)) {
    return result.filter((r) => r && r.ok === false).map((r) => `${r.stage}: ${r.note}`).join("\n");
  }
  return "";
}
