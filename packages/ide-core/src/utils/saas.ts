/**
 * Small pure helpers shared by the SaaS panels.
 *
 * Keeping them here (rather than inside components) makes the security-relevant
 * one — secret redaction — cheap to test without a DOM.
 */

/** Replace every occurrence of a submitted secret in a message. */
export function redactSecret(message: string, secret?: string): string {
  if (!secret) return message;
  return message.split(secret).join('***REDACTED***');
}

/** Humanize a wire status such as `past_due` into `Past due`. */
export function statusLabel(value: string): string {
  const words = value.replace(/[_-]+/g, ' ').trim().toLowerCase();
  if (!words) return '';
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** Render integer minor units as a currency amount. */
export function formatMoney(cents: number, currency = 'USD'): string {
  const amount = (cents / 100).toFixed(2);
  try {
    return new Intl.NumberFormat(undefined, {
      style: 'currency',
      currency,
    }).format(cents / 100);
  } catch {
    return `${currency} ${amount}`;
  }
}
