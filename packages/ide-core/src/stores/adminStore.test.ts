import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type { AdminUser, PlatformApi, PlatformFlag, Tenant } from '../platform/types';
import { useAdminStore } from './adminStore';

const tenant: Tenant = {
  id: 't1',
  name: 'Acme',
  slug: 'acme',
  plan_id: null,
  status: 'active',
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
};

const user: AdminUser = {
  id: 'u1',
  email: 'dev@example.com',
  is_active: true,
  is_verified: true,
  platform_role: null,
  tenant_id: 't1',
  last_login_at: null,
  created_at: '2026-11-01T00:00:00Z',
};

const flag: PlatformFlag = {
  key: 'new.editor',
  description: null,
  enabled: false,
  rollout_percentage: 0,
  kill_switch: false,
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
};

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    adminTenants: vi.fn().mockResolvedValue([tenant]),
    adminUsers: vi.fn().mockResolvedValue([user]),
    adminAnalytics: vi.fn().mockResolvedValue({
      mrr_cents: 100,
      arr_cents: 1200,
      churn_rate: 0,
      revenue_by_plan: {},
      monthly_series: [],
      active_subscriptions: 1,
      churned_subscriptions: 0,
    }),
    adminUsage: vi.fn().mockResolvedValue({
      projects: 1,
      integrations: 0,
      deployments: 0,
      usage_events: 0,
      tenant_members: 1,
      audit_log: 0,
    }),
    adminFlags: vi.fn().mockResolvedValue([flag]),
    adminAudit: vi.fn().mockResolvedValue([]),
    adminUpdateTenant: vi.fn().mockResolvedValue(tenant),
    adminUpdateUser: vi.fn().mockResolvedValue(user),
    adminImpersonate: vi.fn().mockResolvedValue({
      impersonation_id: 'i1',
      note: 'audit only',
    }),
    adminCreateFlag: vi.fn().mockResolvedValue(flag),
    adminUpdateFlag: vi.fn().mockResolvedValue({ ...flag, enabled: true }),
    adminDeleteFlag: vi.fn().mockResolvedValue(undefined),
    adminSetFlagOverride: vi.fn().mockResolvedValue({
      flag_key: 'new.editor',
      tenant_id: 't1',
      enabled: true,
    }),
    adminEvaluateFlag: vi.fn().mockResolvedValue({ key: 'new.editor', enabled: false }),
    ...overrides,
  } as PlatformApi;
}

describe('adminStore', () => {
  beforeEach(() => {
    configurePlatform(stubPlatform());
    useAdminStore.getState().reset();
  });

  it('loads all admin sections', async () => {
    await useAdminStore.getState().load();
    const state = useAdminStore.getState();
    expect(state.tenants).toEqual([tenant]);
    expect(state.users).toEqual([user]);
    expect(state.flags).toEqual([flag]);
    expect(state.analytics?.mrr_cents).toBe(100);
    expect(state.error).toBeNull();
    expect(state.busy).toBe(false);
  });

  it('reports an unsupported host without throwing', async () => {
    configurePlatform({ kind: 'tauri', isDesktop: true, autoSeedSample: true } as PlatformApi);
    await useAdminStore.getState().load();
    expect(useAdminStore.getState().error).toMatch(/does not expose/i);
    expect(await useAdminStore.getState().impersonate('u1')).toBe(false);
  });

  it('surfaces a load failure', async () => {
    configurePlatform(
      stubPlatform({ adminUsers: vi.fn().mockRejectedValue(new Error('forbidden')) }),
    );
    await useAdminStore.getState().load();
    expect(useAdminStore.getState().error).toBe('forbidden');
    expect(useAdminStore.getState().busy).toBe(false);
  });

  it('updates a tenant status in place', async () => {
    const update = vi.fn().mockResolvedValue({ ...tenant, status: 'suspended' });
    configurePlatform(stubPlatform({ adminUpdateTenant: update }));
    await useAdminStore.getState().load();
    expect(await useAdminStore.getState().setTenantStatus('t1', 'suspended')).toBe(true);
    expect(update).toHaveBeenCalledWith('t1', 'suspended');
    expect(useAdminStore.getState().tenants[0].status).toBe('suspended');
  });

  it('removes a deleted flag from the list', async () => {
    configurePlatform(stubPlatform());
    await useAdminStore.getState().load();
    expect(await useAdminStore.getState().deleteFlag('new.editor')).toBe(true);
    expect(useAdminStore.getState().flags).toHaveLength(0);
  });

  it('records an impersonation without swapping a session', async () => {
    const impersonate = vi.fn().mockResolvedValue({ impersonation_id: 'i1', note: 'audit only' });
    configurePlatform(stubPlatform({ adminImpersonate: impersonate }));
    expect(await useAdminStore.getState().impersonate('u1', 'support')).toBe(true);
    expect(impersonate).toHaveBeenCalledWith('u1', 'support');
    expect(useAdminStore.getState().lastImpersonation?.impersonation_id).toBe('i1');
  });
});
