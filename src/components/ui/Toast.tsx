import { AnimatePresence, motion } from "motion/react";
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-react";
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { transition, useReducedMotion } from "../../lib/motion";

export type ToastLevel = "info" | "success" | "warning" | "error";

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface ToastInput {
  level?: ToastLevel;
  title?: string;
  message: string;
  action?: ToastAction;
  /** ms; 0 = sticky until dismissed. Defaults: 5 s, errors 9 s, with an action 8 s. */
  duration?: number;
  /** Collapse duplicates: a toast with the same key replaces the previous one. */
  key?: string;
}

interface ToastItem extends ToastInput {
  id: number;
  level: ToastLevel;
}

interface ToastApi {
  show(t: ToastInput): number;
  dismiss(id: number): void;
}

const Ctx = createContext<ToastApi | null>(null);

/** Toast system. Also exposed on `window.tuldmiToast` so shell-driven flows can use it. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const seq = useRef(0);
  const reduced = useReducedMotion();

  const dismiss = useCallback((id: number) => setItems((all) => all.filter((t) => t.id !== id)), []);
  const show = useCallback((t: ToastInput) => {
    const id = ++seq.current;
    setItems((all) => {
      const rest = t.key ? all.filter((x) => x.key !== t.key) : all;
      return [...rest.slice(-3), { ...t, id, level: t.level ?? "info" }];
    });
    return id;
  }, []);
  const api = useMemo(() => ({ show, dismiss }), [show, dismiss]);

  useEffect(() => {
    (window as unknown as { tuldmiToast?: ToastApi }).tuldmiToast = api;
  }, [api]);

  return (
    <Ctx.Provider value={api}>
      {children}
      <div
        aria-live="polite"
        aria-relevant="additions"
        className="pointer-events-none fixed bottom-[calc(var(--tm-capturebar-h)+16px)] right-4 z-[90] flex w-[360px] max-w-[calc(100vw-32px)] flex-col items-end gap-2"
      >
        <AnimatePresence initial={false}>
          {items.map((t) => (
            <motion.div
              key={t.id}
              layout={!reduced}
              initial={reduced ? { opacity: 0 } : { opacity: 0, y: 10, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={reduced ? { opacity: 0 } : { opacity: 0, x: 24, transition: transition.micro }}
              transition={transition.panel}
              className="pointer-events-auto w-full"
            >
              <ToastCard item={t} onDismiss={() => dismiss(t.id)} />
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </Ctx.Provider>
  );
}

const icons = {
  info: <Info size={16} className="text-muted" />,
  success: <CheckCircle2 size={16} className="text-success" />,
  warning: <AlertTriangle size={16} className="text-warning" />,
  error: <XCircle size={16} className="text-danger" />,
};

function ToastCard({ item, onDismiss }: { item: ToastItem; onDismiss: () => void }) {
  const [hover, setHover] = useState(false);
  const duration = item.duration ?? (item.level === "error" ? 9000 : item.action ? 8000 : 5000);
  useEffect(() => {
    if (!duration || hover) return;
    const t = setTimeout(onDismiss, duration);
    return () => clearTimeout(t);
  }, [duration, hover, onDismiss]);
  return (
    <div
      role={item.level === "error" ? "alert" : "status"}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      className="flex items-start gap-3 rounded-lg border border-line bg-overlay px-3.5 py-3 shadow-3"
    >
      <span className="mt-0.5 shrink-0">{icons[item.level]}</span>
      <div className="min-w-0 flex-1">
        {item.title && <div className="text-base font-semibold text-text">{item.title}</div>}
        <div className="selectable text-base text-muted">{item.message}</div>
        {item.action && (
          <button
            type="button"
            onClick={() => {
              item.action!.onClick();
              onDismiss();
            }}
            className="mt-2 -ml-1 rounded px-1 text-sm font-semibold text-accent-text hover:underline"
          >
            {item.action.label}
          </button>
        )}
      </div>
      <button type="button" aria-label="Dismiss" onClick={onDismiss} className="-mr-1 -mt-0.5 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-faint hover:bg-hover hover:text-text">
        <X size={14} />
      </button>
    </div>
  );
}

export function useToast(): ToastApi {
  const c = useContext(Ctx);
  if (!c) throw new Error("ToastProvider missing");
  return c;
}
