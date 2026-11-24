import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface UIState {
  sidebarOpen: boolean;
  sidebarCollapsed: boolean;
  theme: 'light' | 'dark';
  activePanel: 'dashboard' | 'forms' | 'workflows' | 'data' | 'lists' | 'preview' | 'pages' | 'theme' | 'plugins' | 'generate' | 'deploy' | 'tenants' | 'billing' | 'admin' | 'connectors' | 'version' | 'settings';
  // Navigation sections the user has manually collapsed. All sections start open.
  collapsedNavGroups: string[];
  notifications: Notification[];
  setSidebarOpen: (open: boolean) => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  toggleSidebar: () => void;
  setTheme: (theme: 'light' | 'dark') => void;
  setActivePanel: (panel: UIState['activePanel']) => void;
  toggleNavGroup: (id: string) => void;
  addNotification: (notification: Omit<Notification, 'id' | 'timestamp'>) => void;
  removeNotification: (id: string) => void;
}

export interface Notification {
  id: string;
  type: 'success' | 'error' | 'warning' | 'info';
  title: string;
  message?: string;
  duration?: number;
  timestamp: number;
}

export const useUIStore = create<UIState>()(
  persist(
    (set, get) => ({
      sidebarOpen: false,
      sidebarCollapsed: false,
      theme: 'light',
      activePanel: 'forms',
      collapsedNavGroups: [],
      notifications: [],
      
      setSidebarOpen: (open) => set({ sidebarOpen: open }),
      setSidebarCollapsed: (collapsed) => set({ sidebarCollapsed: collapsed }),
      toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),
      setTheme: (theme) => {
        set({ theme });
        document.documentElement.classList.toggle('dark', theme === 'dark');
      },
      setActivePanel: (panel) => set({ activePanel: panel }),
      toggleNavGroup: (id) => set((state) => ({
        collapsedNavGroups: state.collapsedNavGroups.includes(id)
          ? state.collapsedNavGroups.filter((groupId) => groupId !== id)
          : [...state.collapsedNavGroups, id],
      })),
      
      addNotification: (notification) => {
        const id = crypto.randomUUID();
        const newNotification: Notification = {
          ...notification,
          id,
          timestamp: Date.now(),
        };
        set((state) => ({ notifications: [...state.notifications, newNotification] }));
        
        if (notification.duration !== 0) {
          setTimeout(() => {
            get().removeNotification(id);
          }, notification.duration || 5000);
        }
      },
      
      removeNotification: (id) => set((state) => ({
        notifications: state.notifications.filter((n) => n.id !== id),
      })),
    }),
    {
      name: 'ui-storage',
      partialize: (state) => ({ 
        sidebarCollapsed: state.sidebarCollapsed, 
        theme: state.theme,
        activePanel: state.activePanel,
        collapsedNavGroups: state.collapsedNavGroups,
      }),
    }
  )
);