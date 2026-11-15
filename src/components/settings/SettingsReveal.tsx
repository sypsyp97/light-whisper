import { useEffect, useId, useState, type CSSProperties, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { prefersReducedMotion } from "@/lib/motion";

const REVEAL_MS = 280;


export function SettingsReveal({ open, gap = 0, id, children }: {
  open: boolean;
  gap?: number;
  id?: string;
  children: ReactNode;
}) {
  const [present, setPresent] = useState(open);
  const [settled, setSettled] = useState(open);
  useEffect(() => {
    if (prefersReducedMotion()) {
      setPresent(open);
      setSettled(open);
      return;
    }
    setSettled(false);
    if (open) setPresent(true);
    const timer = setTimeout(() => {
      setPresent(open);
      setSettled(open);
    }, REVEAL_MS);
    return () => clearTimeout(timer);
  }, [open]);

  return (
    <div id={id} className="settings-reveal" data-open={open} data-settled={open && settled}
      aria-hidden={!open} inert={!open}
      style={{ "--reveal-gap": `${gap}px`, "--reveal-duration": `${REVEAL_MS}ms` } as CSSProperties}>
      <div className="settings-reveal-content">{(open || present) && children}</div>
    </div>
  );
}

export function SettingsDisclosure({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <div className="settings-disclosure">
      <button type="button" className="settings-disclosure-trigger" aria-expanded={open}
        aria-controls={id} onClick={() => setOpen(!open)}>
        <ChevronRight size={12} aria-hidden="true" />{label}
      </button>
      <SettingsReveal open={open} id={id}>
        <div className="settings-disclosure-body">{children}</div>
      </SettingsReveal>
    </div>
  );
}
