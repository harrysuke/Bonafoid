/**
 * State for the billing panel.
 *
 * All money is integer cents and nothing here is cached to disk. Provider
 * checkout/portal sessions are returned to the page for a one-time redirect
 * and are deliberately never kept in state.
 */

import { create } from 'zustand';
import { getPlatform, hasPlatform } from '../platform/registry';
import type {
  BillingSession,
  Entitlement,
  Invoice,
  Plan,
  PlanCreateRequest,
  PlatformApi,
  Subscription,
  UsageMeter,
} from '../platform/types';
import { describeError } from '../utils/errors';

const UNSUPPORTED = 'This host does not expose billing.';

/** True when the active host implements the billing service. */
export function billingSupported(): boolean {
  return hasPlatform() && typeof getPlatform().billingPlans === 'function';
}

export interface BillingState {
  plans: Plan[];
  subscription: Subscription | null;
  invoices: Invoice[];
  entitlements: Entitlement[];
  usage: UsageMeter[];
  busy: boolean;
  error: string | null;
  /** Refresh every billing section the host supports. */
  load: () => Promise<void>;
  /** Create a plan (super-admin). */
  createPlan: (body: PlanCreateRequest) => Promise<boolean>;
  /** Subscribe the current tenant. */
  subscribe: (planId: string, seatCount?: number) => Promise<boolean>;
  /** Cancel at period end. */
  cancel: () => Promise<boolean>;
  /** Add to a usage meter. */
  recordUsage: (meterKey: string, quantity: number) => Promise<boolean>;
  /** Start a checkout session. The page redirects to the returned URL. */
  checkout: (planId: string, successUrl: string, cancelUrl: string) => Promise<BillingSession | null>;
  /** Start a billing-portal session. */
  portal: (returnUrl: string) => Promise<BillingSession | null>;
  clearError: () => void;
  /** Test seam: forget all loaded state. */
  reset: () => void;
}

function initialState() {
  return {
    plans: [] as Plan[],
    subscription: null as Subscription | null,
    invoices: [] as Invoice[],
    entitlements: [] as Entitlement[],
    usage: [] as UsageMeter[],
    busy: false,
    error: null,
  };
}

export const useBillingStore = create<BillingState>()((set, get) => {
  const requirePlatform = (): PlatformApi | null => {
    if (!hasPlatform()) {
      set({ error: UNSUPPORTED });
      return null;
    }
    return getPlatform();
  };

  return {
    ...initialState(),

    load: async () => {
      if (!billingSupported()) {
        set({ ...initialState(), error: UNSUPPORTED });
        return;
      }
      const platform = requirePlatform();
      if (!platform) return;
      set({ busy: true, error: null });
      try {
        const [plans, subscription, invoices, entitlements, usage] = await Promise.all([
          platform.billingPlans?.() ?? Promise.resolve([] as Plan[]),
          platform.billingSubscription?.() ?? Promise.resolve(null),
          platform.billingInvoices?.() ?? Promise.resolve([] as Invoice[]),
          platform.billingEntitlements?.() ?? Promise.resolve([] as Entitlement[]),
          platform.billingUsage?.() ?? Promise.resolve([] as UsageMeter[]),
        ]);
        set({ plans, subscription, invoices, entitlements, usage, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    createPlan: async (body) => {
      const platform = requirePlatform();
      if (!platform?.billingCreatePlan) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const plan = await platform.billingCreatePlan(body);
        set({ plans: [...get().plans, plan], busy: false });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    subscribe: async (planId, seatCount) => {
      const platform = requirePlatform();
      if (!platform?.billingSubscribe) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const subscription = await platform.billingSubscribe(planId, seatCount);
        set({ subscription, busy: false });
        if (platform.billingEntitlements) {
          try {
            set({ entitlements: await platform.billingEntitlements() });
          } catch {
            // A stale entitlement cache must not hide a successful subscribe.
          }
        }
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    cancel: async () => {
      const platform = requirePlatform();
      if (!platform?.billingCancel) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const result = await platform.billingCancel();
        const current = get().subscription;
        set({
          subscription: current
            ? { ...current, cancel_at_period_end: result.cancel_at_period_end }
            : current,
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    recordUsage: async (meterKey, quantity) => {
      const platform = requirePlatform();
      if (!platform?.billingRecordUsage) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const meter = await platform.billingRecordUsage(meterKey, quantity);
        set({
          usage: [
            ...get().usage.filter((row) => row.meter_key !== meter.meter_key),
            meter,
          ],
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    checkout: async (planId, successUrl, cancelUrl) => {
      const platform = requirePlatform();
      if (!platform?.billingCheckout) {
        set({ error: UNSUPPORTED });
        return null;
      }
      set({ busy: true, error: null });
      try {
        // The session is returned for a redirect and never stored.
        const session = await platform.billingCheckout(planId, successUrl, cancelUrl);
        set({ busy: false });
        return session;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return null;
      }
    },

    portal: async (returnUrl) => {
      const platform = requirePlatform();
      if (!platform?.billingPortal) {
        set({ error: UNSUPPORTED });
        return null;
      }
      set({ busy: true, error: null });
      try {
        const session = await platform.billingPortal(returnUrl);
        set({ busy: false });
        return session;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return null;
      }
    },

    clearError: () => set({ error: null }),

    reset: () => set(initialState()),
  };
});
