// One small spring (docs/DESIGN.md §7, §12). It integrates the damped-spring equation, can be retargeted while
// it moves without losing its velocity, and jumps straight to the end when the person asked for reduced motion.
// Durations come from the spring, not from a timer.
import { useEffect, useRef, useState } from "react";

export type SpringOpts = { zeta: number; omega: number };

/** Standard spring for state changes: settles in about a third of a second with a barely visible overshoot. */
export const STANDARD: SpringOpts = { zeta: 0.8, omega: (2 * Math.PI) / 0.35 };
/** Playful spring, for one featured moment per view: a visible overshoot. */
export const PLAYFUL: SpringOpts = { zeta: 0.55, omega: (2 * Math.PI) / 0.45 };
/** Rubber-band resistance past a limit, as on iOS. */
export const RUBBER = 0.55;

export type State = { x: number; v: number };

const SUBSTEP = 1 / 240;

/** Advances a spring toward `target` by `dt` seconds. Pure, so it can be tested. */
export function step(s: State, target: number, dt: number, o: SpringOpts): State {
  let { x, v } = s;
  let left = dt;
  while (left > 0) {
    const h = Math.min(SUBSTEP, left);
    const a = -o.omega * o.omega * (x - target) - 2 * o.zeta * o.omega * v;
    v += a * h;
    x += v * h;
    left -= h;
  }
  return { x, v };
}

export function settled(s: State, target: number, eps = 0.001): boolean {
  return Math.abs(s.x - target) < eps && Math.abs(s.v) < eps;
}

/** Past `limit` the dragged distance is compressed, so the further you pull the less it moves. */
export function rubberBand(distance: number, limit: number, c = RUBBER): number {
  if (distance <= limit) return distance;
  const over = distance - limit;
  return limit + (1 - 1 / ((over * c) / Math.max(limit, 1) + 1)) * Math.max(limit, 1);
}

/** Stretch along the travel axis from speed, capped, with the other axis shrunk so volume is kept. */
export function stretch(velocity: number, k = 0.012, cap = 0.3): { sx: number; sy: number } {
  const sx = 1 + Math.min(Math.abs(velocity) * k, cap);
  return { sx, sy: 1 / sx };
}

export const reducedMotion = () =>
  typeof window !== "undefined" && !!window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** A value that follows `target` on a spring. A new target retargets it from its current speed. */
export function useSpring(target: number, o: SpringOpts = STANDARD): { value: number; velocity: number } {
  const [out, setOut] = useState({ value: target, velocity: 0 });
  const st = useRef<State>({ x: target, v: 0 });
  const goal = useRef(target);
  const raf = useRef(0);
  useEffect(() => {
    goal.current = target;
    if (reducedMotion()) {
      st.current = { x: target, v: 0 };
      setOut({ value: target, velocity: 0 });
      return;
    }
    cancelAnimationFrame(raf.current);
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min((now - last) / 1000, 0.05);
      last = now;
      st.current = step(st.current, goal.current, dt, o);
      if (settled(st.current, goal.current)) {
        st.current = { x: goal.current, v: 0 };
        setOut({ value: goal.current, velocity: 0 });
        return;
      }
      setOut({ value: st.current.x, velocity: st.current.v });
      raf.current = requestAnimationFrame(tick);
    };
    raf.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf.current);
  }, [target, o.zeta, o.omega]);
  return out;
}

/** A scale for a mark that finishes while it is on screen: from small to full size on the playful spring, once.
 *  A mark that is already finished when it first shows is simply full size. */
export function usePop(on: boolean): number {
  const first = useRef(true);
  const [t, setT] = useState(on ? 1 : 0);
  const k = useSpring(t, PLAYFUL).value;
  useEffect(() => {
    if (first.current) { first.current = false; return; }
    setT(on ? 1 : 0);
  }, [on]);
  return on ? Math.max(k, 0.01) : 1;
}
