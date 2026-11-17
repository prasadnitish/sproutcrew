/** The color schemes a person can choose. Each sets the pair of muted tints that tell the writers from the checkers
 *  (docs/DESIGN.md §9) under `[data-palette]` in tokens.css, in light and in dark. Buttons, links, and status colors
 *  do not change. `swatches` are the light values: writing, checking, and the writing tint's fill. */
export const PALETTES = [
  { id: "classic", name: "SproutCrew", note: "Ink blue for the writers and warm apricot tints for the reviewers. The default.", swatches: ["#36558b", "#83542b", "#e7ecf6"] },
  { id: "graphite", name: "Graphite", note: "Quiet and neutral. Graphite for the writers, bronze for the checkers.", swatches: ["#46515f", "#6c5a3c", "#e0e3e8"] },
  { id: "harbor", name: "Harbor", note: "Muted teal for the writers and muted plum for the checkers.", swatches: ["#2f6468", "#7a4f86", "#dcebec"] },
  { id: "meadow", name: "Meadow", note: "Muted green for the writers and muted rose for the checkers.", swatches: ["#46663f", "#86485b", "#e2ecdd"] },
  { id: "signal", name: "Signal", note: "Steel blue for the writers and rust for the checkers. The two sides are furthest apart.", swatches: ["#3a5688", "#8a4a2a", "#dfe6f3"] },
] as const;

export type PaletteId = (typeof PALETTES)[number]["id"];

const KEY = "bff-palette";

/** A stored value as a palette id: the value if it names a palette, else the first. */
export function paletteFromStored(v: string | null): PaletteId {
  return PALETTES.find((p) => p.id === v)?.id ?? PALETTES[0].id;
}

/** Browser storage can be missing or refuse, so both ends are guarded and the app works without it. */
export function loadPalette(): PaletteId {
  try {
    return paletteFromStored(localStorage.getItem(KEY));
  } catch {
    return PALETTES[0].id;
  }
}

export function applyPalette(id: PaletteId, save = true) {
  document.documentElement.dataset.palette = id;
  if (!save) return;
  try {
    localStorage.setItem(KEY, id);
  } catch {
    /* not kept, still applied */
  }
}
