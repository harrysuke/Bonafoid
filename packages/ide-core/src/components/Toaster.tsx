import { useUIStore } from '../stores/uiStore';
import type { Notification } from '../stores/uiStore';
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from 'lucide-react';

const STYLES: Record<Notification['type'], { icon: typeof Info; className: string }> = {
  success: { icon: CheckCircle2, className: 'border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-200' },
  error: { icon: XCircle, className: 'border-red-200 bg-red-50 text-red-800 dark:border-red-800 dark:bg-red-950/60 dark:text-red-200' },
  warning: { icon: AlertTriangle, className: 'border-amber-200 bg-amber-50 text-amber-800 dark:border-amber-800 dark:bg-amber-950/60 dark:text-amber-200' },
  info: { icon: Info, className: 'border-blue-200 bg-blue-50 text-blue-800 dark:border-blue-800 dark:bg-blue-950/60 dark:text-blue-200' },
};

/**
 * Renders the notifications pushed into `uiStore`.
 *
 * The store has carried notifications since before anything rendered them, so
 * `addNotification` was a no-op as far as the user was concerned. Mounted once
 * in the layout shell.
 */
export function Toaster() {
  const notifications = useUIStore((s) => s.notifications);
  const removeNotification = useUIStore((s) => s.removeNotification);

  if (notifications.length === 0) return null;

  return (
    <div
      className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 w-80 max-w-[calc(100vw-2rem)]"
      role="region"
      aria-label="Notifications"
    >
      {notifications.map((notification) => {
        const style = STYLES[notification.type];
        const Icon = style.icon;
        return (
          <div
            key={notification.id}
            role="status"
            aria-live="polite"
            className={`flex items-start gap-3 rounded-lg border p-3 shadow-lg ${style.className}`}
          >
            <Icon className="w-5 h-5 shrink-0 mt-0.5" />
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium">{notification.title}</p>
              {notification.message && (
                <p className="text-xs mt-0.5 break-words opacity-90">{notification.message}</p>
              )}
            </div>
            <button
              type="button"
              aria-label="Dismiss notification"
              onClick={() => removeNotification(notification.id)}
              className="p-1 rounded hover:bg-black/5 dark:hover:bg-white/10"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        );
      })}
    </div>
  );
}