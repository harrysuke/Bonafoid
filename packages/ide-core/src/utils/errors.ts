/**
 * Shared error mapping: one source of truth for every error the IDE shows.
 *
 * Three near-identical helpers used to live in `projectStore`, `pluginStore`
 * and `WorkflowBuilder`, and the only structured mapper (`toError` in the web
 * API client) was module-private. Anything that renders a failure now goes
 * through `describeError`, so a server error reads the same everywhere.
 *
 * The server's contract is stable: every error body is
 * `{ "error": "<stable code>", "message": "<human text>" }` — see
 * `crates/ide-server/src/error.rs`.
 */
import { useUIStore } from '../stores/uiStore';

/** An error carrying the server's HTTP status and stable error code. */
export class ApiRequestError extends Error {
  readonly status: number;
  readonly code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = 'ApiRequestError';
    this.status = status;
    this.code = code;
  }
}

/**
 * A human-readable message for anything that was thrown.
 *
 * `ApiRequestError` already carries the server's message, which is written for
 * a user; a plain `Error` carries whatever the transport produced; anything
 * else is stringified rather than rendered as `[object Object]`.
 */
export function describeError(error: unknown): string {
  if (error instanceof ApiRequestError) {
    return error.message || `${error.code} (HTTP ${error.status})`;
  }
  if (error instanceof Error) {
    return error.message || error.name;
  }
  if (typeof error === 'string') {
    return error;
  }
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message) return message;
  }
  if (error === undefined || error === null) {
    return 'Unknown error';
  }
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

/**
 * Show a failure as a toast and return the message that was shown.
 *
 * Returning the message lets a caller keep its own inline error state in sync
 * without describing the error twice.
 */
export function notifyError(error: unknown, context?: string): string {
  const message = describeError(error);
  useUIStore.getState().addNotification({
    type: 'error',
    title: context ?? 'Something went wrong',
    message,
  });
  return message;
}

/** Show a success toast. */
export function notifySuccess(title: string, message?: string): void {
  useUIStore.getState().addNotification({ type: 'success', title, message });
}

/** Show an informational toast. */
export function notifyInfo(title: string, message?: string): void {
  useUIStore.getState().addNotification({ type: 'info', title, message });
}
