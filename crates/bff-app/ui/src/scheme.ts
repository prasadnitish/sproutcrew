/** The color schemes in Settings, Appearance (DESIGN_SPEC.md §3). Each re-maps every color role in light and dark under
 *  `[data-scheme]` in tokens.css; Electric is plain `:root`. `swatches` are light base, ink, accent, warn, danger, live. */
export const SCHEMES = [
  { id: "electric", name: "Electric", blurb: "Sharp. One electric blue on plain white, vivid status.", swatches: ["#FFFFFF", "#080C18", "#1B4DFF", "#E88A00", "#E3173E", "#00A35E"] },
  { id: "mono", name: "Mono", blurb: "Black and white. Only a status has color, and it is faded.", swatches: ["#FFFFFF", "#0A0A0A", "#0A0A0A", "#A8832B", "#B4534C", "#55806A"] },
  { id: "pastel", name: "Pastel", blurb: "Soft sky, butter, blush and mint under crisp dark text.", swatches: ["#FFFFFF", "#1B2438", "#B7D4FF", "#E9C04A", "#F29CAA", "#7ACFA3"] },
  { id: "pop", name: "Pop", blurb: "Bright. Hot pink action on white, with black type.", swatches: ["#FFFFFF", "#120A12", "#D6086F", "#F09A00", "#D43A00", "#00A07A"] },
  { id: "lagoon", name: "Lagoon", blurb: "Fresh. Deep teal and clear water, cool neutrals.", swatches: ["#FFFFFF", "#06191C", "#00788A", "#E39A12", "#E0434F", "#3FA34D"] },
] as const;

export type SchemeId = (typeof SCHEMES)[number]["id"];
export const DEFAULT_SCHEME: SchemeId = "electric";

const KEY = "sproutcrew-scheme";
/** Version 2.0 kept a crew palette here. It is no longer read, and is removed when the scheme loads. */
const OLD_KEY = "bff-palette";

/** A stored value as a scheme: the value if it names one, else Electric. */
export function schemeFromStored(v: string | null): SchemeId {
  return SCHEMES.find((s) => s.id === v)?.id ?? DEFAULT_SCHEME;
}

/** Browser storage can be missing or refuse, so both ends are guarded and the app works without it. */
export function loadScheme(): SchemeId {
  try {
    localStorage.removeItem(OLD_KEY);
    return schemeFromStored(localStorage.getItem(KEY));
  } catch {
    return DEFAULT_SCHEME;
  }
}

/** Applies a scheme, and stores it only when it is not the default. */
export function applyScheme(id: SchemeId, save = true) {
  document.documentElement.dataset.scheme = id;
  if (!save) return;
  try {
    if (id === DEFAULT_SCHEME) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, id);
  } catch {
    /* not kept, still applied */
  }
}
