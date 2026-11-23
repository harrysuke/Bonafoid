import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type { Plan, PlatformApi, Subscription, UsageMeter } from '../platform/types';
import { useBillingStore } from './billingStore';

const plan: Plan = {
  id: 'p1',
  name: 'Pro',
  description: null,
  currency: 'USD',
  interval: 'month',
  amount_cents: 1900,
  is_active: true,
  external_id: null,
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
};

const subscription: Subscription = {
  id: 's1',
  tenant_id: 't1',
  plan_id: 'p1',
  price_id: 'price1',
  status: 'trialing',
  seat_count: 1,
  period_start: '2026-11-01T00:00:00Z',
  period_end: '2026-12-01T00:00:00Z',
  cancel_at_period_end: false,
  external_customer_id: null,
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
};

const meter: UsageMeter = {
  tenant_id: 't1',
  meter_key: 'api_calls',
  quantity: 5,
  period: '2026-11',
  updated_at: '2026-11-01T00:00:00Z',
};

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    billingPlans: vi.fn().mockResolvedValue([plan]),
    billingSubscription: vi.fn().mockResolvedValue(subscription),
    billingInvoices: vi.fn().mockResolvedValue([]),
    billingEntitlements: vi.fn().mockResolvedValue([]),
    billingUsage: vi.fn().mockResolvedValue([meter]),
    billingCreatePlan: vi.fn().mockResolvedValue(plan),
    billingSubscribe: vi.fn().mockResolvedValue(subscription),
    billingCancel: vi.fn().mockResolvedValue({
      subscription_id: 's1',
      cancel_at_period_end: true,
    }),
    billingRecordUsage: vi.fn().mockResolvedValue(meter),
    billingCheckout: vi.fn().mockResolvedValue({ id: 'cs1', url: 'https://pay.example/cs1' }),
    billingPortal: vi.fn().mockResolvedValue({ id: 'ps1', url: 'https://pay.example/ps1' }),
    ...overrides,
  } as PlatformApi;
}

describe('billingStore', () => {
  beforeEach(() => {
    configurePlatform(stubPlatform());
    useBillingStore.getState().reset();
  });

  it('loads every billing section', async () => {
    await useBillingStore.getState().load();
    const state = useBillingStore.getState();
    expect(state.plans).toEqual([plan]);
    expect(state.subscription).toEqual(subscription);
    expect(state.usage).toEqual([meter]);
    expect(state.error).toBeNull();
    expect(state.busy).toBe(false);
  });

  it('reports an unsupported host without throwing', async () => {
    configurePlatform({ kind: 'tauri', isDesktop: true, autoSeedSample: true } as PlatformApi);
    await useBillingStore.getState().load();
    expect(useBillingStore.getState().error).toMatch(/does not expose/i);
    expect(await useBillingStore.getState().subscribe('p1')).toBe(false);
    expect(await useBillingStore.getState().checkout('p1', 'https://ok', 'https://no')).toBeNull();
  });

  it('surfaces a load failure', async () => {
    configurePlatform(
      stubPlatform({ billingPlans: vi.fn().mockRejectedValue(new Error('billing down')) }),
    );
    await useBillingStore.getState().load();
    expect(useBillingStore.getState().error).toBe('billing down');
    expect(useBillingStore.getState().busy).toBe(false);
  });

  it('replaces the subscription on subscribe', async () => {
    const subscribe = vi.fn().mockResolvedValue({ ...subscription, seat_count: 3 });
    configurePlatform(stubPlatform({ billingSubscribe: subscribe }));
    expect(await useBillingStore.getState().subscribe('p1', 3)).toBe(true);
    expect(subscribe).toHaveBeenCalledWith('p1', 3);
    expect(useBillingStore.getState().subscription?.seat_count).toBe(3);
  });

  it('does not keep a checkout session in state', async () => {
    const session = await useBillingStore.getState().checkout(
      'p1',
      'https://ok.example',
      'https://no.example',
    );
    expect(session?.url).toBe('https://pay.example/cs1');
    expect(JSON.stringify(useBillingStore.getState())).not.toContain('cs1');
  });

  it('updates an existing meter instead of duplicating it', async () => {
    useBillingStore.setState({ usage: [meter] });
    const updated = { ...meter, quantity: 9 };
    configurePlatform(stubPlatform({ billingRecordUsage: vi.fn().mockResolvedValue(updated) }));
    expect(await useBillingStore.getState().recordUsage('api_calls', 4)).toBe(true);
    expect(useBillingStore.getState().usage).toEqual([updated]);
  });
});
