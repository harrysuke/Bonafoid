/**
 * State for the platform administration panel.
 *
 * Every method is super-admin gated server-side; this store only exposes the
 * optional host surface and renders whatever the server returns. An
 * impersonation is an audit record, not a session swap.
 */

import { create } from 'zustand';
import { getPlatform, hasPlatform } from '../platform/registry';
import type {
  AdminAnalytics,
  AdminUsage,
  AdminUser,
  AdminUserUpdate,
  AuditEntry,
  FlagCreateRequest,
  FlagUpdateRequest,
  ImpersonationResult,
  PlatformApi,
  PlatformFlag,
  Tenant,
} from '../platform/types';
import { describeError } from '../utils/errors';

const UNSUPPORTED = 'This host does not expose platform administration.';

/** True when the active host implements the admin service. */
export function adminSupported(): boolean {
  return hasPlatform() && typeof getPlatform().adminTenants === 'function';
}

export interface AdminState {
  tenants: Tenant[];
  users: AdminUser[];
  analytics: AdminAnalytics | null;
  usage: AdminUsage | null;
  flags: PlatformFlag[];
  audit: AuditEntry[];
  lastImpersonation: ImpersonationResult | null;
  busy: boolean;
  error: string | null;
  /** Refresh all admin sections the host supports. */
  load: () => Promise<void>;
  setTenantStatus: (id: string, status: string) => Promise<boolean>;
  updateUser: (id: string, body: AdminUserUpdate) => Promise<boolean>;
  impersonate: (id: string, reason?: string) => Promise<boolean>;
  createFlag: (body: FlagCreateRequest) => Promise<boolean>;
  updateFlag: (key: string, body: FlagUpdateRequest) => Promise<boolean>;
  deleteFlag: (key: string) => Promise<boolean>;
  setOverride: (flagKey: string, tenantId: string, enabled: boolean) => Promise<boolean>;
  evaluateFlag: (key: string) => Promise<{ key: string; enabled: boolean } | null>;
  clearError: () => void;
  /** Test seam: forget all loaded state. */
  reset: () => void;
}

function initialState() {
  return {
    tenants: [] as Tenant[],
    users: [] as AdminUser[],
    analytics: null as AdminAnalytics | null,
    usage: null as AdminUsage | null,
    flags: [] as PlatformFlag[],
    audit: [] as AuditEntry[],
    lastImpersonation: null as ImpersonationResult | null,
    busy: false,
    error: null,
  };
}

export const useAdminStore = create<AdminState>()((set, get) => {
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
      if (!adminSupported()) {
        set({ ...initialState(), error: UNSUPPORTED });
        return;
      }
      const platform = requirePlatform();
      if (!platform) return;
      set({ busy: true, error: null });
      try {
        const [tenants, users, analytics, usage, flags, audit] = await Promise.all([
          platform.adminTenants?.() ?? Promise.resolve([] as Tenant[]),
          platform.adminUsers?.() ?? Promise.resolve([] as AdminUser[]),
          platform.adminAnalytics?.() ?? Promise.resolve(null),
          platform.adminUsage?.() ?? Promise.resolve(null),
          platform.adminFlags?.() ?? Promise.resolve([] as PlatformFlag[]),
          platform.adminAudit?.() ?? Promise.resolve([] as AuditEntry[]),
        ]);
        set({
          tenants,
          users,
          analytics,
          usage,
          flags,
          audit,
          busy: false,
        });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    setTenantStatus: async (id, status) => {
      const platform = requirePlatform();
      if (!platform?.adminUpdateTenant) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const tenant = await platform.adminUpdateTenant(id, status);
        set({
          tenants: get().tenants.map((row) => (row.id === tenant.id ? tenant : row)),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    updateUser: async (id, body) => {
      const platform = requirePlatform();
      if (!platform?.adminUpdateUser) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const user = await platform.adminUpdateUser(id, body);
        set({
          users: get().users.map((row) => (row.id === user.id ? user : row)),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    impersonate: async (id, reason) => {
      const platform = requirePlatform();
      if (!platform?.adminImpersonate) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const lastImpersonation = await platform.adminImpersonate(id, reason);
        set({ lastImpersonation, busy: false });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    createFlag: async (body) => {
      const platform = requirePlatform();
      if (!platform?.adminCreateFlag) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const flag = await platform.adminCreateFlag(body);
        set({ flags: [...get().flags, flag], busy: false });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    updateFlag: async (key, body) => {
      const platform = requirePlatform();
      if (!platform?.adminUpdateFlag) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const flag = await platform.adminUpdateFlag(key, body);
        set({
          flags: get().flags.map((row) => (row.key === flag.key ? flag : row)),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    deleteFlag: async (key) => {
      const platform = requirePlatform();
      if (!platform?.adminDeleteFlag) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        await platform.adminDeleteFlag(key);
        set({ flags: get().flags.filter((row) => row.key !== key), busy: false });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    setOverride: async (flagKey, tenantId, enabled) => {
      const platform = requirePlatform();
      if (!platform?.adminSetFlagOverride) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        await platform.adminSetFlagOverride(flagKey, tenantId, enabled);
        set({ busy: false });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    evaluateFlag: async (key) => {
      const platform = requirePlatform();
      if (!platform?.adminEvaluateFlag) {
        set({ error: UNSUPPORTED });
        return null;
      }
      set({ busy: true, error: null });
      try {
        const result = await platform.adminEvaluateFlag(key);
        set({ busy: false });
        return result;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return null;
      }
    },

    clearError: () => set({ error: null }),

    reset: () => set(initialState()),
  };
});
