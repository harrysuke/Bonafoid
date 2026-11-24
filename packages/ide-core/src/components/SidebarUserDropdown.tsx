import type { LucideIcon } from 'lucide-react';

/** Separate user dropdown for the sidebar (not the navigation bar header). */
export function SidebarUserDropdown({ user, items, onLogout }: {
  user?: { email?: string };
  items: { name: string; icon: LucideIcon; active?: boolean; onClick: () => void }[];
  onLogout?: () => void;
}) {
  return (
    <div className="absolute bottom-full left-0 mb-2 w-52 bg-white dark:bg-slate-800 rounded-lg shadow-xl border border-gray-200 dark:border-slate-700 py-1 z-[70]">
      <div className="px-4 py-2 border-b border-gray-100 dark:border-slate-700">
        <p className="text-sm font-medium text-gray-900 dark:text-white">{user?.email ?? 'Not signed in'}</p>
      </div>
      {items.map(({ name, icon: Icon, active, onClick }) => (
        <button
          key={name}
          onClick={onClick}
          className={`w-full text-left px-4 py-2 text-sm flex items-center gap-2 hover:bg-gray-100 dark:hover:bg-slate-700 ${
            active
              ? 'bg-primary-50 text-primary-700 dark:bg-primary-900/30 dark:text-primary-400'
              : 'text-gray-700 dark:text-gray-300'
          }`}
        >
          <Icon className="w-4 h-4" /> {name}
        </button>
      ))}
      {onLogout && (
        <button
          onClick={onLogout}
          className="w-full text-left px-4 py-2 text-sm text-red-600 dark:text-red-400 hover:bg-gray-100 dark:hover:bg-slate-700 flex items-center gap-2 border-t border-gray-100 dark:border-slate-700"
        >
          <span className="w-4 h-4" aria-hidden="true" />
          Logout
        </button>
      )}
    </div>
  );
}
