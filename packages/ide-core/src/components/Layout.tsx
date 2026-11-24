import { useEffect, useRef, useState } from 'react';
import { Outlet, NavLink, useLocation, useNavigate } from 'react-router-dom';
import {
  Menu, ChevronLeft, ChevronRight, Home,
  Database, GitBranch, ClipboardList, Palette,
  Puzzle, Code, Settings, LogOut, Sun, Moon, ChevronDown,
  FilePlus, FolderOpen, Save, Table2, Monitor, Rocket,
  Building2, CreditCard, ShieldCheck, PlugZap, GitCommitHorizontal,
  LayoutTemplate
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { getPlatform } from '../platform/registry';
import { useUIStore } from '../stores/uiStore';
import { useProjectStore } from '../stores/projectStore';
import { confirmLeave, useUnsavedChangesGuard } from '../hooks/useUnsavedChangesGuard';
import { Toaster } from './Toaster';
import { SidebarUserDropdown } from './SidebarUserDropdown';
import brandLogo from '../assets/brand/android-chrome-512x512.png';

type PanelId = 'dashboard' | 'forms' | 'workflows' | 'data' | 'lists' | 'preview' | 'pages' | 'theme' | 'plugins' | 'generate' | 'deploy' | 'tenants' | 'billing' | 'admin' | 'connectors' | 'version' | 'settings';

interface NavItem {
  name: string;
  href: string;
  icon: LucideIcon;
  panel: PanelId;
}

interface NavGroup {
  id: string;
  label: string;
  items: NavItem[];
}

// One source of truth for both grouping and order. Sections run from the
// everyday build loop down to infrequent account administration.
const dashboardItem: NavItem = {
  name: 'Dashboard', href: '/', icon: Home, panel: 'dashboard',
};

const navGroups: NavGroup[] = [
  {
    id: 'build',
    label: 'Build',
    items: [
      { name: 'Forms', href: '/forms', icon: ClipboardList, panel: 'forms' },
      { name: 'Workflows', href: '/workflows', icon: GitBranch, panel: 'workflows' },
      { name: 'Data Model', href: '/data', icon: Database, panel: 'data' },
      { name: 'Pages', href: '/pages', icon: LayoutTemplate, panel: 'pages' },
      { name: 'Lists', href: '/lists', icon: Table2, panel: 'lists' },
      { name: 'Theme', href: '/theme', icon: Palette, panel: 'theme' },
    ],
  },
  {
    id: 'release',
    label: 'Release',
    items: [
      { name: 'Preview', href: '/preview', icon: Monitor, panel: 'preview' },
      { name: 'Generate', href: '/generate', icon: Code, panel: 'generate' },
      { name: 'Deploy', href: '/deploy', icon: Rocket, panel: 'deploy' },
      { name: 'Version', href: '/version', icon: GitCommitHorizontal, panel: 'version' },
    ],
  },
  {
    id: 'integrations',
    label: 'Integrations',
    items: [
      { name: 'Plugins', href: '/plugins', icon: Puzzle, panel: 'plugins' },
      { name: 'Connectors', href: '/connectors', icon: PlugZap, panel: 'connectors' },
    ],
  },
];

// Account-level destinations live in the user menu rather than the sidebar.
// The same list drives both the header menu and the sidebar footer menu.
const accountNavItems: NavItem[] = [
  { name: 'Workspaces', href: '/tenants', icon: Building2, panel: 'tenants' },
  { name: 'Billing', href: '/billing', icon: CreditCard, panel: 'billing' },
  { name: 'Admin', href: '/admin', icon: ShieldCheck, panel: 'admin' },
  { name: 'Settings', href: '/settings', icon: Settings, panel: 'settings' },
];

const allNavItems = [
  dashboardItem,
  ...navGroups.flatMap((group) => group.items),
  ...accountNavItems,
];

// Detail routes such as /forms/:id should keep their parent's label instead of
// falling back to "Dashboard".
function getPageTitle(pathname: string): string {
  const exact = allNavItems.find((item) => item.href === pathname);
  if (exact) return exact.name;
  const parent = allNavItems
    .filter((item) => item.href !== '/' && pathname.startsWith(`${item.href}/`))
    .sort((a, b) => b.href.length - a.href.length)[0];
  return parent?.name ?? 'Dashboard';
}

interface NavItemLinkProps {
  item: NavItem;
  collapsed: boolean;
  onNavigate: (item: NavItem) => void;
}

function NavItemLink({ item, collapsed, onNavigate }: NavItemLinkProps) {
  return (
    <NavLink
      to={item.href}
      className={({ isActive }) => `
        flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors
        ${isActive
          ? 'bg-primary-50 text-primary-700 dark:bg-primary-900/30 dark:text-primary-400'
          : 'text-gray-600 hover:bg-gray-100 hover:text-gray-900 dark:text-gray-400 dark:hover:bg-slate-700 dark:hover:text-white'
        }
        ${collapsed ? 'justify-center' : ''}
      `}
      title={collapsed ? item.name : undefined}
      onClick={() => onNavigate(item)}
    >
      <item.icon className="w-5 h-5 flex-shrink-0" />
      {!collapsed && <span>{item.name}</span>}
    </NavLink>
  );
}

interface NavGroupSectionProps {
  group: NavGroup;
  collapsed: boolean;
  closed: boolean;
  onToggle: (id: string) => void;
  onNavigate: (item: NavItem) => void;
}

function NavGroupSection({ group, collapsed, closed, onToggle, onNavigate }: NavGroupSectionProps) {
  const { pathname } = useLocation();
  // Never hide the section containing the active route (for example a detail
  // page such as /forms/:id) just because the user collapsed it earlier.
  const hasActiveItem = group.items.some((item) =>
    pathname === item.href || pathname.startsWith(`${item.href}/`)
  );
  // In rail mode there is no room for a section label: show a divider instead
  // and keep every icon reachable through its tooltip.
  const isOpen = collapsed || !closed || hasActiveItem;

  return (
    <div>
      {collapsed ? (
        <div className="my-2 border-t border-gray-200 dark:border-slate-700" aria-hidden="true" />
      ) : (
        <button
          type="button"
          onClick={() => onToggle(group.id)}
          aria-expanded={isOpen}
          className="w-full flex items-center justify-between px-3 pb-1 pt-3 text-[11px] font-semibold uppercase tracking-wider text-gray-400 hover:text-gray-600 dark:text-gray-500 dark:hover:text-gray-300"
        >
          <span>{group.label}</span>
          <ChevronDown className={`w-3.5 h-3.5 transition-transform ${isOpen ? '' : '-rotate-90'}`} />
        </button>
      )}
      {isOpen && (
        <div className="space-y-1">
          {group.items.map((item) => (
            <NavItemLink key={item.name} item={item} collapsed={collapsed} onNavigate={onNavigate} />
          ))}
        </div>
      )}
    </div>
  );
}

export function Layout() {
  const { 
    sidebarOpen, 
    sidebarCollapsed, 
    theme, 
    setSidebarOpen, 
    setSidebarCollapsed, 
    setTheme,
    setActivePanel,
    collapsedNavGroups,
    toggleNavGroup
  } = useUIStore();
  
  const location = useLocation();
  const navigate = useNavigate();
  // Warn on tab close while the project is dirty. Sign-out is guarded at its
  // own handlers below, because that is a deliberate navigation.
  useUnsavedChangesGuard();
  const [showUserMenu, setShowUserMenu] = useState(false);
  const [showSidebarUserMenu, setShowSidebarUserMenu] = useState(false);

  const {
    project,
    projectPath,
    isDirty,
    newProject,
    openProject,
    saveProject,
  } = useProjectStore();

  const handleNew = async () => {
    const name = window.prompt('Project name', 'My App');
    if (name) await newProject(name);
  };

  // The account menu shows a real identity where the host has one (web) and
  // says so plainly where it does not (desktop).
  const platform = getPlatform();
  const user = platform.currentUser?.() ?? null;
  const identity = user?.email ?? null;
  const initial = (identity ?? '?').charAt(0).toUpperCase();

  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark');
  }, [theme]);

  // Close the account menu on an outside click. This used to be an injected
  // <script> looking for an id ("user-dropdown") that no element had, so the
  // menu stayed open until the button was clicked again.
  const userMenuRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!showUserMenu) return;
    const onPointerDown = (event: MouseEvent) => {
      if (userMenuRef.current && !userMenuRef.current.contains(event.target as Node)) {
        setShowUserMenu(false);
      }
    };
    document.addEventListener('mousedown', onPointerDown);
    return () => document.removeEventListener('mousedown', onPointerDown);
  }, [showUserMenu]);

  const handleNavigate = (item: NavItem) => {
    setSidebarOpen(false);
    setShowUserMenu(false);
    setShowSidebarUserMenu(false);
    setActivePanel(item.panel);
  };

  // Dropdown items are buttons rather than NavLinks, so they navigate explicitly.
  const handleAccountNavigate = (item: NavItem) => {
    handleNavigate(item);
    navigate(item.href);
  };

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-slate-900 transition-colors flex flex-row">
      {/* Mobile sidebar backdrop */}
      {sidebarOpen && (
        <div
          className="fixed inset-0 z-40 bg-gray-600 bg-opacity-75 lg:hidden"
          onClick={() => setSidebarOpen(false)}
        />
      )}

      {/* Sidebar */}
      <aside
        className={`fixed inset-y-0 left-0 z-50 flex flex-col bg-white dark:bg-slate-800 shadow-lg transition-all duration-300 ease-in-out lg:relative ${
          sidebarCollapsed ? 'w-16' : 'w-64'
        } ${sidebarOpen ? 'translate-x-0' : '-translate-x-full lg:translate-x-0'}`}
      >
        <div className="flex h-16 items-center justify-between px-4 border-b border-gray-200 dark:border-slate-700">
          {!sidebarCollapsed && (
            <h1 className="text-xl font-bold text-gray-900 dark:text-white flex items-center gap-2">
              <img src={brandLogo} alt="" className="w-6 h-6 rounded" />
              Bonafoid
            </h1>
          )}
          <button
            onClick={() => setSidebarCollapsed(!sidebarCollapsed)}
            className="p-2 rounded-lg text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700"
            aria-label={sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          >
            {sidebarCollapsed ? <ChevronRight className="w-5 h-5" /> : <ChevronLeft className="w-5 h-5" />}
          </button>
        </div>

        <nav className="flex-1 px-2 py-2 overflow-y-auto flex flex-col">
          <NavItemLink item={dashboardItem} collapsed={sidebarCollapsed} onNavigate={handleNavigate} />

          {navGroups.map((group) => (
            <NavGroupSection
              key={group.id}
              group={group}
              collapsed={sidebarCollapsed}
              closed={collapsedNavGroups.includes(group.id)}
              onToggle={toggleNavGroup}
              onNavigate={handleNavigate}
            />
          ))}
        </nav>

        <div className="p-4 border-t border-gray-200 dark:border-slate-700">
          <div className="flex items-center justify-between p-2 rounded-lg hover:bg-gray-100 dark:hover:bg-slate-700">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-full bg-primary-100 dark:bg-primary-900 flex items-center justify-center">
                <span className="text-sm font-medium text-primary-700 dark:text-primary-300">{initial}</span>
              </div>
              {!sidebarCollapsed && (
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-gray-900 dark:text-white truncate">{identity ?? 'Local project'}</p>
                  <p className="text-xs text-gray-500 dark:text-gray-400 truncate">{user ? 'Signed in' : 'Local IDE'}</p>
                </div>
              )}
            </div>
            <div className="relative">
              <button
                onClick={() => setShowSidebarUserMenu(!showSidebarUserMenu)}
                className="p-1 rounded text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
                aria-label="Open sidebar user menu"
              >
                <ChevronDown className="w-4 h-4" />
              </button>
              {showSidebarUserMenu && (
                <div className="absolute left-0 bottom-full mb-2 z-[70]">
                  <SidebarUserDropdown
                    user={user ? { email: user.email } : undefined}
                    items={accountNavItems.map((item) => ({
                      name: item.name,
                      icon: item.icon,
                      active: location.pathname === item.href,
                      onClick: () => handleAccountNavigate(item),
                    }))}
                    onLogout={platform.logout ? () => { setShowSidebarUserMenu(false); if (!confirmLeave()) return; void platform.logout?.().then(() => navigate('/login')); } : undefined}
                  />
                </div>
              )}
            </div>
          </div>
        </div>
      </aside>

      {/* Main content */}
      <div className={`flex-1 transition-all duration-300 min-w-0 ${sidebarCollapsed ? 'lg:pl-2' : 'lg:pl-2'}`} style={{ minWidth: 0 }}>
        {/* Top header */}
        <header className="sticky top-0 z-30 bg-white dark:bg-slate-800 shadow-sm border-b border-gray-200 dark:border-slate-700">
          <div className="flex items-center justify-between h-16 px-4 sm:px-6">
            <div className="flex items-center gap-4">
              <button
                className="lg:hidden p-2 rounded-lg text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700"
                onClick={() => setSidebarOpen(true)}
              >
                <Menu className="w-6 h-6" />
              </button>
              <h2 className="text-lg font-semibold text-gray-900 dark:text-white">
                {getPageTitle(location.pathname)}
              </h2>
            </div>

            <div className="flex items-center gap-2">
              <div className="hidden md:flex flex-col items-end mr-2">
                <span className="text-sm font-medium text-gray-900 dark:text-white">
                  {project?.name ?? 'No project'}
                  {isDirty && <span className="text-primary-600"> *</span>}
                </span>
                <span className="text-xs text-gray-400 truncate max-w-[240px]">
                  {projectPath ?? 'unsaved'}
                </span>
              </div>
              <button
                onClick={handleNew}
                title="New project"
                className="p-2 rounded-lg text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700"
              >
                <FilePlus className="w-5 h-5" />
              </button>
              <button
                onClick={() => void openProject()}
                title="Open project"
                className="p-2 rounded-lg text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700"
              >
                <FolderOpen className="w-5 h-5" />
              </button>
              <button
                onClick={() => void saveProject()}
                title="Save project"
                className="p-2 rounded-lg text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700"
              >
                <Save className="w-5 h-5" />
              </button>
            </div>

            <div className="flex items-center gap-4">
              <button
                onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}
                className="p-2 rounded-lg text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700"
              >
                {theme === 'light' ? <Moon className="w-5 h-5" /> : <Sun className="w-5 h-5" />}
              </button>
              
              <div className="relative" id="user-menu" ref={userMenuRef}>
                <button
                  className="flex items-center gap-2 p-2 rounded-lg text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700"
                  onClick={(e) => {
                    e.stopPropagation();
                    setShowUserMenu(!showUserMenu);
                  }}
                >
                  <div className="w-8 h-8 rounded-full bg-primary-100 dark:bg-primary-900 flex items-center justify-center">
                    <span className="text-sm font-medium text-primary-700 dark:text-primary-300">
                      {initial}
                    </span>
                  </div>
                  <span className="hidden sm:block text-sm font-medium text-gray-700 dark:text-gray-300">
                    {identity ?? 'Local project'}
                  </span>
                  <ChevronDown className="w-4 h-4" />
                </button>

                {showUserMenu && (
                  <div className="absolute right-0 mt-2 w-48 bg-white dark:bg-slate-800 rounded-lg shadow-lg border border-gray-200 dark:border-slate-700 py-1 z-[60]">
                    <div className="px-4 py-2 border-b border-gray-100 dark:border-slate-700">
                      <p className="text-sm font-medium text-gray-900 dark:text-white">
                        {user?.email ?? 'Not signed in'}
                      </p>
                    </div>
                    {accountNavItems.map((item) => (
                      <button
                        key={item.name}
                        type="button"
                        className={`w-full text-left px-4 py-2 text-sm flex items-center gap-2 hover:bg-gray-100 dark:hover:bg-slate-700 ${
                          location.pathname === item.href
                            ? 'bg-primary-50 text-primary-700 dark:bg-primary-900/30 dark:text-primary-400'
                            : 'text-gray-700 dark:text-gray-300'
                        }`}
                        onClick={() => handleAccountNavigate(item)}
                      >
                        <item.icon className="w-4 h-4" /> {item.name}
                      </button>
                    ))}
                    {platform.logout && (
                      <button
                        type="button"
                        className="w-full text-left px-4 py-2 text-sm text-red-600 dark:text-red-400 hover:bg-gray-100 dark:hover:bg-slate-700 flex items-center gap-2"
                        onClick={() => {
                          setShowUserMenu(false);
                          if (!confirmLeave()) return;
                          void platform.logout?.().then(() => navigate('/login'));
                        }}
                      >
                        <LogOut className="w-4 h-4" /> Logout
                      </button>
                    )}
                  </div>
                )}
              </div>
            </div>
          </div>
        </header>

        {/* Page content */}
        <main className="flex-1 p-4 sm:p-6 lg:p-8 min-w-0">
          <Outlet />
        </main>
      </div>

      <Toaster />
    </div>
  );
}

