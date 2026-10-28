import { X } from "lucide-react";
import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useT } from "../../i18n";
import { Button, IconButton, type ButtonVariant } from "./Button";

interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg";
  /** Prevent closing via Escape/backdrop (e.g. while working). */
  locked?: boolean;
  role?: "dialog" | "alertdialog";
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function Modal({ open, onClose, title, children, footer, size = "md", locked, role = "dialog" }: ModalProps) {
  const t = useT();
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const previouslyFocused = document.activeElement as HTMLElement | null;
    const node = ref.current;
    const first = node?.querySelector<HTMLElement>("[autofocus], [data-autofocus]") ?? node?.querySelector<HTMLElement>(FOCUSABLE);
    first?.focus();
    return () => previouslyFocused?.focus?.();
  }, [open]);

  if (!open) return null;

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape" && !locked) {
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key === "Tab" && ref.current) {
      const items = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
      if (items.length === 0) return;
      const first = items[0]!;
      const last = items[items.length - 1]!;
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  };

  return createPortal(
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !locked && onClose()}>
      <div ref={ref} className={`modal modal-${size}`} role={role} aria-modal="true" aria-labelledby={titleId} onKeyDown={onKeyDown}>
        <header className="modal-head">
          <h2 id={titleId}>{title}</h2>
          <IconButton icon={X} label={t("common.close")} onClick={onClose} disabled={locked} />
        </header>
        <div className="modal-body">{children}</div>
        {footer && <footer className="modal-foot">{footer}</footer>}
      </div>
    </div>,
    document.body,
  );
}

interface ConfirmProps {
  open: boolean;
  title: ReactNode;
  children: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
  variant?: ButtonVariant;
  busy?: boolean;
}

export function ConfirmDialog({ open, title, children, confirmLabel, cancelLabel, onConfirm, onCancel, variant = "primary", busy }: ConfirmProps) {
  const t = useT();
  return (
    <Modal
      open={open}
      onClose={onCancel}
      title={title}
      size="sm"
      role="alertdialog"
      locked={busy}
      footer={
        <>
          <Button variant="ghost" onClick={onCancel} disabled={busy} data-autofocus>
            {cancelLabel ?? t("common.cancel")}
          </Button>
          <Button variant={variant} onClick={onConfirm} loading={busy}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Modal>
  );
}
