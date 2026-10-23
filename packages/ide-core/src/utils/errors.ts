/** Shared error mapping: one source of truth for all error descriptions. */
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

export function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
