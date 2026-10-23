import { useEffect, useRef, useState } from 'react';
import { Outlet, NavLink, useLocation, useNavigate } from 'react-router-dom';
import {
  Menu, ChevronLeft, ChevronRight, Home,
  Database, GitBranch, FileText, Palette,
  Puzzle, Code, Settings, LogOut, Sun, Moon, ChevronDown,
  FilePlus, FolderOpen, Save, Table2, Monitor
} from 'lucide-react';
import { getPlatform } from '../platform/registry';
import { useUIStore } from '../stores/uiStore';
import { useProjectStore } from '../stores/projectStore';
import { Toaster } from './Toaster';
import { SidebarUserDropdown } from './SidebarUserDropdown';
import brandLogo from '../assets/brand/android-chrome-512x512.png';

const navigation = [
  { name: 'Dashboard', href: '/', icon: Home },
  { name: 'Forms', href: '/forms', icon: FileText },
  { name: 'Workflows', href: '/workflows', icon: GitBranch },
  { name: 'Data Model', href: '/data', icon: Database },
  { name: 'Lists', href: '/lists', icon: Table2 },
  { name: 'Pages', href: '/pages', icon: FileText },
  { name: 'Theme', href: '/theme', icon: Palette },
  { name: 'Plugins', href: '/plugins', icon: Puzzle },
  { name: 'Preview', href: '/preview', icon: Monitor },
  { name: 'Generate', href: '/generate', icon: Code },
  { name: 'Settings', href: '/settings', icon: Settings },
];

export function Layout() {
  const { 
    sidebarOpen, 
    sidebarCollapsed, 
    theme, 
    setSidebarOpen, 
    setSidebarCollapsed, 
    setTheme,
    setActivePanel
  } = useUIStore();
  
  const location = useLocation();
  const navigate = useNavigate();
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

  const handleGenerate = () => {
    setActivePanel('generate');
    navigate('/generate');
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
        className={`fixed inset-y-0 left-0 z-50 bg-white dark:bg-slate-800 shadow-lg transition-all duration-300 ease-in-out lg:relative ${
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

        <nav className="flex-1 px-2 py-4 space-y-1 overflow-y-auto">
          {navigation.map((item) => {
          
            
            return (
              <NavLink
                key={item.name}
                to={item.href}
                className={({ isActive }) => `
                  flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors
                  ${isActive
                    ? 'bg-primary-50 text-primary-700 dark:bg-primary-900/30 dark:text-primary-400'
                    : 'text-gray-600 hover:bg-gray-100 hover:text-gray-900 dark:text-gray-400 dark:hover:bg-slate-700 dark:hover:text-white'
                  }
                  ${sidebarCollapsed ? 'justify-center' : ''}
                `}
                title={sidebarCollapsed ? item.name : undefined}
                onClick={() => {
                  setSidebarOpen(false);
                  const panel = item.href === '/' ? 'dashboard' : item.href.replace('/', '') as 'forms' | 'workflows' | 'data' | 'lists' | 'preview' | 'pages' | 'theme' | 'plugins' | 'generate' | 'settings';
          setActivePanel(panel);
                }}
              >
                <item.icon className="w-5 h-5 flex-shrink-0" />
                {!sidebarCollapsed && <span>{item.name}</span>}
              </NavLink>
            );
          })}
        </nav>

        <div className="p-4 border-t border-gray-200 dark:border-slate-700">
          <button
            onClick={handleGenerate}
            className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-primary-600 text-white rounded-lg hover:bg-primary-700 transition-colors"
            disabled={sidebarCollapsed}
          >
            <Code className="w-5 h-5" />
            {!sidebarCollapsed && <span>Generate Code</span>}
          </button>
          
          <div className="mt-4 flex items-center justify-between p-2 rounded-lg hover:bg-gray-100 dark:hover:bg-slate-700">
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-full bg-primary-100 dark:bg-primary-900 flex items-center justify-center">
                <span className="text-sm font-medium text-primary-700 dark:text-primary-300">U</span>
              </div>
              {!sidebarCollapsed && (
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-gray-900 dark:text-white truncate">User</p>
                  <p className="text-xs text-gray-500 dark:text-gray-400 truncate">Developer</p>
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
                    onSettings={() => { setShowSidebarUserMenu(false); navigate('/settings'); }}
                    onLogout={platform.logout ? () => { setShowSidebarUserMenu(false); void platform.logout?.().then(() => navigate('/login')); } : undefined}
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
                {navigation.find(n => n.href === location.pathname)?.name || 'Dashboard'}
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
                    <button
                      type="button"
                      className="w-full text-left px-4 py-2 text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700 flex items-center gap-2"
                      onClick={() => {
                        setShowUserMenu(false);
                        navigate('/settings');
                      }}
                    >
                      <Settings className="w-4 h-4" /> Settings
                    </button>
                    {platform.logout && (
                      <button
                        type="button"
                        className="w-full text-left px-4 py-2 text-sm text-red-600 dark:text-red-400 hover:bg-gray-100 dark:hover:bg-slate-700 flex items-center gap-2"
                        onClick={() => {
                          setShowUserMenu(false);
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

