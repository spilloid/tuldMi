import { CalendarRange, CornerDownLeft, Loader2, Search, SearchX, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, RefObject } from "react";
import type { SearchHit, SearchQuery, Source } from "../../lib/ipc/types";
import { splitSnippet } from "../../lib/snippet";
import { DAY, clock, dayKey, dayLabel, relativeAgo, startOfDay } from "../../lib/time";
import { navigate } from "../../lib/router";
import { useSearch, useStore } from "../../state/hooks";
import { useUi } from "../../state/ui";
import { Segmented } from "../../components/ui/controls";

export type RangeKey = "today" | "7d" | "30d" | "all" | "custom";
export type SourceFilter = "all" | Source;

export interface SearchFilters {
  text: string;
  source: SourceFilter;
  range: RangeKey;
  /** yyyy-mm-dd, for custom range */
  fromDate: string;
  toDate: string;
}

export const EMPTY_FILTERS: SearchFilters = { text: "", source: "all", range: "all", fromDate: "", toDate: "" };

export function toQuery(f: SearchFilters, now = Date.now(), limit = 50): SearchQuery {
  let from: number | null = null;
  let to: number | null = null;
  if (f.range === "today") from = startOfDay(now);
  else if (f.range === "7d") from = startOfDay(now) - 6 * DAY;
  else if (f.range === "30d") from = startOfDay(now) - 29 * DAY;
  else if (f.range === "custom") {
    if (f.fromDate) from = new Date(`${f.fromDate}T00:00:00`).getTime();
    if (f.toDate) to = new Date(`${f.toDate}T23:59:59.999`).getTime();
  }
  return { text: f.text, sources: f.source === "all" ? [] : [f.source], from, to, limit };
}

/** Rounded to the minute so the debounced query key is stable while typing. */
function useMinute() {
  const [m, setM] = useState(() => Math.floor(Date.now() / 60_000) * 60_000);
  useEffect(() => {
    const t = setInterval(() => setM(Math.floor(Date.now() / 60_000) * 60_000), 30_000);
    return () => clearInterval(t);
  }, []);
  return m;
}

