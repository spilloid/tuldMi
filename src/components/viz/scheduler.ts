/**
 * One shared requestAnimationFrame loop for every canvas in the app.
 *
 * - The loop only runs while at least one task is awake AND visible (on screen, document
 *   visible). Otherwise no rAF is pending at all.
 * - Each task declares its own frame budget (60 or 30 fps); the loop never exceeds 60 fps.
 * - A task's `frame()` returns the fps it wants next, or 0 to go dormant until `wake()`.
 */

export interface VizTask {
  /** Element whose visibility gates the task (usually the canvas). */
  el: Element;
  /** Draw. Return desired fps for subsequent frames (60 / 30), or 0 to sleep. */
  frame(now: number): number;
}

interface Entry {
  task: VizTask;
  awake: boolean;
  visible: boolean;
  fps: number;
  last: number;
}

const MAX_FPS = 60;

class Scheduler {
  private entries = new Map<VizTask, Entry>();
  private raf: number | null = null;
  private io: IntersectionObserver | null = null;
  private docVisible = typeof document === "undefined" ? true : document.visibilityState !== "hidden";
  private lastLoop = 0;

  constructor() {
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", () => {
        this.docVisible = document.visibilityState !== "hidden";
        this.kick();
      });
    }
  }

  private observer(): IntersectionObserver | null {
    if (this.io || typeof IntersectionObserver === "undefined") return this.io;
    this.io = new IntersectionObserver((records) => {
      for (const r of records) {
        for (const e of this.entries.values()) {
          if (e.task.el === r.target) e.visible = r.isIntersecting;
        }
      }
      this.kick();
    });
    return this.io;
  }

  add(task: VizTask): () => void {
    const entry: Entry = { task, awake: true, visible: true, fps: MAX_FPS, last: 0 };
    this.entries.set(task, entry);
    this.observer()?.observe(task.el);
    this.kick();
    return () => {
      this.entries.delete(task);
      this.io?.unobserve(task.el);
      if (this.entries.size === 0 && this.raf != null) {
        cancelAnimationFrame(this.raf);
        this.raf = null;
      }
    };
  }

  /** Request frames for a task (e.g. new data arrived). Cheap to call often. */
  wake(task: VizTask) {
    const e = this.entries.get(task);
    if (!e) return;
    if (!e.awake) {
      e.awake = true;
      e.fps = MAX_FPS;
    }
    this.kick();
  }

  private runnable(e: Entry) {
    return e.awake && e.visible && this.docVisible;
  }

  private kick() {
    if (this.raf != null) return;
    for (const e of this.entries.values()) {
      if (this.runnable(e)) {
        this.raf = requestAnimationFrame(this.loop);
        return;
      }
    }
  }

  private loop = (now: number) => {
    this.raf = null;
    // Global cap: on >60 Hz displays skip frames that come too soon.
    if (now - this.lastLoop < 1000 / MAX_FPS - 2) {
      this.raf = requestAnimationFrame(this.loop);
      return;
    }
    this.lastLoop = now;
    let any = false;
    for (const e of this.entries.values()) {
      if (!this.runnable(e)) continue;
      any = true;
      if (now - e.last < 1000 / e.fps - 2) continue;
      e.last = now;
      const fps = e.task.frame(now);
      if (fps <= 0) e.awake = false;
      else e.fps = Math.min(MAX_FPS, fps);
    }
    if (any) this.kick();
  };

  /** For tests/diagnostics. */
  get running() {
    return this.raf != null;
  }
}

export const scheduler = new Scheduler();
