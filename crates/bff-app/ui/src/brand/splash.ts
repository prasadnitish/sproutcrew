/** The final pose settles by 1.55s, with a readable hold within the existing startup window. */
export const SPLASH_DURATION_MS = 2600;
export const REDUCED_SPLASH_DURATION_MS = 350;
export const isSplashSkipKey = (key: string) => ['Escape', 'Enter', ' '].includes(key);
