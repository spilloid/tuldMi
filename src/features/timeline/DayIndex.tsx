import { useEffect, useMemo, useState } from "react";
import type { RefObject } from "react";
import type { SpeechEvent } from "../../lib/ipc/types";
import { DAY, HOUR, dayKey, dayLabel, startOfDay } from "../../lib/time";
import { useReducedMotion } from "../../lib/motion";

interface DaySummary {
  key: string;
  at: number;
  count: number;
  /** Seconds of speech per hour of the day, split by source. */
  hours: { mic: number; system: number }[];
}

export function summarizeDays(events: SpeechEvent[]): DaySummary[] {
  const map = new Map<string, DaySummary>();
  for (const e of events) {
    const k = dayKey(e.startedAt);
    let d = map.get(k);
    if (!d) {
      d = { key: k, at: startOfDay(e.startedAt), count: 0, hours: Array.from({ length: 24 }, () => ({ mic: 0, system: 0 })) };
      map.set(k, d);
    }
    d.count++;
    const h = Math.min(23, Math.floor((e.startedAt - d.at) / HOUR));
    d.hours[h][e.source] += (e.endedAt - e.startedAt) / 1000;
  }
  return [...map.values()].sort((a, b) => b.at - a.at);
}

/**
 * Overview of loaded days on wide windows: each day's speech density across 24 hours
 * (real event durations, Me above / Remote below). Click to jump to that day.
 */
export function DayIndex({ events, scroller }: { events: SpeechEvent[]; scroller: RefObject<HTMLDivElement | null> }) {
  const days = useMemo(() => summarizeDays(events), [events]);
  const reduced = useReducedMotion();
  const [active, setActive] = useState<string | null>(null);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    let raf = 0;
    const update = () => {
      raf = 0;
      const top = el.getBoundingClientRect().top + 60;
      let cur: string | null = null;
      el.querySelectorAll<HTMLElement>("[data-day]").forEach((h) => {
        if (h.getBoundingClientRect().top <= top || cur === null) cur = h.id.slice(4);
      });
      setActive(cur);
    };
    const onScroll = () => {
      if (!raf) raf = requestAnimationFrame(update);
    };
    update();
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      el.removeEventListener("scroll", onScroll);
      if (raf) cancelAnimationFrame(raf);
    };
  }, [scroller, days.length]);

  if (days.length < 2) return null;
  const peak = Math.max(60, ...days.flatMap((d) => d.hours.map((h) => h.mic + h.system)));
  const today = startOfDay(Date.now());

  return (
    <nav aria-label="Days" className="absolute right-5 top-5 hidden w-[184px] min-[1380px]:block">
      <div className="caps mb-2 px-2 text-faint">Days</div>
      <ul className="space-y-0.5">
        {days.map((d) => {
          const on = active === d.key;
          const label = dayLabel(d.at).title;
          const ago = Math.round((today - d.at) / DAY);
          return (
            <li key={d.key}>
              <button
                type="button"
                aria-current={on ? "true" : undefined}
                onClick={() => {
                  document.getElementById(`day-${d.key}`)?.scrollIntoView({ block: "start", behavior: reduced ? "auto" : "smooth" });
                }}
                className={`w-full rounded-md px-2 py-1.5 text-left transition-colors duration-[140ms] ${on ? "bg-selected" : "hover:bg-hover"}`}
              >
                <span className="flex items-baseline justify-between gap-2">
                  <span className={`truncate text-sm ${on ? "font-semibold text-text" : "text-muted"}`}>{ago > 6 ? dayLabel(d.at).detail : label}</span>
                  <span className="text-xs text-faint tnum">{d.count}</span>
                </span>
                <span className="mt-1 flex h-[14px] items-center gap-px" aria-hidden>
                  {d.hours.map((h, i) => {
                    const m = h.mic / peak;
                    const s = h.system / peak;
                    return (
                      <span key={i} className="flex h-full flex-1 flex-col justify-center gap-px">
                        <span className="w-full rounded-[1px] bg-me" style={{ height: m ? `${Math.max(28, Math.sqrt(m) * 50)}%` : 0, opacity: 0.75 }} />
                        <span className="w-full rounded-[1px] bg-remote" style={{ height: s ? `${Math.max(28, Math.sqrt(s) * 50)}%` : 0, opacity: 0.75 }} />
                        {!m && !s && <span className="h-px w-full bg-line" />}
                      </span>
                    );
                  })}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      <div className="mt-2 flex items-center gap-3 px-2 text-[10.5px] text-faint">
        <span className="flex items-center gap-1"><span className="h-1.5 w-1.5 rounded-full bg-me" />Me</span>
        <span className="flex items-center gap-1"><span className="h-1.5 w-1.5 rounded-full bg-remote" />Remote</span>
        <span className="ml-auto">24 h</span>
      </div>
    </nav>
  );
}