export function Highlighted({ snippet }: { snippet: string }) {
  return (
    <>
      {splitSnippet(snippet).map((p, i) =>
        p.hit ? (
          <mark key={i} className="rounded-[3px] bg-accent-soft px-[1px] text-text [box-decoration-break:clone]">
            {p.text}
          </mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}

export function useOpenHit() {
  const store = useStore();
  const ui = useUi();
  return async (hit: SearchHit) => {
    ui.setPaletteOpen(false);
    navigate("/timeline");
    const ev = await store.ensureLoaded(hit.eventId);
    if (ev) requestAnimationFrame(() => ui.jumpTo(ev.id));
  };
}

interface Props {
  filters: SearchFilters;
  onFilters: (f: SearchFilters) => void;
  variant: "palette" | "page";
  inputRef?: RefObject<HTMLInputElement | null>;
}

export function SearchPanel({ filters, onFilters, variant, inputRef }: Props) {
  const minute = useMinute();
  const query = useMemo(() => toQuery(filters, minute, variant === "page" ? 200 : 40), [filters, minute, variant]);
  const { hits, pending, error, settled } = useSearch(query);
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const open = useOpenHit();
  const hasText = filters.text.trim().length > 0;

  useEffect(() => setActive(0), [hits]);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-hit-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(hits.length - 1, a + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === "Enter" && hits[active]) {
      e.preventDefault();
      void open(hits[active]);
    }
  };

  const groups = useMemo(() => {
    const out: { key: string; at: number; items: { hit: SearchHit; index: number }[] }[] = [];
    hits.forEach((hit, index) => {
      const k = dayKey(hit.at);
      let g = out.find((x) => x.key === k);
      if (!g) out.push((g = { key: k, at: startOfDay(hit.at), items: [] }));
      g.items.push({ hit, index });
    });
    return out;
  }, [hits]);

  const page = variant === "page";

  return (
    <div className="flex min-h-0 flex-1 flex-col" onKeyDown={onKeyDown}>
      {/* Query */}
      <div className={`flex items-center gap-3 ${page ? "h-12 rounded-xl border border-line bg-surface px-4 shadow-1 focus-within:border-accent-line" : "h-14 border-b border-line-soft px-4"}`}>
        {pending && hasText ? (
          <Loader2 size={17} className="shrink-0 animate-spin text-faint motion-reduce:animate-none" aria-hidden />
        ) : (
          <Search size={17} className="shrink-0 text-faint" aria-hidden />
        )}
        <input
          ref={inputRef}
          autoFocus
          value={filters.text}
          onChange={(e) => onFilters({ ...filters, text: e.target.value })}
          placeholder="Search everything you've heard and said"
          aria-label="Search transcripts"
          aria-controls="search-results"
          aria-activedescendant={hits[active] ? `hit-${active}` : undefined}
          spellCheck={false}
          className="min-w-0 flex-1 bg-transparent text-[16px] text-text outline-none placeholder:text-faint"
          style={{ outline: "none" }}
        />
        {filters.text && (
          <button type="button" aria-label="Clear search" onClick={() => onFilters({ ...filters, text: "" })} className="inline-flex h-6 w-6 items-center justify-center rounded text-faint hover:bg-hover hover:text-text">
            <X size={14} />
          </button>
        )}
      </div>

      {/* Filters */}
      <div className={`flex flex-wrap items-center gap-2 ${page ? "pb-2 pt-3" : "border-b border-line-soft px-4 py-2"}`}>
        <Segmented
          size="sm"
          label="Source"
          value={filters.source}
          onChange={(v) => onFilters({ ...filters, source: v })}
          options={[
            { value: "all", label: "All" },
            { value: "mic", label: <><span aria-hidden className="h-1.5 w-1.5 rounded-full bg-me" />Me</> },
            { value: "system", label: <><span aria-hidden className="h-1.5 w-1.5 rounded-full bg-remote" />Remote</> },
          ]}
        />
        <Segmented
          size="sm"
          label="Date range"
          value={filters.range}
          onChange={(v) => onFilters({ ...filters, range: v })}
          options={[
            { value: "today", label: "Today" },
            { value: "7d", label: "7 days" },
            { value: "30d", label: "30 days" },
            { value: "all", label: "All" },
            { value: "custom", label: <><CalendarRange size={12} aria-hidden />Custom</> },
          ]}
        />
        {filters.range === "custom" && (
          <div className="flex items-center gap-1.5 text-sm text-muted">
            <DateInput label="From" value={filters.fromDate} onChange={(v) => onFilters({ ...filters, fromDate: v })} />
            <span className="text-faint">–</span>
            <DateInput label="To" value={filters.toDate} onChange={(v) => onFilters({ ...filters, toDate: v })} />
          </div>
        )}
        {hasText && settled && !error && (
          <span className="ml-auto text-xs text-faint tnum" aria-live="polite">
            {hits.length === 0 ? "No matches" : `${hits.length}${hits.length >= (query.limit ?? 50) ? "+" : ""} ${hits.length === 1 ? "match" : "matches"}`}
          </span>
        )}
      </div>

      {/* Results */}
      <div ref={listRef} id="search-results" role="listbox" aria-label="Search results" className={`min-h-0 flex-1 overflow-y-auto ${page ? "pt-2" : "px-2 py-2"}`}>
        {!hasText && <SearchIntro page={page} />}
        {hasText && error && <div className="px-3 py-8 text-center text-base text-danger">{error}</div>}
        {hasText && settled && !error && hits.length === 0 && <NoResults filters={filters} onFilters={onFilters} />}
        {groups.map((g) => (
          <div key={g.key} role="group" aria-label={dayLabel(g.at).title} className="mb-1">
            <div className={`caps sticky top-0 z-[1] bg-inherit px-3 pb-1 pt-2 text-faint ${page ? "bg-canvas" : "bg-overlay"}`}>
              {dayLabel(g.at).title}
              <span className="ml-2 font-normal normal-case tracking-normal">{dayLabel(g.at).title === "Today" || dayLabel(g.at).title === "Yesterday" ? "" : dayLabel(g.at).detail}</span>
            </div>
            {g.items.map(({ hit, index }) => (
              <HitRow key={hit.segmentId} hit={hit} index={index} active={index === active} onHover={() => setActive(index)} onOpen={() => void open(hit)} page={page} />
            ))}
          </div>
        ))}
      </div>

      {!page && (
        <div className="flex h-9 items-center gap-4 border-t border-line-soft px-4 text-xs text-faint">
          <span className="flex items-center gap-1.5"><kbd className="kbd !h-[18px]">↑</kbd><kbd className="kbd !h-[18px]">↓</kbd> Navigate</span>
          <span className="flex items-center gap-1.5"><kbd className="kbd !h-[18px]"><CornerDownLeft size={10} /></kbd> Jump to moment</span>
          <span className="flex items-center gap-1.5"><kbd className="kbd !h-[18px]">Esc</kbd> Close</span>
          <span className="ml-auto">Searched on this device</span>
        </div>
      )}
    </div>
  );
}

function DateInput({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  return (
    <input
      type="date"
      aria-label={label}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="h-7 rounded-md border border-line bg-raised px-2 text-sm text-text [color-scheme:inherit]"
    />
  );
}

function HitRow({ hit, index, active, onHover, onOpen, page }: { hit: SearchHit; index: number; active: boolean; onHover: () => void; onOpen: () => void; page: boolean }) {
  const me = hit.source === "mic";
  return (
    <div
      id={`hit-${index}`}
      role="option"
      aria-selected={active}
      data-hit-index={index}
      onMouseMove={onHover}
      onClick={onOpen}
      className={`grid cursor-default grid-cols-[64px_minmax(0,1fr)_auto] items-baseline gap-3 rounded-lg px-3 ${page ? "py-2.5" : "py-2"} ${active ? "bg-selected" : ""}`}
    >
      <span className="flex items-center gap-1.5 text-xs font-semibold">
        <span aria-hidden className={`h-1.5 w-1.5 shrink-0 rounded-full ${me ? "bg-me" : "bg-remote"}`} />
        <span className={me ? "text-me-text" : "text-remote-text"}>{me ? "Me" : "Remote"}</span>
      </span>
      <p className={`min-w-0 text-base leading-[21px] text-muted ${page ? "" : "truncate"}`}>
        <Highlighted snippet={hit.snippet} />
      </p>
      <span className="flex items-center gap-2 whitespace-nowrap text-xs text-faint">
        <span className="font-mono tnum">{clock(hit.at)}</span>
        {page && <span>{relativeAgo(hit.at)}</span>}
        {active && <CornerDownLeft size={12} className="text-muted" aria-hidden />}
      </span>
    </div>
  );
}

function SearchIntro({ page }: { page: boolean }) {
  const tips = [
    ["Words, not exact phrases", "Every word must appear; endings match too — “budget” finds “budgets”."],
    ["Filter by who spoke", "Me is your microphone, Remote is everything through your speakers."],
    ["Jump to the moment", "Enter opens the conversation in the timeline, highlighted."],
  ];
  return (
    <div className={`${page ? "pt-10" : "px-3 py-6"}`}>
      <div className={`grid gap-3 ${page ? "max-w-[640px] grid-cols-3 max-[1100px]:grid-cols-1" : "grid-cols-1"}`}>
        {tips.map(([t, d]) => (
          <div key={t} className={page ? "rounded-lg border border-line-soft bg-surface p-3.5" : "flex gap-3"}>
            <div className="text-base font-medium text-text">{t}</div>
            <div className="text-sm text-faint">{d}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

function NoResults({ filters, onFilters }: { filters: SearchFilters; onFilters: (f: SearchFilters) => void }) {
  const narrowed = filters.source !== "all" || filters.range !== "all";
  return (
    <div className="flex flex-col items-center px-6 py-12 text-center">
      <SearchX size={22} className="mb-3 text-faint" aria-hidden />
      <div className="text-md font-medium text-text">Nothing matches “{filters.text.trim()}”</div>
      <p className="mt-1 max-w-[360px] text-base text-muted">
        {narrowed ? "You're searching a narrower slice of your history." : "Try fewer or shorter words — every word has to appear in the same moment."}
      </p>
      {narrowed && (
        <button type="button" onClick={() => onFilters({ ...filters, source: "all", range: "all" })} className="mt-3 text-sm font-semibold text-accent-text hover:underline">
          Search all sources and dates
        </button>
      )}
    </div>
  );
}
