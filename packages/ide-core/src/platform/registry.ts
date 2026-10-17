/**
 * Module-level platform registry.
 *
 * React components read the platform through `usePlatform()` (context), but
 * non-React code such as the zustand stores cannot use hooks. The registry
 * gives that code a stable accessor that `<PlatformProvider>` populates on
 * first render.
 */

import type { PlatformApi } from './types';

let current: PlatformApi | null = null;

/** Register the active platform. Idempotent and safe to call on every render. */
export function configurePlatform(platform: PlatformApi): void {
  current = platform;
}

/** Return the active platform, or throw if the app forgot to install one. */
export function getPlatform(): PlatformApi {
  if (!current) {
    throw new Error(
      'Platform not configured. Wrap the app in <PlatformProvider>.',
    );
  }
  return current;
}

/** True once a platform has been registered. */
export function hasPlatform(): boolean {
  return current !== null;
}
