import { useEffect, useRef, useState } from "react";
import { api } from "./api";

// A synthetic high-rate transcript for the M1 WKWebView check. It appends many lines per second, keeps
// only the newest window in the DOM, and records the longest gap between animation frames. It also
// counts frames while the window is hidden, which shows how hidden-window throttling behaves.
const WINDOW = 300;

export default function Stress() {
  const [lines, setLines] = useState<string[]>([]);
  const [report, setReport] = useState<string>("running");
  const total = useRef(0);

  useEffect(() => {
    const perTick = 50; // 50 lines every 10 ms is 5000 lines per second
    const buf: string[] = [];
    let maxGap = 0;
    let last = performance.now();
    let frames = 0;
    let hiddenFrames = 0;
    let hiddenMs = 0;
    let hiddenSince = 0;
    const onVis = () => {
      if (document.hidden) hiddenSince = performance.now();
      else hiddenMs += performance.now() - hiddenSince;
    };
    document.addEventListener("visibilitychange", onVis);
    const feed = setInterval(() => {
      for (let i = 0; i < perTick; i++) {
        total.current++;
        buf.push(`event ${total.current} item/agentMessage/delta tokens=${(total.current * 7) % 997}`);
      }
      if (buf.length > WINDOW) buf.splice(0, buf.length - WINDOW);
    }, 10);
    let raf = 0;
    const frame = (t: number) => {
      frames++;
      if (document.hidden) hiddenFrames++;
      if (frames > 5) maxGap = Math.max(maxGap, t - last);
      last = t;
      setLines(buf.slice());
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    const stop = setTimeout(() => {
      clearInterval(feed);
      cancelAnimationFrame(raf);
      document.removeEventListener("visibilitychange", onVis);
      const r = { lines_produced: total.current, dom_lines: WINDOW, frames, longest_frame_gap_ms: Math.round(maxGap), frames_while_hidden: hiddenFrames, hidden_ms: Math.round(hiddenMs) };
      (window as unknown as { __stress: unknown }).__stress = r;
      setReport(JSON.stringify(r));
      api.call("write_stress_report", { report: { ...r, user_agent: navigator.userAgent, visibility_at_end: document.visibilityState } }).catch(() => undefined);
    }, 8000);
    return () => {
      clearInterval(feed);
      clearTimeout(stop);
      cancelAnimationFrame(raf);
    };
  }, []);

  return (
    <section className="stress">
      <h2>Transcript stress</h2>
      <p className="muted">5000 lines per second for 8 seconds, newest {WINDOW} in the page.</p>
      <pre id="stress-report">{report}</pre>
      <pre className="stress-log">{lines.slice(-12).join("\n")}</pre>
    </section>
  );
}
