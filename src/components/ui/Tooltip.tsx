import * as RT from "@radix-ui/react-tooltip";
import type { ReactNode } from "react";
import { Keys } from "./Kbd";

interface Props {
  content: ReactNode;
  shortcut?: string;
  side?: "top" | "bottom" | "left" | "right";
  children: ReactNode;
  delay?: number;
}

export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <RT.Provider delayDuration={450} skipDelayDuration={250}>
      {children}
    </RT.Provider>
  );
}

export function Tooltip({ content, shortcut, side = "top", children, delay }: Props) {
  return (
    <RT.Root delayDuration={delay}>
      <RT.Trigger asChild>{children}</RT.Trigger>
      <RT.Portal>
        <RT.Content
          side={side}
          sideOffset={6}
          collisionPadding={8}
          className="tm-pop z-[80] flex max-w-72 items-center gap-2 rounded-md border border-line bg-overlay px-2 py-1 text-sm text-text shadow-2"
        >
          <span>{content}</span>
          {shortcut && <Keys combo={shortcut} small />}
        </RT.Content>
      </RT.Portal>
    </RT.Root>
  );
}
