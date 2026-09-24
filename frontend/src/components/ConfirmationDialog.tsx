import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { TriangleAlert, X } from 'lucide-react';

interface ConfirmationItem {
  id: string;
  label: string;
  detail?: string;
  icon?: ReactNode;
}

export interface ConfirmationOptions {
  title: string;
  description: string;
  items?: ConfirmationItem[];
  warning?: string;
  confirmLabel?: string;
  danger?: boolean;
}

export function ConfirmationDialog({ title, description, items, warning, confirmLabel = '确认', danger = false, onResult }: ConfirmationOptions & {
  onResult: (confirmed: boolean) => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const backdropPointerRef = useRef(false);
  const id = useId();

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    // A modal dialog contains keyboard focus and makes the workspace inert,
    // including terminals that finish restoring while confirmation is open.
    dialog.showModal();
    cancelRef.current?.focus();
    return () => { dialog.close(); };
  }, []);

  const outsideDialog = (event: React.PointerEvent<HTMLDialogElement> | React.MouseEvent<HTMLDialogElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return event.target === event.currentTarget
      && (event.clientX < bounds.left || event.clientX > bounds.right
        || event.clientY < bounds.top || event.clientY > bounds.bottom);
  };

  return createPortal(
    <dialog
      ref={dialogRef}
      className="project-aow-modal confirmation-dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby={`${id}-title`}
      aria-describedby={`${id}-description${warning ? ` ${id}-warning` : ''}`}
      onCancel={(event) => { event.preventDefault(); onResult(false); }}
      onPointerDown={(event) => { backdropPointerRef.current = outsideDialog(event); event.stopPropagation(); }}
      onClick={(event) => {
        if (backdropPointerRef.current && outsideDialog(event)) onResult(false);
        backdropPointerRef.current = false;
        event.stopPropagation();
      }}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key !== 'Tab') return;
        const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
        const boundary = event.shiftKey ? buttons[0] : buttons.at(-1);
        if (document.activeElement === boundary) {
          event.preventDefault();
          (event.shiftKey ? buttons.at(-1) : buttons[0])?.focus();
        }
      }}
    >
      <header>
        <h2 id={`${id}-title`}>{title}</h2>
        <button type="button" title="关闭对话框" aria-label="关闭对话框" onClick={() => onResult(false)}><X /></button>
      </header>
      <div className="confirmation-body">
        <p id={`${id}-description`}>{description}</p>
        {items?.length ? <ul className="confirmation-items">
          {items.map((item) => <li key={item.id}>
            {item.icon}
            <div><strong>{item.label}</strong>{item.detail ? <span>{item.detail}</span> : null}</div>
          </li>)}
        </ul> : null}
        {warning ? <div className="confirmation-warning" id={`${id}-warning`}><TriangleAlert aria-hidden="true" /><p>{warning}</p></div> : null}
      </div>
      <footer>
        <button ref={cancelRef} type="button" onClick={() => onResult(false)}>取消</button>
        <button type="button" className={danger ? 'danger' : 'primary'} onClick={() => onResult(true)}>{confirmLabel}</button>
      </footer>
    </dialog>,
    document.body,
  );
}

export function useConfirmation() {
  const [options, setOptions] = useState<ConfirmationOptions>();
  const resolveRef = useRef<((confirmed: boolean) => void) | undefined>(undefined);

  const confirm = useCallback((next: ConfirmationOptions): Promise<boolean> => {
    if (resolveRef.current) return Promise.resolve(false);
    return new Promise((resolve) => {
      resolveRef.current = resolve;
      setOptions(next);
    });
  }, []);

  const onResult = useCallback((confirmed: boolean) => {
    const resolve = resolveRef.current;
    resolveRef.current = undefined;
    setOptions(undefined);
    resolve?.(confirmed);
  }, []);

  useEffect(() => () => {
    // Unmounting the owner cancels pending destructive actions.
    resolveRef.current?.(false);
    resolveRef.current = undefined;
  }, []);

  return { confirm, confirmationDialog: options ? <ConfirmationDialog {...options} onResult={onResult} /> : null };
}
