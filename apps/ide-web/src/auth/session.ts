/**
 * Browser session state for the IDE server.
 *
 * Deliberately framework-free: the React shell subscribes with
 * `useSyncExternalStore`, and the platform adapter reads `currentUser()`
 * directly. Keeping it outside React avoids a provider-ordering problem with
 * `<PlatformProvider platform={webPlatform}>`.
 */

import { authApi, onUnauthorized, rememberCsrfToken } from '../api/client';
import type { ApiUser } from '../api/client';

export type SessionStatus = 'unknown' | 'authenticated' | 'anonymous';

let status: SessionStatus = 'unknown';
let user: ApiUser | null = null;
const listeners = new Set<() => void>();

function emit(): void {
  listeners.forEach((listener) => listener());
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getStatus(): SessionStatus {
  return status;
}

export function currentUser(): ApiUser | null {
  return user;
}

function setSession(next: { status: SessionStatus; user: ApiUser | null }): void {
  status = next.status;
  user = next.user;
  emit();
}

// A refresh failure anywhere in the app drops us back to the login screen.
onUnauthorized(() => setSession({ status: 'anonymous', user: null }));

/** Resolve the session once at app start (`GET /auth/me`, refresh on 401). */
export async function bootstrapSession(): Promise<SessionStatus> {
  const existing = await authApi.me();
  setSession(existing ? { status: 'authenticated', user: existing } : { status: 'anonymous', user: null });
  return status;
}

export async function login(email: string, password: string): Promise<void> {
  const result = await authApi.login(email, password);
  rememberCsrfToken(result.csrf_token);
  setSession({ status: 'authenticated', user: result.user });
}

export async function register(email: string, password: string): Promise<void> {
  const result = await authApi.register(email, password);
  rememberCsrfToken(result.csrf_token);
  setSession({ status: 'authenticated', user: result.user });
}

export async function logout(): Promise<void> {
  await authApi.logout();
  setSession({ status: 'anonymous', user: null });
}