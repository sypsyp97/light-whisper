import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import IconButton from "./IconButton";

export interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  "data-testid"?: string;
}

export function Modal({ open, onClose, title, children, "data-testid": testId }: ModalProps) {
  const { t } = useTranslation();
  const modalRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  useEffect(() => {
    if (open) {
      const el = modalRef.current?.querySelector<HTMLElement>("button, input, textarea, select, [tabindex]");
      el?.focus();
    }
  }, [open]);

  if (!open || typeof document === "undefined") return null;

  return createPortal(
    <div
      className="lw-modal-overlay"
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div
        ref={modalRef}
        className="lw-modal"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid={testId}
      >
        <div className="lw-modal-header">
          <h2 className="lw-modal-title">{title}</h2>
          <IconButton label={t("common.close")} icon={<X size={14} />} onClick={onClose} />
        </div>
        <div className="lw-modal-body">{children}</div>
      </div>
    </div>,
    document.body,
  );
}

export default Modal;
