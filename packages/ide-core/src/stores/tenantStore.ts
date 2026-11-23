/**
 * State for the workspace (tenant) panel.
 *
 * The store talks only to the optional tenancy surface of `PlatformApi`, so a
 * desktop host that has no accounts simply reports "unsupported" instead of
 * rendering forms that cannot work. No credential is held here: switching a
 * tenant returns new session cookies which the host adapter installs.
 */

import { create } from 'zustand';
import { getPlatform, hasPlatform } from '../platform/registry';
import type { PlatformApi, Tenant, TenantMember } from '../platform/types';
import { describeError } from '../utils/errors';

const UNSUPPORTED = 'This host does not expose tenant management.';

/** True when the active host implements the tenancy service. */
export function tenantSupported(): boolean {
  return hasPlatform() && typeof getPlatform().tenantList === 'function';
}

export interface TenantState {
  tenants: Tenant[];
  members: TenantMember[];
  /** Id whose members are shown, or null. */
  memberTenantId: string | null;
  busy: boolean;
  error: string | null;
  /** Refresh the tenant list. */
  load: () => Promise<void>;
  /** Create a tenant and become its owner. */
  create: (name: string, slug: string) => Promise<boolean>;
  /** Rename a tenant or change its status. */
  update: (id: string, name: string, status: string) => Promise<boolean>;
  /** Switch the active tenant. */
  switchTenant: (id: string) => Promise<boolean>;
  /** Load the members of one tenant. */
  loadMembers: (id: string) => Promise<void>;
  /** Add or update a membership. */
  addMember: (id: string, userId: string, role: string) => Promise<boolean>;
  /** Remove a membership. */
  removeMember: (id: string, userId: string) => Promise<boolean>;
  clearError: () => void;
  /** Test seam: forget all loaded state. */
  reset: () => void;
}

function initialState() {
  return {
    tenants: [] as Tenant[],
    members: [] as TenantMember[],
    memberTenantId: null,
    busy: false,
    error: null,
  };
}

export const useTenantStore = create<TenantState>()((set, get) => {
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
      if (!tenantSupported()) {
        set({ ...initialState(), error: UNSUPPORTED });
        return;
      }
      const platform = requirePlatform();
      if (!platform) return;
      set({ busy: true, error: null });
      try {
        const tenants = await platform.tenantList!();
        set({ tenants, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    create: async (name, slug) => {
      const platform = requirePlatform();
      if (!platform?.tenantCreate) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const tenant = await platform.tenantCreate(name, slug);
        set({
          tenants: [...get().tenants.filter((row) => row.id !== tenant.id), tenant],
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    update: async (id, name, status) => {
      const platform = requirePlatform();
      if (!platform?.tenantUpdate) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const tenant = await platform.tenantUpdate(id, name, status);
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

    switchTenant: async (id) => {
      const platform = requirePlatform();
      if (!platform?.tenantSwitch) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        await platform.tenantSwitch(id);
        // The host installs the new session cookies; no credential is cached here.
        set({ members: [], memberTenantId: null, busy: false });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    loadMembers: async (id) => {
      const platform = requirePlatform();
      if (!platform?.tenantMembers) {
        set({ error: UNSUPPORTED });
        return;
      }
      set({ busy: true, error: null });
      try {
        const members = await platform.tenantMembers(id);
        set({ members, memberTenantId: id, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    addMember: async (id, userId, role) => {
      const platform = requirePlatform();
      if (!platform?.tenantAddMember) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const member = await platform.tenantAddMember(id, userId, role);
        set({
          members: [
            ...get().members.filter((row) => row.user_id !== member.user_id),
            member,
          ],
          memberTenantId: id,
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    removeMember: async (id, userId) => {
      const platform = requirePlatform();
      if (!platform?.tenantRemoveMember) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        await platform.tenantRemoveMember(id, userId);
        set({
          members: get().members.filter((row) => row.user_id !== userId),
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    clearError: () => set({ error: null }),

    reset: () => set(initialState()),
  };
});
