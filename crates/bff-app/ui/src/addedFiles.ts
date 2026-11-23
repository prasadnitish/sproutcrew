/** "12 KB" under a megabyte (never below 1 KB), "2.5 MB" above. */
export const sizeLabel = (bytes: number) => (bytes < 1024 * 1024 ? `${Math.max(1, Math.round(bytes / 1024))} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`);

/** What the app found out about a dropped or typed path. A folder has no size. */
export type PathInfo = { name: string; size: number | null; is_dir: boolean };

/** A path as the list shows it: its name and size, "folder" for a folder, or the path as given until the app has looked. */
export function pathLabel(path: string, info: PathInfo | undefined): string {
  if (!info) return path;
  return info.is_dir ? `${info.name} · folder` : info.size == null ? info.name : `${info.name} · ${sizeLabel(info.size)}`;
}

/** The line shown under "Your files": what has been added so far, or nothing when the list is empty. */
export function addedLine(items: { kind: "path" | "url" | "file" | "results"; folder?: boolean }[]): string {
  const count = (test: (i: (typeof items)[number]) => boolean) => items.filter(test).length;
  const parts = [
    [count((i) => (i.kind === "file" || i.kind === "path") && !i.folder), "file", "files"],
    [count((i) => i.kind === "path" && !!i.folder), "folder", "folders"],
    [count((i) => i.kind === "url"), "web address", "web addresses"],
    [count((i) => i.kind === "results"), "set of project results", "sets of project results"],
  ].flatMap(([n, one, many]) => (n ? [`${n} ${n === 1 ? one : many}`] : []));
  if (!parts.length) return "";
  return `${parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : parts[0]} added`;
}
