/** Separate user dropdown for the sidebar (not the navigation bar header). */
export function SidebarUserDropdown({ user, onSettings, onLogout }: {
  user?: { email?: string };
  identity?: string;
  onSettings: () => void;
  onLogout?: () => void;
}) {
  return (
    <div className="absolute bottom-full left-0 mb-2 w-48 bg-white dark:bg-slate-800 rounded-lg shadow-xl border border-gray-200 dark:border-slate-700 py-1 z-[70]">
      <div className="px-4 py-2 border-b border-gray-100 dark:border-slate-700">
        <p className="text-sm font-medium text-gray-900 dark:text-white">{user?.email ?? 'Not signed in'}</p>
      </div>
      <button
        onClick={() => { onSettings(); }}
        className="w-full text-left px-4 py-2 text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 flex items-center gap-2"
      >
        Settings
      </button>
      {onLogout && (
        <button
          onClick={() => { onLogout(); }}
          className="w-full text-left px-4 py-2 text-sm text-red-600 dark:text-red-400 hover:bg-gray-100 dark:hover:bg-slate-700 flex items-center gap-2"
        >
          Logout
        </button>
      )}
    </div>
  );
}
