/** The run of a plan's steps (DESIGN_SPEC.md §14, the run loop out of `WorkTab`). It is plain code with its calls
 *  passed in, so any screen can start it and the tests can drive it. Only one run goes at a time: the app's hold
 *  refuses a second `begin_run`, and that refusal is passed on without touching the first run's hold. */
export type Ended = "finished" | "stopped" | "failed" | "questions";

export type RunIO = {
  call: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  run: (cmd: string, args: Record<string, unknown>) => Promise<unknown>;
  /** After each step or group of steps, so the screen shows each finished step before the next starts. */
  afterWave: () => Promise<void>;
  /** Whether the person pressed Stop. */
  stopped: () => boolean;
  /** Clears a Stop left from an earlier run, once this run holds the project. */
  resetStop: () => void;
};

/** Runs what is ready, then asks again, up to three steps at a time. A step is tried once per run, so one that fails
 *  is not tried again in a loop. Throws what a failed step threw, after letting go of the hold. */
export async function runSteps(io: RunIO, engine: string): Promise<Ended> {
  let ended: Ended = "finished";
  let began = false;
  try {
    // The whole run holds the project, so another one stays closed in the moment between two steps too.
    await io.call("begin_run");
    began = true;
    io.resetStop();
    const tried = new Set<string>();
    while (!io.stopped()) {
      const ready = (await io.call<string[]>("ready_stages")).filter((id) => !tried.has(id));
      // Stop may have been pressed while that was being asked, and nothing new starts after it.
      if (ready.length === 0 || io.stopped()) break;
      const batch = ready.slice(0, 3);
      batch.forEach((id) => tried.add(id));
      if (batch.length === 1) await io.run("run_stage", { engine, stageId: batch[0] });
      else await io.run("run_wave", { engine, stageIds: batch });
      await io.afterWave();
    }
    if (io.stopped()) ended = "stopped";
    // Steps held for the person's answers are not a finished run: the form takes the screen, and the answers carry it on.
    else if ((await io.call<unknown[]>("fact_questions")).length > 0) ended = "questions";
    return ended;
  } catch (e) {
    ended = io.stopped() ? "stopped" : "failed";
    throw e;
  } finally {
    // Waited for, so the project read right after this shows the run as over.
    if (began) await io.call("end_run").catch(() => undefined);
    // A Mac notification, only when SproutCrew is not in front and notifications are on.
    if (began && ended !== "questions") void io.call("notify_run", { outcome: ended }).catch(() => undefined);
  }
}
