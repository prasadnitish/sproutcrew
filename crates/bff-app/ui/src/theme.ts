export type Theme = 'system' | 'light' | 'dark';
const KEY = 'sproutcrew-theme';
export const validTheme = (value: string | null): Theme => value === 'light' || value === 'dark' ? value : 'system';
export function loadTheme(): Theme {
  try { return validTheme(localStorage.getItem(KEY)); } catch { return 'system'; }
}
export function applyTheme(theme: Theme, persist = true) {
  if (theme === 'system') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  if (persist) try { localStorage.setItem(KEY, theme); } catch { /* session-only preference */ }
}
