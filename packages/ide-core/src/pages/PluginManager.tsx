import { useEffect, useRef, useState } from 'react';
import { Puzzle, Trash2, Upload, RefreshCw, Shield } from 'lucide-react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';
import { getPlatform } from '../platform/registry';
import { usePluginStore } from '../stores/pluginStore';
import { useUIStore } from '../stores/uiStore';
import type { PlatformPlugin } from '../platform/types';

/**
 * The installed plugins.
 *
 * A plugin is a WebAssembly module with a `plugin.json` manifest, and the
 * IDE server owns the sandbox it runs in. There is no marketplace: a plugin
 * is installed from a manifest and a module the user already has.
 */
export function PluginManager() {
  const { plugins, busy, error, loadPlugins, installPlugin, setPluginEnabled, uninstallPlugin } =
    usePluginStore();
  const addNotification = useUIStore((state) => state.addNotification);

  // The plugin sandbox lives in the IDE server, so a host without one
  // cannot manage plugins. Say so rather than offering a form that
  // cannot work.
  const supported = typeof getPlatform().pluginInstall === 'function';

  const [search, setSearch] = useState('');
  const [manifestFile, setManifestFile] = useState<File | null>(null);
  const [moduleFile, setModuleFile] = useState<File | null>(null);
  const manifestInput = useRef<HTMLInputElement>(null);
  const moduleInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (supported) void loadPlugins();
  }, [loadPlugins, supported]);

  const visible = plugins.filter((plugin) => {
    const needle = search.trim().toLowerCase();
    if (!needle) return true;
    return (
      plugin.name.toLowerCase().includes(needle) ||
      plugin.id.toLowerCase().includes(needle) ||
      plugin.description?.toLowerCase().includes(needle)
    );
  });

  const install = async () => {
    if (!manifestFile || !moduleFile) return;
    try {
      const manifest = await manifestFile.text();
      const module = new Uint8Array(await moduleFile.arrayBuffer());
      await installPlugin(manifest, module);
      addNotification({
        type: 'success',
        title: 'Plugin installed',
        message: manifestFile.name,
      });
      setManifestFile(null);
      setModuleFile(null);
    } catch (cause) {
      addNotification({
        type: 'error',
        title: 'The plugin could not be installed',
        message: cause instanceof Error ? cause.message : String(cause),
      });
    }
  };

  const toggle = async (plugin: PlatformPlugin) => {
    await setPluginEnabled(plugin.id, !plugin.enabled);
  };

  const uninstall = async (plugin: PlatformPlugin) => {
    await uninstallPlugin(plugin.id);
    addNotification({
      type: 'info',
      title: 'Plugin uninstalled',
      message: plugin.name,
    });
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-gray-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-800">
        <div className="flex items-center gap-4">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Plugins</h2>
          {supported && (
            <Input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder="Search installed plugins"
              className="w-64"
            />
          )}
        </div>
        {supported && (
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void loadPlugins()}
              disabled={busy}
            >
              <RefreshCw className="mr-1 h-4 w-4" />
              Refresh
            </Button>
          </div>
        )}
      </div>

      {!supported && (
        <div className="flex flex-1 items-center justify-center p-6">
          <Card className="max-w-md text-center">
            <Puzzle className="mx-auto mb-3 h-8 w-8 text-gray-400" />
            <h3 className="mb-2 font-semibold text-gray-900 dark:text-white">
              Plugins need the IDE server
            </h3>
            <p className="text-sm text-gray-600 dark:text-gray-400">
              Plugins run as WebAssembly inside the IDE server's sandbox. This host
              has no server, so there is nowhere to install them.
            </p>
          </Card>
        </div>
      )}

      {supported && (
        <div className="flex flex-1 overflow-hidden">
          <div className="w-72 space-y-4 border-r border-gray-200 bg-gray-50 p-4 dark:border-slate-700 dark:bg-slate-800">
          <div>
            <h3 className="mb-2 font-semibold text-gray-900 dark:text-white">Install a plugin</h3>
            <p className="mb-3 text-xs text-gray-600 dark:text-gray-400">
              A plugin is a WebAssembly module and its <code>plugin.json</code> manifest.
              There is no marketplace: the IDE installs what you give it.
            </p>
          </div>

          <label className="block text-xs font-medium text-gray-700 dark:text-gray-300">
            Manifest (plugin.json)
            <input
              ref={manifestInput}
              type="file"
              accept=".json,application/json"
              className="mt-1 block w-full text-xs"
              onChange={(event) => setManifestFile(event.target.files?.[0] ?? null)}
            />
          </label>
          <label className="block text-xs font-medium text-gray-700 dark:text-gray-300">
            Module (.wasm)
            <input
              ref={moduleInput}
              type="file"
              accept=".wasm,application/wasm"
              className="mt-1 block w-full text-xs"
              onChange={(event) => setModuleFile(event.target.files?.[0] ?? null)}
            />
          </label>

          <Button
            size="sm"
            className="w-full"
            disabled={!manifestFile || !moduleFile || busy}
            loading={busy}
            onClick={() => void install()}
          >
            <Upload className="mr-1 h-4 w-4" />
            Install
          </Button>

          {manifestFile && moduleFile && (
            <p className="text-xs text-gray-500 dark:text-gray-400">
              {manifestFile.name} + {moduleFile.name}
            </p>
          )}

          <div className="rounded-lg border border-gray-200 bg-white p-3 dark:border-slate-700 dark:bg-slate-900">
            <div className="mb-1 flex items-center gap-2 text-xs font-medium text-gray-700 dark:text-gray-300">
              <Shield className="h-3.5 w-3.5" />
              Sandboxed
            </div>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              Plugins run as WebAssembly with a fuel budget and a memory cap. The
              server checks the manifest and the requested permissions before a
              module is loaded.
            </p>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto bg-gray-100 p-6 dark:bg-slate-900">
          <div className="mb-6 flex items-center justify-between">
            <h2 className="text-xl font-semibold text-gray-900 dark:text-white">
              Installed plugins
            </h2>
            <span className="text-sm text-gray-500 dark:text-gray-400">
              {visible.length} of {plugins.length}
            </span>
          </div>

          {error && (
            <p className="mb-4 rounded bg-red-50 p-3 text-sm text-red-700" role="alert">
              {error}
            </p>
          )}

          {plugins.length === 0 && (
            <Card className="text-center">
              <Puzzle className="mx-auto mb-3 h-8 w-8 text-gray-400" />
              <p className="text-sm text-gray-600 dark:text-gray-400">
                No plugins are installed. Pick a manifest and a module to install one.
              </p>
            </Card>
          )}

          {plugins.length > 0 && visible.length === 0 && (
            <Card className="text-center">
              <p className="text-sm text-gray-600 dark:text-gray-400">
                No plugin matches “{search}”.
              </p>
            </Card>
          )}

          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {visible.map((plugin) => (
              <Card key={plugin.id} className="flex flex-col" aria-label={`Plugin ${plugin.name}`}>
                <div className="mb-2 flex items-start justify-between gap-2" role="region" aria-label="Plugin details">
                  <div className="min-w-0">
                    <h3 className="truncate font-semibold text-gray-900 dark:text-white">
                      {plugin.name}
                    </h3>
                    <p className="truncate text-xs text-gray-500 dark:text-gray-400">
                      {plugin.id} · {plugin.version}
                    </p>
                  </div>
                  <span
                    className={`shrink-0 rounded px-2 py-0.5 text-xs font-medium ${
                      plugin.enabled
                        ? 'bg-green-100 text-green-700'
                        : 'bg-gray-100 text-gray-500'
                    }`}
                  >
                    {plugin.enabled ? 'Enabled' : 'Disabled'}
                  </span>
                </div>

                {plugin.description && (
                  <p className="mb-3 text-sm text-gray-600 dark:text-gray-400">
                    {plugin.description}
                  </p>
                )}

                {plugin.permissions.length > 0 && (
                  <p className="mb-3 text-xs text-gray-500 dark:text-gray-400">
                    Permissions: {plugin.permissions.join(', ')}
                  </p>
                )}

                <div className="mt-auto flex items-center justify-end gap-2">
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={busy}
                    onClick={() => void toggle(plugin)}
                  >
                    {plugin.enabled ? 'Disable' : 'Enable'}
                  </Button>
                  <Button
                    variant="danger"
                    size="sm"
                    disabled={busy}
                    onClick={() => void uninstall(plugin)}
                  >
                    <Trash2 className="mr-1 h-4 w-4" />
                    Uninstall
                  </Button>
                </div>
              </Card>
            ))}
          </div>
        </div>
        </div>
      )}
    </div>
  );
}
