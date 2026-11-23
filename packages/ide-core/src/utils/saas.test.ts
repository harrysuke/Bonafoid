import { describe, expect, it } from 'vitest';
import { formatMoney, redactSecret, statusLabel } from './saas';

describe('saas helpers', () => {
  it('redacts every occurrence of a submitted secret', () => {
    const message = 'rejected secret-123 and secret-123 again';
    const redacted = redactSecret(message, 'secret-123');
    expect(redacted).toBe('rejected ***REDACTED*** and ***REDACTED*** again');
    expect(redacted).not.toContain('secret-123');
  });

  it('leaves the message alone when no secret was supplied', () => {
    expect(redactSecret('plain message')).toBe('plain message');
    expect(redactSecret('plain message', '')).toBe('plain message');
  });

  it('humanizes snake_case and kebab-case statuses', () => {
    expect(statusLabel('past_due')).toBe('Past due');
    expect(statusLabel('ci-succeeded')).toBe('Ci succeeded');
    expect(statusLabel('active')).toBe('Active');
    expect(statusLabel('')).toBe('');
  });

  it('formats integer cents as currency', () => {
    // Intl uses the host locale, so assert on the digits rather than separators.
    expect(formatMoney(1900, 'USD')).toMatch(/19/);
    expect(formatMoney(5, 'EUR')).toMatch(/0[.,]05/);
  });
});
