/** tuldMi mark: three voice bars and a memory dot. Inherits theme colors. */
export function Logo({ size = 20, className = "" }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" className={className}>
      <rect width="32" height="32" rx="8" fill="var(--tm-text)" />
      <g fill="none" stroke="var(--tm-accent)" strokeWidth="2.6" strokeLinecap="round">
        <path d="M9 13.5v5" />
        <path d="M14 9.5v13" />
        <path d="M19 12v8" />
      </g>
      <circle cx="24" cy="16" r="2.2" fill="var(--tm-accent)" />
    </svg>
  );
}
