/**
 * React context that exposes the active `PlatformApi` to the shared IDE and,
 * as a side effect, registers it in the module registry so non-React code can
 * reach it too.
 */

import { createContext, useContext, useMemo, type ReactNode } from 'react';

import { configurePlatform } from './registry';
import type { PlatformApi } from './types';

const PlatformContext = createContext<PlatformApi | null>(null);

export interface PlatformProviderProps {
  platform: PlatformApi;
  children: ReactNode;
}

/**
 * Install the host platform for the whole IDE tree. Host applications render
 * this once near the root.
 */
export function PlatformProvider({ platform, children }: PlatformProviderProps) {
  configurePlatform(platform);
  const value = useMemo(() => platform, [platform]);
  return (
    <PlatformContext.Provider value={value}>{children}</PlatformContext.Provider>
  );
}

/** Access the active platform from a component. */
export function usePlatform(): PlatformApi {
  const platform = useContext(PlatformContext);
  if (!platform) {
    throw new Error('usePlatform must be used within <PlatformProvider>.');
  }
  return platform;
}
