"use client";

// Material's dialog, on top of the platform's own `<dialog>`. The native element hands
// us the top layer, the backdrop, focus trapping, restoring focus on close and Escape —
// all of which a hand-rolled modal gets subtly wrong, usually for keyboard users.

import { useEffect, useRef, type ReactNode } from "react";

import { Button, Icon } from "./ui";

export function Dialog({
  open,
  onClose,
  icon,
  title,
  children,
  actions,
}: {
  open: boolean;
  onClose: () => void;
  icon?: string;
  title: ReactNode;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open && !el.open) el.showModal();
    if (!open && el.open) el.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      // Escape fires `cancel`; routing it through our own state keeps React the one
      // source of truth for whether the dialog is open.
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      // A click that lands on the dialog element itself landed on the backdrop: the
      // content is in a child, so it never targets this node.
      onClick={(e) => {
        if (e.target === ref.current) onClose();
      }}
      className="m-auto w-[min(32rem,calc(100vw-2rem))] rounded-xl bg-surface-container-high p-0 text-on-surface shadow-e4 backdrop:bg-scrim"
    >
      <div className="p-6">
        {icon && (
          <Icon
            name={icon}
            className="mb-4 block text-center text-[24px] text-on-surface-variant"
          />
        )}
        <h2 className={`text-2xl leading-8 text-on-surface ${icon ? "text-center" : ""}`}>
          {title}
        </h2>
        {children && (
          <div
            className={`mt-4 text-sm leading-relaxed text-on-surface-variant ${icon ? "text-center" : ""}`}
          >
            {children}
          </div>
        )}
        {actions && <div className="mt-6 flex flex-wrap justify-end gap-2">{actions}</div>}
      </div>
    </dialog>
  );
}

/**
 * The shape almost every dialog here takes: say what will happen, and let them go
 * through with it or back out. `danger` is for the ones that can't be undone.
 */
export function ConfirmDialog({
  open,
  onClose,
  onConfirm,
  title,
  confirmLabel,
  busy = false,
  danger = false,
  children,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: ReactNode;
  confirmLabel: string;
  busy?: boolean;
  danger?: boolean;
  children?: ReactNode;
}) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      icon={danger ? "warning" : undefined}
      title={title}
      actions={
        <>
          <Button tone="text" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button
            tone={danger ? "danger-filled" : "primary"}
            disabled={busy}
            onClick={onConfirm}
            autoFocus
          >
            {busy ? "Working…" : confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Dialog>
  );
}
