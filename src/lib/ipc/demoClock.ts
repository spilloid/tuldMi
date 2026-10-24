/**
 * Demo-only: `?demo&clock=15:20` pretends the wall clock reads 15:20 today, so screenshots of
 * the sample timeline look the same whatever time they are taken. Never active outside the
 * demo backend.
 */
export function installDemoClock(search: string) {
  const q = new URLSearchParams(search);
  const clock = q.get("clock");
  if (!clock || !/^\d{1,2}:\d{2}$/.test(clock)) return;
  const [h, m] = clock.split(":").map(Number);
  const RealDate = Date;
  const target = new RealDate();
  target.setHours(h, m, 0, 0);
  const offset = target.getTime() - RealDate.now();

  class DemoDate extends RealDate {
    constructor(...args: unknown[]) {
      if (args.length === 0) super(RealDate.now() + offset);
      // @ts-ignore -- forwards any Date constructor overload
      else super(...args);
    }
    static now() {
      return RealDate.now() + offset;
    }
  }
  globalThis.Date = DemoDate as DateConstructor;
}
