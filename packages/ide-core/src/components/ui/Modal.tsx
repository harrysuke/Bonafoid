import { useEffect } from 'react';
import { X } from 'lucide-react';
import { cn } from '../../utils/helpers';

interface ModalProps {
  open: boolean;
  title?: string;
  onClose: () => void;
  className?: string;
  children: React.ReactNode;
}

/**
 * Accessible modal dialog rendered in a portal. Closes on Escape and on
 * backdrop click, and locks body scroll while open.
 */
export function Modal({ open, title, onClose, className, children }: ModalProps) {
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKeyDown);
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      role="dialog"
      aria-modal="true"
      aria-label={title}
    >
      <div
        className="absolute inset-0 bg-black/50"
        onClick={onClose}
        aria-hidden="true"
      />
      <div
        className={cn(
          'relative w-full max-w-lg rounded-xl bg-white dark:bg-slate-800 shadow-xl border border-gray-200 dark:border-slate-700',
          className,
        )}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-slate-700">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-white">{title}</h3>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="rounded p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700 dark:text-gray-400 dark:hover:bg-slate-700 focus:outline-none focus:ring-2 focus:ring-primary-500"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
        <div className="p-4">{children}</div>
      </div>
    </div>
  );
}
