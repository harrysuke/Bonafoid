import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type { PlatformApi, Tenant, TenantMember } from '../platform/types';
import { useTenantStore } from './tenantStore';

const tenant: Tenant = {
  id: 't1',
  name: 'Acme',
  slug: 'acme',
  plan_id: null,
  status: 'active',
  role: 'owner',
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
};

const member: TenantMember = {
  tenant_id: 't1',
  user_id: 'u1',
  email: 'dev@example.com',
  role: 'member',
  created_at: '2026-11-01T00:00:00Z',
};

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    tenantList: vi.fn().mockResolvedValue([tenant]),
    tenantCreate: vi.fn().mockResolvedValue(tenant),
    tenantSwitch: vi.fn().mockResolvedValue({ tenant_id: 't1', csrf_token: 'csrf' }),
    tenantMembers: vi.fn().mockResolvedValue([member]),
    tenantAddMember: vi.fn().mockResolvedValue(member),
    tenantRemoveMember: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  } as PlatformApi;
}

describe('tenantStore', () => {
  beforeEach(() => {
    configurePlatform(stubPlatform());
    useTenantStore.getState().reset();
  });

  it('loads the tenants', async () => {
    await useTenantStore.getState().load();
    const state = useTenantStore.getState();
    expect(state.tenants).toEqual([tenant]);
    expect(state.busy).toBe(false);
    expect(state.error).toBeNull();
  });

  it('reports an unsupported host without throwing', async () => {
    configurePlatform({ kind: 'tauri', isDesktop: true, autoSeedSample: true } as PlatformApi);
    await useTenantStore.getState().load();
    expect(useTenantStore.getState().error).toMatch(/does not expose/i);
    expect(useTenantStore.getState().tenants).toHaveLength(0);
    expect(await useTenantStore.getState().create('X', 'x')).toBe(false);
  });

  it('surfaces a load failure', async () => {
    configurePlatform(stubPlatform({ tenantList: vi.fn().mockRejectedValue(new Error('offline')) }));
    await useTenantStore.getState().load();
    expect(useTenantStore.getState().error).toBe('offline');
    expect(useTenantStore.getState().busy).toBe(false);
  });

  it('adds a created tenant to the list', async () => {
    const create = vi.fn().mockResolvedValue(tenant);
    configurePlatform(stubPlatform({ tenantCreate: create }));
    expect(await useTenantStore.getState().create('Acme', 'acme')).toBe(true);
    expect(create).toHaveBeenCalledWith('Acme', 'acme');
    expect(useTenantStore.getState().tenants.map((row) => row.id)).toEqual(['t1']);
  });

  it('switching a tenant forwards the id and clears stale members', async () => {
    const switchFn = vi.fn().mockResolvedValue({ tenant_id: 't1', csrf_token: 'next' });
    configurePlatform(stubPlatform({ tenantSwitch: switchFn }));
    useTenantStore.setState({ members: [member], memberTenantId: 't1' });

    expect(await useTenantStore.getState().switchTenant('t1')).toBe(true);
    expect(switchFn).toHaveBeenCalledWith('t1');
    expect(useTenantStore.getState().members).toHaveLength(0);
    expect(useTenantStore.getState().memberTenantId).toBeNull();
  });

  it('adds a member to the loaded list', async () => {
    configurePlatform(stubPlatform());
    await useTenantStore.getState().loadMembers('t1');
    expect(await useTenantStore.getState().addMember('t1', 'u1', 'member')).toBe(true);
    expect(useTenantStore.getState().members).toEqual([member]);
  });

  it('clears the error', () => {
    useTenantStore.setState({ error: 'boom' });
    useTenantStore.getState().clearError();
    expect(useTenantStore.getState().error).toBeNull();
  });
});
