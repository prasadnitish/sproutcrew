import { useEffect, useRef, useState } from "react";
import { PLAYFUL, reducedMotion, settled, step, stretch } from "./spring";

const TRAVEL = 18;

/** The one featured custom switch (docs/DESIGN.md §6, §7): the knob travels on a damped spring, stretches along
 *  its travel with speed while keeping its volume, and squishes softly at each end. It is a real switch for the
 *  keyboard and for assistive tools, and with Reduce Motion it just moves. */
export default function LiquidSwitch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (on: boolean) => void; label: string; disabled?: boolean }) {
  const [pos, setPos] = useState({ x: checked ? TRAVEL : 0, v: 0 });
  const st = useRef({ x: checked ? TRAVEL : 0, v: 0 });
  const goal = useRef(checked ? TRAVEL : 0);
  const raf = useRef(0);
  useEffect(() => {
    goal.current = checked ? TRAVEL : 0;
    if (reducedMotion()) { st.current = { x: goal.current, v: 0 }; setPos({ x: goal.current, v: 0 }); return; }
    cancelAnimationFrame(raf.current);
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min((now - last) / 1000, 0.05);
      last = now;
      st.current = step(st.current, goal.current, dt, PLAYFUL);
      if (settled(st.current, goal.current, 0.01)) { st.current = { x: goal.current, v: 0 }; setPos({ x: goal.current, v: 0 }); return; }
      setPos({ x: st.current.x, v: st.current.v });
      raf.current = requestAnimationFrame(tick);
    };
    raf.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf.current);
  }, [checked]);
  // Stretch with speed. At the ends the knob is pressed a little against the track.
  const s = stretch(pos.v, 0.006, 0.3);
  const edge = Math.max(0, -pos.x, pos.x - TRAVEL);
  const squish = 1 - Math.min(edge * 0.03, 0.2);
  const sx = s.sx * squish, sy = s.sy / squish;
  return (
    <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} className={`liquid${checked ? " on" : ""}`} onClick={() => onChange(!checked)}>
      <span className="liquid-knob" style={{ transform: `translateX(${pos.x}px) scale(${sx}, ${sy})` }} />
    </button>
  );
}
