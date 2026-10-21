import { create } from 'zustand';
import { getPlatform } from '../platform/registry';
import type { PlatformPlugin } from '../platform/types';

/** State of the plugin panel. */
export interface PluginState {
  /** The plugins the user has installed. */
  plugins: PlatformPlugin[];
  /** A request is in flight. */
  busy: boolean;
  /** The last error, if any. */
  error: string | null;
  /** Load the installed plugins. */
  loadPlugins: () => Promise<void>;
  /** Install a plugin from its manifest text and WebAssembly bytes. */
  installPlugin: (manifest: string, module: Uint8Array) => Promise<void>;
  /** Turn a plugin on or off. */
  setPluginEnabled: (id: string, enabled: boolean) => Promise<void>;
  /** Uninstall a plugin. */
  uninstallPlugin: (id: string) => Promise<void>;
  /** Forget the last error. */
  clearError: () => void;
}

/** Uniform error text for host/API failures. */
function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The installed plugins.
 *
 * A plugin is a WebAssembly module with a manifest. The IDE server owns the
 * sandbox and the storage; this slice is the view of it.
 */
export const usePluginStore = create<PluginState>()((set, get) => ({
  plugins: [],
  busy: false,
  error: null,

  loadPlugins: async () => {
    const platform = getPlatform();
    if (!platform.pluginList) {
      set({ plugins: [], error: 'This host cannot manage plugins.' });
      return;
    }
    set({ busy: true, error: null });
    try {
      const plugins = await platform.pluginList();
      set({ plugins, busy: false });
    } catch (error) {
      set({ busy: false, error: describeError(error) });
    }
  },

  installPlugin: async (manifest, module) => {
    const platform = getPlatform();
    if (!platform.pluginInstall) {
      set({ error: 'This host cannot install plugins.' });
      return;
    }
    set({ busy: true, error: null });
    try {
      const installed = await platform.pluginInstall(manifest, module);
      set({
        plugins: [...get().plugins.filter((p) => p.id !== installed.id), installed],
        busy: false,
      });
    } catch (error) {
      set({ busy: false, error: describeError(error) });
    }
  },

  setPluginEnabled: async (id, enabled) => {
    const platform = getPlatform();
    if (!platform.pluginSetEnabled) {
      set({ error: 'This host cannot manage plugins.' });
      return;
    }
    set({ busy: true, error: null });
    try {
      const updated = await platform.pluginSetEnabled(id, enabled);
      set({
        plugins: get().plugins.map((plugin) =>
          plugin.id === updated.id ? updated : plugin,
        ),
        busy: false,
      });
    } catch (error) {
      set({ busy: false, error: describeError(error) });
    }
  },

  uninstallPlugin: async (id) => {
    const platform = getPlatform();
    if (!platform.pluginUninstall) {
      set({ error: 'This host cannot manage plugins.' });
      return;
    }
    set({ busy: true, error: null });
    try {
      await platform.pluginUninstall(id);
      set({
        plugins: get().plugins.filter((plugin) => plugin.id !== id),
        busy: false,
      });
    } catch (error) {
      set({ busy: false, error: describeError(error) });
    }
  },

  clearError: () => set({ error: null }),
}));
