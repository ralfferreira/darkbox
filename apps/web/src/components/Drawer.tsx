import { useId, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { useDialogFocus } from '../hooks/useDialogFocus';

interface DrawerProps {
  open: boolean;
  onClose: () => void;
  title: string;
  subtitle?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
}

export function Drawer({ open, onClose, title, subtitle, children, footer }: DrawerProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  useDialogFocus(open, onClose, panelRef);

  if (!open) return null;

  return createPortal(
    <div
      className="overlay drawer-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={panelRef}
        className="drawer"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <div className="drawer-head">
          <div className="drawer-heading">
            <h2 id={titleId}>{title}</h2>
            {subtitle ? <div className="drawer-sub">{subtitle}</div> : null}
          </div>
          <button
            type="button"
            className="btn btn-icon btn-ghost"
            onClick={onClose}
            aria-label="Close panel"
          >
            <X size={16} strokeWidth={1.5} />
          </button>
        </div>
        <div className="drawer-body">{children}</div>
        {footer ? <div className="drawer-foot">{footer}</div> : null}
      </div>
    </div>,
    document.body,
  );
}
