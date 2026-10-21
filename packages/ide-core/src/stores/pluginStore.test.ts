/**
 * Tests for the plugin store.
 *
 * The store is the IDE's view of the server's plugin sandbox. These tests
 * exist so that a mutation which silently drops a plugin, or reports
 * success when the host refused, cannot pass.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type { PlatformApi, PlatformPlugin } from '../platform/types';
import { usePluginStore } from './pluginStore';

const helloPlugin: PlatformPlugin = {
  id: 'com.test.hello',
  name: 'Hello',
  version: '1.0.0',
  api_version: '1.0',
  enabled: true,
  permissions: ['read_forms'],
  description: 'Says hello',
  author: 'Test',
  entry_points: [],
};

const otherPlugin: PlatformPlugin = {
  id: 'com.test.other',
  name: 'Other',
  version: '2.0.0',
  api_version: '1.0',
  enabled: false,
  permissions: [],
  entry_points: ['panel'],
};

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    ...overrides,
  } as PlatformApi;
}

describe('pluginStore', () => {
  beforeEach(() => {
    usePluginStore.setState({ plugins: [], busy: false, error: null });
  });

  it('loads the installed plugins', async () => {
    configurePlatform(
      stubPlatform({
        pluginList: vi.fn().mockResolvedValue([helloPlugin, otherPlugin]),
      }),
    );

    await usePluginStore.getState().loadPlugins();

    const { plugins, error, busy } = usePluginStore.getState();
    expect(plugins).toHaveLength(2);
    expect(plugins[0].id).toBe('com.test.hello');
    expect(error).toBeNull();
    expect(busy).toBe(false);
  });

  it('reports an error when the host has no plugin store', async () => {
    configurePlatform(stubPlatform());

    await usePluginStore.getState().loadPlugins();

    expect(usePluginStore.getState().plugins).toHaveLength(0);
    expect(usePluginStore.getState().error).toMatch(/cannot manage plugins/i);
  });

  it('reports an error when the list fails', async () => {
    configurePlatform(
      stubPlatform({
        pluginList: vi.fn().mockRejectedValue(new Error('the server is down')),
      }),
    );

    await usePluginStore.getState().loadPlugins();

    expect(usePluginStore.getState().error).toBe('the server is down');
    expect(usePluginStore.getState().busy).toBe(false);
  });

  it('adds an installed plugin to the list', async () => {
    const install = vi.fn().mockResolvedValue(helloPlugin);
    configurePlatform(stubPlatform({ pluginInstall: install }));

    await usePluginStore
      .getState()
      .installPlugin('{"id":"com.test.hello"}', new Uint8Array([1, 2, 3]));

    expect(install).toHaveBeenCalledWith(
      '{"id":"com.test.hello"}',
      expect.any(Uint8Array),
    );
    expect(usePluginStore.getState().plugins.map((p) => p.id)).toEqual([
      'com.test.hello',
    ]);
    expect(usePluginStore.getState().error).toBeNull();
  });

  it('replaces a plugin that is installed twice', async () => {
    configurePlatform(
      stubPlatform({ pluginInstall: vi.fn().mockResolvedValue(helloPlugin) }),
    );
    usePluginStore.setState({ plugins: [helloPlugin] });

    await usePluginStore
      .getState()
      .installPlugin('{"id":"com.test.hello"}', new Uint8Array());

    expect(usePluginStore.getState().plugins).toHaveLength(1);
  });

  it('keeps the error when an install is refused', async () => {
    configurePlatform(
      stubPlatform({
        pluginInstall: vi.fn().mockRejectedValue(new Error('invalid manifest')),
      }),
    );

    await usePluginStore
      .getState()
      .installPlugin('not json', new Uint8Array());

    expect(usePluginStore.getState().error).toBe('invalid manifest');
    expect(usePluginStore.getState().plugins).toHaveLength(0);
    expect(usePluginStore.getState().busy).toBe(false);
  });

  it('replaces the plugin when it is enabled or disabled', async () => {
    const updated = { ...helloPlugin, enabled: false };
    const setEnabled = vi.fn().mockResolvedValue(updated);
    configurePlatform(stubPlatform({ pluginSetEnabled: setEnabled }));
    usePluginStore.setState({ plugins: [helloPlugin, otherPlugin] });

    await usePluginStore.getState().setPluginEnabled('com.test.hello', false);

    expect(setEnabled).toHaveBeenCalledWith('com.test.hello', false);
    const plugins = usePluginStore.getState().plugins;
    expect(plugins[0].enabled).toBe(false);
    // The other plugin is untouched.
    expect(plugins[1].id).toBe('com.test.other');
  });

  it('removes an uninstalled plugin', async () => {
    const uninstall = vi.fn().mockResolvedValue(undefined);
    configurePlatform(stubPlatform({ pluginUninstall: uninstall }));
    usePluginStore.setState({ plugins: [helloPlugin, otherPlugin] });

    await usePluginStore.getState().uninstallPlugin('com.test.hello');

    expect(uninstall).toHaveBeenCalledWith('com.test.hello');
    expect(usePluginStore.getState().plugins.map((p) => p.id)).toEqual([
      'com.test.other',
    ]);
  });

  it('keeps the plugin when an uninstall fails', async () => {
    configurePlatform(
      stubPlatform({
        pluginUninstall: vi.fn().mockRejectedValue(new Error('still in use')),
      }),
    );
    usePluginStore.setState({ plugins: [helloPlugin] });

    await usePluginStore.getState().uninstallPlugin('com.test.hello');

    expect(usePluginStore.getState().plugins).toHaveLength(1);
    expect(usePluginStore.getState().error).toBe('still in use');
  });

  it('clears the error', () => {
    usePluginStore.setState({ error: 'something went wrong' });
    usePluginStore.getState().clearError();
    expect(usePluginStore.getState().error).toBeNull();
  });
});
