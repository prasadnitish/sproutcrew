import { useEffect } from "react";
import BrandMark, { LeafTone } from "./BrandMark";
import { WindowChrome } from "./ui";
import { isSplashSkipKey, REDUCED_SPLASH_DURATION_MS, SPLASH_DURATION_MS } from "./brand/splash";

export function SplashIdentity({ reducedMotion = false, leafTone = "apricot" }: { reducedMotion?: boolean; leafTone?: LeafTone }) {
  return <div className={`splash-identity${reducedMotion ? " motion-reduced" : ""}`}>
    <div className="splash-symbol"><span className="splash-seed" aria-hidden="true" /><BrandMark leafTone={leafTone} /></div>
    <h1 className="splash-wordmark">SproutCrew</h1>
    <p className="splash-tagline">Your AI team for everyday work.</p>
    <p className="splash-credit">Built by Nitish Prasad.</p>
  </div>;
}

// Brand introduction, with a visible skip action and a short static reduced-motion path.
export default function Splash({ onDone }: { onDone: () => void }) {
  useEffect(() => {
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    let t: ReturnType<typeof setTimeout>;
    const schedule = () => {
      clearTimeout(t);
      t = setTimeout(onDone, preference.matches ? REDUCED_SPLASH_DURATION_MS : SPLASH_DURATION_MS);
    };
    const done = (event: KeyboardEvent) => {
      if (isSplashSkipKey(event.key)) { event.preventDefault(); onDone(); }
    };
    schedule();
    preference.addEventListener("change", schedule);
    window.addEventListener("keydown", done);
    return () => {
      clearTimeout(t);
      preference.removeEventListener("change", schedule);
      window.removeEventListener("keydown", done);
    };
  }, [onDone]);
  return (
    <main className="splash" aria-label="Welcome to SproutCrew">
      <WindowChrome />
      <SplashIdentity />
      <button className="ghost splash-skip" onClick={onDone} autoFocus>Skip introduction <span aria-hidden="true">↗</span></button>
    </main>
  );
}
