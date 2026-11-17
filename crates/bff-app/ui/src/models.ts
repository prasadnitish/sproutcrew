/** OpenRouter models as the picker shows them. Prices are dollars per million tokens; null is unknown. */
export type CatalogModel = {
  id: string;
  name: string;
  maker: string;
  context_length: number | null;
  input_per_mtok: number | null;
  output_per_mtok: number | null;
  free: boolean;
  sees_images: boolean;
};

export type Catalog = {
  models: CatalogModel[];
  fetched_at: number | null;
  offline: boolean;
  recommended: { writing: string[]; checking: string[] };
};

/** One price, or "unknown". Never shows an unknown price as $0. */
export function money(v: number | null): string {
  if (v === null || !Number.isFinite(v) || v < 0) return "unknown";
  if (v === 0) return "$0";
  return v < 0.1 ? `$${v.toFixed(3).replace(/0+$/, "")}` : `$${v.toFixed(2)}`;
}

/** "$0.10 in · $0.50 out per million tokens", or "Free", or "Price unknown". */
export function priceLine(m: CatalogModel): string {
  if (m.free) return "Free";
  if (m.input_per_mtok === null && m.output_per_mtok === null) return "Price unknown";
  return `${money(m.input_per_mtok)} in · ${money(m.output_per_mtok)} out per million tokens`;
}

export function contextLine(n: number | null): string {
  if (!n) return "";
  return n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(1)}M context` : `${Math.round(n / 1000)}K context`;
}

/** Search by name or ID, every word must match; then the maker filter. Order is kept. */
export function filterModels(models: CatalogModel[], query: string, maker: string): CatalogModel[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return models.filter((m) => (!maker || m.maker === maker) && words.every((w) => m.id.toLowerCase().includes(w) || m.name.toLowerCase().includes(w)));
}

export function makers(models: CatalogModel[]): string[] {
  return [...new Set(models.map((m) => m.maker))].sort();
}

/** The three groups in order. A model appears once: Recommended wins over Free, Free over All. */
export function groups(models: CatalogModel[], recommended: string[]): { title: string; models: CatalogModel[] }[] {
  const rec = recommended.map((id) => models.find((m) => m.id === id)).filter((m): m is CatalogModel => !!m);
  const free = models.filter((m) => m.free && !recommended.includes(m.id));
  const rest = models.filter((m) => !m.free && !recommended.includes(m.id));
  return [
    { title: "Recommended", models: rec },
    { title: "Free", models: free },
    { title: "All models", models: rest },
  ].filter((g) => g.models.length > 0);
}
