import { useEffect, useState } from "react";
import { navigate } from "../../lib/router";
import { EMPTY_FILTERS, SearchPanel } from "./SearchPanel";
import type { RangeKey, SearchFilters, SourceFilter } from "./SearchPanel";

function fromParams(p: URLSearchParams): SearchFilters {
  const src = p.get("src");
  const range = p.get("range");
  return {
    text: p.get("q") ?? "",
    source: (["mic", "system"].includes(src ?? "") ? src : "all") as SourceFilter,
    range: (["today", "7d", "30d", "custom"].includes(range ?? "") ? range : "all") as RangeKey,
    fromDate: p.get("from") ?? "",
    toDate: p.get("to") ?? "",
  };
}

function toParams(f: SearchFilters): string {
  const p = new URLSearchParams();
  if (f.text) p.set("q", f.text);
  if (f.source !== "all") p.set("src", f.source);
  if (f.range !== "all") p.set("range", f.range);
  if (f.range === "custom" && f.fromDate) p.set("from", f.fromDate);
  if (f.range === "custom" && f.toDate) p.set("to", f.toDate);
  const s = p.toString();
  return s ? `?${s}` : "";
}

export function SearchView({ params }: { params: URLSearchParams }) {
  const [filters, setFilters] = useState<SearchFilters>(() => ({ ...EMPTY_FILTERS, ...fromParams(params) }));

  // Mirror filters into the URL (replace, so typing doesn't spam history).
  useEffect(() => {
    navigate(`/search${toParams(filters)}`, { replace: true });
  }, [filters]);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="mx-auto flex min-h-0 w-full max-w-[860px] flex-1 flex-col px-6 pt-8 max-[1100px]:px-4">
        <h1 className="mb-1.5 font-display text-[28px] leading-tight font-[460] tracking-[-0.02em] text-text">Search</h1>
        <p className="mb-5 text-base text-muted">Everything you've said and heard, searched on this device.</p>
        <SearchPanel variant="page" filters={filters} onFilters={setFilters} />
      </div>
    </div>
  );
}
