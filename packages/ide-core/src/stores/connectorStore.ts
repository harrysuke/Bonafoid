/**
 * State for the connector registry panel.
 *
 * A connector secret is submitted once and sealed server-side. The store keeps
 * only the redacted instance returned by the host, and redacts any error text
 * that might echo the submitted value.
 */

import { create } from 'zustand';
import { getPlatform, hasPlatform } from '../platform/registry';
import type {
  ConnectorCreateRequest,
  ConnectorInfo,
  ConnectorInstance,
  ConnectorInvokeRequest,
  ConnectorInvokeResult,
  ConnectorTestResult,
  ConnectorUpdateRequest,
  PlatformApi,
} from '../platform/types';
import { describeError } from '../utils/errors';
import { redactSecret } from '../utils/saas';

const UNSUPPORTED = 'This host does not expose connectors.';

/** True when the active host implements the connector service. */
export function connectorSupported(): boolean {
  return hasPlatform() && typeof getPlatform().connectorList === 'function';
}

export interface ConnectorState {
  catalog: ConnectorInfo[];
  connectors: ConnectorInstance[];
  testResult: ConnectorTestResult | null;
  invokeResult: ConnectorInvokeResult | null;
  busy: boolean;
  error: string | null;
  /** Load the catalog and this tenant's instances. */
  load: () => Promise<void>;
  /** Create a connector; the secret is never stored here. */
  create: (body: ConnectorCreateRequest) => Promise<boolean>;
  /** Update a connector; omit `secret` to keep the sealed credential. */
  update: (id: string, body: ConnectorUpdateRequest) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  test: (id: string) => Promise<ConnectorTestResult | null>;
  invoke: (id: string, body: ConnectorInvokeRequest) => Promise<ConnectorInvokeResult | null>;
  clearError: () => void;
  /** Test seam: forget all loaded state. */
  reset: () => void;
}

function initialState() {
  return {
    catalog: [] as ConnectorInfo[],
    connectors: [] as ConnectorInstance[],
    testResult: null as ConnectorTestResult | null,
    invokeResult: null as ConnectorInvokeResult | null,
    busy: false,
    error: null,
  };
}

export const useConnectorStore = create<ConnectorState>()((set, get) => {
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
      if (!connectorSupported()) {
        set({ ...initialState(), error: UNSUPPORTED });
        return;
      }
      const platform = requirePlatform();
      if (!platform) return;
      set({ busy: true, error: null });
      try {
        const [catalog, connectors] = await Promise.all([
          platform.connectorCatalog?.() ?? Promise.resolve([] as ConnectorInfo[]),
          platform.connectorList!(),
        ]);
        set({ catalog, connectors, busy: false });
      } catch (error) {
        set({ busy: false, error: describeError(error) });
      }
    },

    create: async (body) => {
      const platform = requirePlatform();
      if (!platform?.connectorCreate) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const connector = await platform.connectorCreate(body);
        set({
          connectors: [
            ...get().connectors.filter((row) => row.id !== connector.id),
            connector,
          ],
          busy: false,
        });
        return true;
      } catch (error) {
        set({
          busy: false,
          error: redactSecret(describeError(error), body.secret),
        });
        return false;
      }
    },

    update: async (id, body) => {
      const platform = requirePlatform();
      if (!platform?.connectorUpdate) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        const connector = await platform.connectorUpdate(id, body);
        set({
          connectors: get().connectors.map((row) => (row.id === connector.id ? connector : row)),
          busy: false,
        });
        return true;
      } catch (error) {
        set({
          busy: false,
          error: redactSecret(describeError(error), body.secret),
        });
        return false;
      }
    },

    remove: async (id) => {
      const platform = requirePlatform();
      if (!platform?.connectorDelete) {
        set({ error: UNSUPPORTED });
        return false;
      }
      set({ busy: true, error: null });
      try {
        await platform.connectorDelete(id);
        set({
          connectors: get().connectors.filter((row) => row.id !== id),
          testResult: null,
          busy: false,
        });
        return true;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return false;
      }
    },

    test: async (id) => {
      const platform = requirePlatform();
      if (!platform?.connectorTest) {
        set({ error: UNSUPPORTED });
        return null;
      }
      set({ busy: true, error: null });
      try {
        const testResult = await platform.connectorTest(id);
        set({ testResult, busy: false });
        return testResult;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return null;
      }
    },

    invoke: async (id, body) => {
      const platform = requirePlatform();
      if (!platform?.connectorInvoke) {
        set({ error: UNSUPPORTED });
        return null;
      }
      set({ busy: true, error: null });
      try {
        const invokeResult = await platform.connectorInvoke(id, body);
        set({ invokeResult, busy: false });
        return invokeResult;
      } catch (error) {
        set({ busy: false, error: describeError(error) });
        return null;
      }
    },

    clearError: () => set({ error: null }),

    reset: () => set(initialState()),
  };
});
