import { AnimatePresence, motion } from "motion/react";
import { ArrowDown, Loader2 } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { SpeechEvent } from "../../lib/ipc/types";
import type { Conversation } from "../../lib/time";
import { buildTimeline, clock, dayLabel, humanDuration } from "../../lib/time";
import { isTypingTarget, overlayOpen, plain } from "../../lib/keys";
import { transition, useReducedMotion } from "../../lib/motion";
import { useStatus, useTimeline } from "../../state/hooks";
import { useUi } from "../../state/ui";
import { EventRow } from "./EventRow";
import { TimelineEmpty } from "./TimelineEmpty";

const BOTTOM_SLACK = 64;

export function Timeline({ routeEventId }: { routeEventId?: string }) {
  const { events, loading, hasMore, error, arrivals, loadOlder, ensureLoaded } = useTimeline();
  const status = useStatus();
  const ui = useUi();
  const reduced = useReducedMotion();
  const scroller = useRef<HTMLDivElement>(null);
  const topSentinel = useRef<HTMLDivElement>(null);
  const [atBottom, setAtBottom] = useState(true);
  const atBottomRef = useRef(true);
  const [unseen, setUnseen] = useState<string[]>([]);
  const initialScrolled = useRef(false);
  const lastSeq = useRef(0);
  const arrivedIds = useRef(new Set<string>());
  const anchor = useRef<{ firstId: string; height: number; top: number } | null>(null);

  const rows = useMemo(() => buildTimeline(events), [events]);
  const flat = useMemo(() => events.map((e) => e.id), [events]);

  // ---- scroll bookkeeping ------------------------------------------------------------------

  const scrollToBottom = useCallback(
    (smooth: boolean) => {
      const el = scroller.current;
      if (!el) return;
      el.scrollTo({ top: el.scrollHeight, behavior: smooth && !reduced ? "smooth" : "auto" });
    },
    [reduced],
  );

  const onScroll = () => {
    const el = scroller.current!;
    const bottom = el.scrollHeight - el.scrollTop - el.clientHeight < BOTTOM_SLACK;
    if (bottom !== atBottomRef.current) {
      atBottomRef.current = bottom;
      setAtBottom(bottom);
    }
    if (bottom && unseen.length) setUnseen([]);
  };

  // First load: start at "now".
  useLayoutEffect(() => {
    if (!initialScrolled.current && !loading && events.length) {
      initialScrolled.current = true;
      scrollToBottom(false);
    }
  }, [loading, events.length, scrollToBottom]);

  // Keep position when older history is prepended.
  useLayoutEffect(() => {
    const el = scroller.current;
    const a = anchor.current;
    if (!el || !a) return;
    if (events[0] && events[0].id !== a.firstId) {
      el.scrollTop = a.top + (el.scrollHeight - a.height);
      anchor.current = null;
    }
  }, [events]);

  // Live arrivals: follow if the user is at the bottom; otherwise count them. Never yank.
  useLayoutEffect(() => {
    const fresh = arrivals.filter((a) => a.seq > lastSeq.current);
    if (!fresh.length) return;
    lastSeq.current = fresh[fresh.length - 1].seq;
    const newestStart = events[events.length - 1]?.startedAt ?? 0;
    const live = fresh.filter((a) => {
      const ev = events.find((e) => e.id === a.id);
      return ev && ev.startedAt >= newestStart - 10 * 60_000;
    });
    live.forEach((a) => arrivedIds.current.add(a.id));
    if (!live.length) return;
    if (atBottomRef.current) requestAnimationFrame(() => scrollToBottom(true));
    else setUnseen((u) => [...new Set([...u, ...live.map((a) => a.id)])]);
  }, [arrivals, events, scrollToBottom]);

  // Infinite scroll upward-in-time.
  useEffect(() => {
    const s = topSentinel.current;
    const root = scroller.current;
    if (!s || !root || !hasMore) return;
    const io = new IntersectionObserver(
      (rec) => {
        if (rec[0].isIntersecting && initialScrolled.current && events.length) {
          anchor.current = { firstId: events[0].id, height: root.scrollHeight, top: root.scrollTop };
          void loadOlder();
        }
      },
      { root, rootMargin: "400px 0px 0px 0px" },
    );
    io.observe(s);
    return () => io.disconnect();
  }, [hasMore, events, loadOlder]);

  // Reveal the selected event (search jump, J/K).
  useEffect(() => {
    if (!ui.selectedId || !ui.revealSeq) return;
    const el = scroller.current?.querySelector<HTMLElement>(`[data-event-id="${CSS.escape(ui.selectedId)}"]`);
    if (el) {
      el.scrollIntoView({ block: "center", behavior: reduced ? "auto" : "smooth" });
      el.querySelector<HTMLElement>("[role=button]")?.focus({ preventScroll: true });
    }
  }, [ui.revealSeq, ui.selectedId, reduced, rows]);

  // Deep link: #/timeline/<id>
  useEffect(() => {
    if (!routeEventId) return;
    // `@N` = the Nth most recent event (handy for links and screenshots).
    if (routeEventId.startsWith("@")) {
      if (loading || !events.length) return;
      const ev = events[Math.max(0, events.length - 1 - (Number(routeEventId.slice(1)) || 0))];
      requestAnimationFrame(() => ui.jumpTo(ev.id));
      return;
    }
    void ensureLoaded(routeEventId).then((ev) => ev && requestAnimationFrame(() => ui.jumpTo(ev.id)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [routeEventId, ensureLoaded, loading]);

  const backToLive = useCallback(() => {
    setUnseen([]);
    scrollToBottom(true);
  }, [scrollToBottom]);

  // ---- keyboard: J/K, Enter, L/End ---------------------------------------------------------

  const uiRef = useRef(ui);
  uiRef.current = ui;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || isTypingTarget(e.target) || overlayOpen()) return;
      const u = uiRef.current;
      if (plain(e) && (e.key === "j" || e.key === "k")) {
        if (!flat.length) return;
        e.preventDefault();
        const i = u.selectedId ? flat.indexOf(u.selectedId) : -1;
        const next = i < 0 ? flat.length - 1 : Math.max(0, Math.min(flat.length - 1, i + (e.key === "j" ? 1 : -1)));
        u.select(flat[next], { reveal: true });
      } else if (plain(e) && e.key === "Enter" && u.selectedId && !(e.target as HTMLElement).closest?.("[role=button],button")) {
        e.preventDefault();
        u.toggleExpanded(u.selectedId);
      } else if ((plain(e) && e.key === "l") || (e.key === "End" && !e.ctrlKey && !(e.target as HTMLElement).closest?.("[role=slider]"))) {
        e.preventDefault();
        backToLive();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [flat, backToLive]);

  const onSelect = useCallback((id: string) => uiRef.current.select(id), []);
  const onToggle = useCallback((id: string) => uiRef.current.toggleExpanded(id), []);

  // ---- render ----------------------------------------------------------------------------

  if (!loading && events.length === 0 && !error) {
    return <TimelineEmpty status={status} />;
  }

  const showPill = !atBottom && (unseen.length > 0 || events.length > 0);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div
        ref={scroller}
        onScroll={onScroll}
        className="min-h-0 flex-1 overflow-y-auto overscroll-contain [scrollbar-gutter:stable]"
        role="feed"
        aria-busy={loading}
        aria-label="Conversation timeline"
      >
        <div className="mx-auto w-full max-w-[820px] px-6 pb-10 max-[1100px]:px-4">
          <div ref={topSentinel} className="h-px" />
          {hasMore && events.length > 0 && (
            <div className="flex h-12 items-center justify-center gap-2 text-sm text-faint">
              {loading ? <Loader2 size={14} className="animate-spin motion-reduce:animate-none" /> : null}
              {loading ? "Loading earlier conversations…" : "Scroll for earlier conversations"}
            </div>
          )}
          {!hasMore && events.length > 0 && <HistoryStart at={events[0].startedAt} />}
          {loading && events.length === 0 && <TimelineSkeleton />}
          {error && (
            <div className="my-6 rounded-lg border border-line bg-surface p-4 text-base text-muted">
              Couldn't load the timeline: <span className="text-text">{error}</span>
            </div>
          )}
          {rows.map((row) => {
            if (row.kind === "day") return <DayHeader key={row.key} at={row.at} events={events} />;
            if (row.kind === "gap") return <GapRow key={row.key} from={row.from} to={row.to} />;
            return (
              <ConversationBlock
                key={row.key}
                conversation={row.conversation}
                selectedId={ui.selectedId}
                expanded={ui.expanded}
                flashId={ui.flashId}
                arrived={arrivedIds.current}
                onSelect={onSelect}
                onToggle={onToggle}
              />
            );
          })}
          <LiveEdge state={status?.state} />
        </div>
      </div>

      <AnimatePresence>
        {showPill && (
          <motion.div
            initial={reduced ? { opacity: 0 } : { opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={reduced ? { opacity: 0 } : { opacity: 0, y: 8 }}
            transition={transition.panel}
            className="pointer-events-none absolute inset-x-0 bottom-4 flex justify-center"
          >
            <button
              type="button"
              onClick={backToLive}
              className="pointer-events-auto inline-flex h-8 items-center gap-2 rounded-full border border-line bg-overlay pl-3 pr-2 text-sm font-medium text-text shadow-2 transition-colors hover:border-line-strong"
            >
              {unseen.length > 0 && (
                <>
                  <span className="h-1.5 w-1.5 rounded-full bg-accent" aria-hidden />
                  <span className="tnum">{unseen.length} new</span>
                  <span className="text-faint" aria-hidden>·</span>
                </>
              )}
              <span>Back to live</span>
              <span className="inline-flex h-5 w-5 items-center justify-center rounded-full bg-sunken">
                <ArrowDown size={12} />
              </span>
              <span className="kbd !h-[18px] !min-w-[18px] !text-[10px]">L</span>
            </button>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

// ---- pieces ------------------------------------------------------------------------------------

function DayHeader({ at, events }: { at: number; events: SpeechEvent[] }) {
  const { title, detail } = dayLabel(at);
  const dayEnd = at + 86_400_000;
  const dayEvents = events.filter((e) => e.startedAt >= at && e.startedAt < dayEnd);
  const speech = dayEvents.reduce((n, e) => n + (e.endedAt - e.startedAt), 0);
  return (
    <div className="sticky top-0 z-10 -mx-6 mb-2 mt-6 bg-canvas px-6 pb-2 pt-4 max-[1100px]:-mx-4 max-[1100px]:px-4">
      <div className="flex items-baseline gap-3 border-b border-line-soft pb-2.5">
        <h2 className="font-display text-[23px] font-[480] leading-none tracking-[-0.015em] text-text">{title}</h2>
        <span className="text-sm text-muted">{detail}</span>
        <span className="ml-auto text-xs text-faint tnum">
          {dayEvents.length} {dayEvents.length === 1 ? "event" : "events"} · {humanDuration(speech)} of speech
        </span>
      </div>
    </div>
  );
}

function GapRow({ from, to }: { from: number; to: number }) {
  return (
    <div className="my-1 grid grid-cols-[56px_1fr] items-center py-2" aria-label={`${humanDuration(to - from)} without speech`}>
      <span />
      <div className="flex items-center gap-3 text-xs text-faint">
        <span className="h-px w-6 bg-line" aria-hidden />
        <span className="tnum">{humanDuration(to - from)} quiet</span>
        <span className="h-px flex-1 bg-line-soft" aria-hidden />
      </div>
    </div>
  );
}

function ConversationBlock({
  conversation: c,
  selectedId,
  expanded,
  flashId,
  arrived,
  onSelect,
  onToggle,
}: {
  conversation: Conversation;
  selectedId: string | null;
  expanded: Set<string>;
  flashId: string | null;
  arrived: Set<string>;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
}) {
  const who = [c.sources.mic ? "Me" : null, c.sources.system ? "Remote" : null].filter(Boolean).join(" & ");
  const dur = c.endedAt - c.startedAt;
  return (
    <section aria-label={`Conversation at ${clock(c.startedAt)}`} className="mb-2">
      <div className="grid grid-cols-[56px_1fr] items-center pb-1 pt-2">
        <span />
        <div className="flex items-center gap-2 pl-[2px] text-xs text-faint">
          <span className="font-medium text-muted tnum">
            {clock(c.startedAt)}–{clock(c.endedAt)}
          </span>
          <span aria-hidden>·</span>
          <span>{c.events.length === 1 ? humanDuration(dur) : `${humanDuration(dur)}, ${c.events.length} events`}</span>
          <span aria-hidden>·</span>
          <span>{who}</span>
        </div>
      </div>
      <div className="flex flex-col gap-px">
        {c.events.map((ev, i) => {
          const prev = c.events[i - 1];
          return (
            <EventRow
              key={ev.id}
              ev={ev}
              showLabel={!prev || prev.source !== ev.source}
              showTime={!prev || clock(prev.startedAt) !== clock(ev.startedAt)}
              selected={selectedId === ev.id}
              expanded={expanded.has(ev.id)}
              flash={flashId === ev.id}
              arrived={arrived.has(ev.id)}
              onSelect={onSelect}
              onToggle={onToggle}
            />
          );
        })}
      </div>
    </section>
  );
}

function HistoryStart({ at }: { at: number }) {
  return (
    <div className="flex flex-col items-center gap-1 pb-2 pt-10 text-center">
      <span className="font-display text-md italic text-muted">The beginning of your timeline</span>
      <span className="text-xs text-faint">First recorded {new Date(at).toLocaleDateString(undefined, { dateStyle: "long" })}</span>
    </div>
  );
}

function LiveEdge({ state }: { state?: string }) {
  if (state !== "recording" && state !== "paused") return <div className="h-6" />;
  return (
    <div className="mt-4 grid grid-cols-[56px_1fr] items-center">
      <span />
      <div className="flex items-center gap-2.5 text-xs">
        <span className={`h-1.5 w-1.5 rounded-full ${state === "recording" ? "bg-rec" : "bg-faint"}`} aria-hidden />
        <span className={state === "recording" ? "text-muted" : "text-faint"}>
          {state === "recording" ? "Listening — new speech appears here" : "Paused — nothing is being captured"}
        </span>
        <span className="h-px flex-1 bg-line-soft" aria-hidden />
      </div>
    </div>
  );
}

function TimelineSkeleton() {
  return (
    <div className="space-y-6 pt-10" aria-hidden>
      {[0, 1, 2].map((i) => (
        <div key={i} className="grid grid-cols-[56px_1fr] gap-y-3">
          <div className="skeleton ml-auto mr-3 h-2.5 w-8" />
          <div className="space-y-2">
            <div className="skeleton h-2.5 w-24" />
            <div className="skeleton h-2.5 w-[86%]" />
            <div className="skeleton h-2.5 w-[60%]" />
          </div>
        </div>
      ))}
    </div>
  );
}
