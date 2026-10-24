/** True when a keystroke should be left to a text field / editable control. */
export function isTypingTarget(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el || !el.tagName) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (el.isContentEditable) return true;
  return false;
}

/** True when a modal surface (dialog / menu) currently owns the keyboard. */
export function overlayOpen(): boolean {
  return !!document.querySelector('[role="dialog"][data-state="open"], [role="menu"][data-state="open"], [role="alertdialog"][data-state="open"]');
}

/** Plain key with no modifiers (Shift allowed when `shift` is true). */
export function plain(e: KeyboardEvent, shift = false): boolean {
  return !e.ctrlKey && !e.metaKey && !e.altKey && (shift || !e.shiftKey);
}
