/** Keep the original order and objects so search never changes project shortcuts. */
export function filterProjects<T extends { name: string; brief: string }>(projects: T[], query: string): T[] {
  const words = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return projects.filter((project) => {
    const searchable = `${project.name} ${project.brief}`.toLocaleLowerCase();
    return words.every((word) => searchable.includes(word));
  });
}

export function restartName(original: string, existing: string[], word = 'restart'): string {
  const names = new Set(existing.map((name) => name.toLowerCase()));
  for (let attempt = 1; ; attempt += 1) {
    const suffix = attempt === 1 ? `-${word}` : `-${word}-${attempt}`;
    const name = `${original.slice(0, 64 - suffix.length)}${suffix}`;
    if (!names.has(name.toLowerCase())) return name;
  }
}

/** A project name from the outcome and the first words of the brief, in the characters a name allows. */
export function suggestName(kind: string, brief: string): string {
  const words = brief.toLowerCase().replace(/[^a-z0-9\s-]/g, " ").split(/\s+/).filter((w) => w.length > 2 && !STOP.has(w)).slice(0, 4);
  const base = (words.length ? words.join("-") : kind.replace(/_/g, "-")).slice(0, 48).replace(/-+$/, "");
  return base || "project";
}
const STOP = new Set(["the", "and", "for", "our", "about", "with", "that", "this", "from", "what", "should", "write", "make", "use", "into", "are", "how", "did", "does", "can", "could", "would", "will", "why", "who", "when", "where", "which", "was", "were", "has", "have", "its", "they", "their", "please", "help", "need", "want"]);

/** When something last happened, short and plain: "Just now", "12 min ago", "Today 6:54 PM", "Yesterday", "Nov 3". */
export function lastActivity(atSecs: number | null | undefined, now = Date.now()): string {
  if (!atSecs) return "";
  const at = new Date(atSecs * 1000);
  const mins = Math.floor((now - at.getTime()) / 60000);
  if (mins < 1) return "Just now";
  if (mins < 60) return `${mins} min ago`;
  const today = new Date(now);
  const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();
  if (sameDay(at, today)) return `Today ${at.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`;
  const yesterday = new Date(now - 86400000);
  if (sameDay(at, yesterday)) return "Yesterday";
  return at.toLocaleDateString("en-US", { month: "short", day: "numeric", ...(at.getFullYear() !== today.getFullYear() ? { year: "numeric" } : {}) });
}
