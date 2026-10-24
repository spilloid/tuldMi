// Motion tokens (MISSION §2). Mirrored as CSS custom properties in src/styles/tokens.css.
//   microinteractions 100–180 ms · panels 180–280 ms · major views 250–400 ms.
// Every animation must communicate state, continuity, hierarchy, or feedback.

import { useEffect, useState } from "react";

export const duration = {
  /** Hover, press, toggles, small reveals. */
  micro: 0.14,
  /** Panels, popovers, expanding rows, arrivals. */
  panel: 0.22,
  /** Route-level transitions, dialogs. */
  view: 0.32,
} as const;

/** Cubic-bezier tuples for motion/react `transition.ease`. */
export const ease = {
  /** Decelerate into rest. Default for things entering. */
  out: [0.22, 1, 0.36, 1] as [number, number, number, number],
  /** Symmetric, for things moving between two resting states. */
  inOut: [0.65, 0, 0.35, 1] as [number, number, number, number],
  /** Material-ish standard curve for state changes. */
  standard: [0.2, 0, 0, 1] as [number, number, number, number],
};

export const transition = {
  micro: { duration: duration.micro, ease: ease.out },
  panel: { duration: duration.panel, ease: ease.out },
  view: { duration: duration.view, ease: ease.out },
  /** Event arrival in the timeline (spec: 180–240 ms, opacity + translate). */
  arrival: { duration: 0.21, ease: ease.out },
} as const;

const query = "(prefers-reduced-motion: reduce)";

export function prefersReducedMotion(): boolean {
  return typeof window !== "undefined" && !!window.matchMedia?.(query).matches;
}

/** Live reduced-motion preference. */
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(prefersReducedMotion);
  useEffect(() => {
    const mq = window.matchMedia?.(query);
    if (!mq) return;
    const on = () => setReduced(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return reduced;
}
