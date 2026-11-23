import { api, RunEvent } from "./api";
import { Ended, runSteps } from "./runLoop";
import { RUN_GOING } from "./status";

/** Who is following the run: a project page shows its events and reads the project again after each step; the app
 *  shell names the running project in every page bar. */
type Listener = { event?: (e: RunEvent) => void; change?: (project: string | null) => void; wave?: () => Promise<void> | void };

/** Set by Stop and read between steps. It lives outside every page, so Stop reaches a run from any screen. */
export const stopFlag = { current: false };

let project: string | null = null;
let last: RunEvent | null = null;
const listeners = new Set<Listener>();

const emit = (e: RunEvent) => {
  last = e.event === "finished" ? null : e.event === "started" ? e : last;
  listeners.forEach((l) => l.event?.(e));
};

export const runner = {
  /** The project whose steps are running now, or null. */
  get project() { return project; },
  /** The step that started last and has not finished, for a page that opens partway through a run. */
  get started() { return last; },
  subscribe(l: Listener): () => void {
    listeners.add(l);
    return () => { listeners.delete(l); };
  },
  /** Runs the open project's ready steps until none is left, the person stops, or answers are needed. `name` is the
   *  open project; the app's hold keeps any other from being opened until the run ends. */
  async start(name: string, engine: string): Promise<Ended> {
    // A second start while one goes is refused here, before it can touch the first run's state.
    if (project) throw RUN_GOING;
    project = name;
    last = null;
    listeners.forEach((l) => l.change?.(name));
    try {
      return await runSteps({
        call: (cmd, args) => api.call(cmd, args),
        run: (cmd, args) => api.run(cmd, args, emit),
        afterWave: async () => { for (const l of [...listeners]) await l.wave?.(); },
        stopped: () => stopFlag.current,
        resetStop: () => { stopFlag.current = false; },
      }, engine);
    } finally {
      project = null;
      last = null;
      listeners.forEach((l) => l.change?.(null));
    }
  },
};
