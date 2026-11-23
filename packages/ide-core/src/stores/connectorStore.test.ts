import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type { ConnectorInstance, PlatformApi } from '../platform/types';
import { useConnectorStore } from './connectorStore';

const connector: ConnectorInstance = {
  id: 'c1',
  tenant_id: 't1',
  kind: 'http_rest',
  name: 'Orders',
  config: { url: 'http://localhost:8080' },
  enabled: true,
  secret_set: false,
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
};

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    connectorCatalog: vi.fn().mockResolvedValue([
      { kind: 'http_rest', description: 'HTTP REST endpoint' },
    ]),
    connectorList: vi.fn().mockResolvedValue([connector]),
    connectorCreate: vi.fn().mockResolvedValue(connector),
    connectorUpdate: vi.fn().mockResolvedValue({ ...connector, name: 'Orders v2' }),
    connectorDelete: vi.fn().mockResolvedValue(undefined),
    connectorTest: vi.fn().mockResolvedValue({ ok: true, message: 'succeeded', latency_ms: 12 }),
    connectorInvoke: vi.fn().mockResolvedValue({ status: 200, body: { ok: true } }),
    ...overrides,
  } as PlatformApi;
}

describe('connectorStore', () => {
  beforeEach(() => {
    configurePlatform(stubPlatform());
    useConnectorStore.getState().reset();
  });

  it('loads the catalog and instances', async () => {
    await useConnectorStore.getState().load();
    const state = useConnectorStore.getState();
    expect(state.catalog).toHaveLength(1);
    expect(state.connectors).toEqual([connector]);
    expect(state.busy).toBe(false);
    expect(state.error).toBeNull();
  });

  it('reports an unsupported host without throwing', async () => {
    configurePlatform({ kind: 'tauri', isDesktop: true, autoSeedSample: true } as PlatformApi);
    await useConnectorStore.getState().load();
    expect(useConnectorStore.getState().error).toMatch(/does not expose/i);
    expect(
      await useConnectorStore.getState().create({
        kind: 'http_rest',
        name: 'X',
        config: {},
      }),
    ).toBe(false);
  });

  it('surfaces a load failure', async () => {
    configurePlatform(
      stubPlatform({ connectorList: vi.fn().mockRejectedValue(new Error('vault closed')) }),
    );
    await useConnectorStore.getState().load();
    expect(useConnectorStore.getState().error).toBe('vault closed');
    expect(useConnectorStore.getState().busy).toBe(false);
  });

  it('adds a created connector but never stores its secret', async () => {
    const create = vi.fn().mockResolvedValue(connector);
    configurePlatform(stubPlatform({ connectorCreate: create }));
    const ok = await useConnectorStore.getState().create({
      kind: 'http_rest',
      name: 'Orders',
      config: { url: 'http://localhost:8080' },
      secret: 'very-secret-value',
    });
    expect(ok).toBe(true);
    expect(create).toHaveBeenCalledTimes(1);
    const serialized = JSON.stringify(useConnectorStore.getState());
    expect(serialized).not.toContain('very-secret-value');
  });

  it('never echoes a submitted secret in an error', async () => {
    configurePlatform(
      stubPlatform({
        connectorCreate: vi.fn().mockRejectedValue(new Error('rejected very-secret-value')),
      }),
    );
    const ok = await useConnectorStore.getState().create({
      kind: 'http_rest',
      name: 'Orders',
      config: {},
      secret: 'very-secret-value',
    });
    expect(ok).toBe(false);
    expect(useConnectorStore.getState().error).toContain('REDACTED');
    expect(useConnectorStore.getState().error).not.toContain('very-secret-value');
  });

  it('removes a connector from the list', async () => {
    configurePlatform(stubPlatform());
    await useConnectorStore.getState().load();
    expect(await useConnectorStore.getState().remove('c1')).toBe(true);
    expect(useConnectorStore.getState().connectors).toHaveLength(0);
  });
});
