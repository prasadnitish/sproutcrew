/** Which pieces of work are in flight, and which of them the person stopped. A stopped run is over only when the
 *  work that was in flight at Stop has returned: work started after Stop, such as a question, is not part of it. */
export class RunTracker {
  private next = 1;
  private live = new Set<number>();
  private stopped: Set<number> | null = null;

  start(): number {
    const id = this.next++;
    this.live.add(id);
    return id;
  }

  /** Ends a piece of work. `idle`: nothing is in flight. `released`: the stopped work has all returned. */
  finish(id: number): { idle: boolean; released: boolean } {
    this.live.delete(id);
    let released = false;
    if (this.stopped) {
      this.stopped.delete(id);
      if (this.stopped.size === 0) { this.stopped = null; released = true; }
    }
    return { idle: this.live.size === 0, released };
  }

  /** Remembers the work in flight now as stopped. False when there is nothing to stop or Stop was already pressed. */
  stop(): boolean {
    if (this.stopped || this.live.size === 0) return false;
    this.stopped = new Set(this.live);
    return true;
  }

  get stopping(): boolean { return this.stopped !== null; }
  get idle(): boolean { return this.live.size === 0; }
}
