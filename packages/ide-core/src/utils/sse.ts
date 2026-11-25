/**
 * Minimal Server-Sent Events decoder for the assistant stream.
 *
 * Written by hand for three reasons:
 *
 * - `EventSource` cannot POST, and the prompt is a body;
 * - the wire contract is one JSON object per frame, so a full SSE
 *   implementation (event ids, reconnection, last-event-id) would be dead code
 *   that still has to be tested;
 * - a parser that is a plain function is testable without a browser, which the
 *   `fetch` + `ReadableStream` plumbing is not.
 *
 * The format implemented here is the subset the server emits: `event:` and
 * `data:` lines, `:` comments for keep-alives, frames separated by a blank
 * line. Multi-line `data:` is joined with `\n` as the spec requires.
 */

import type { AgentEvent } from '../types/agent';

/** A frame separator: `\n\n`, or `\r\n\r\n` from a proxy that rewrites them. */
const FRAME_END = /\r?\n\r?\n/;

/**
 * Parse one frame (without its trailing blank line).
 *
 * Returns `null` for keep-alives, unknown fields and anything that is not a
 * tagged agent event — a malformed frame must not kill the stream.
 */
export function parseSseFrame(frame: string): AgentEvent | null {
  const data: string[] = [];

  for (const rawLine of frame.split(/\r?\n/)) {
    // A line starting with a colon is a comment. The server sends `: ping`
    // while the model is thinking, which keeps proxies from timing out.
    if (rawLine.startsWith(':')) continue;

    const colon = rawLine.indexOf(':');
    const field = colon === -1 ? rawLine : rawLine.slice(0, colon);
    let value = colon === -1 ? '' : rawLine.slice(colon + 1);
    // A single leading space after the colon is part of the syntax.
    if (value.startsWith(' ')) value = value.slice(1);

    if (field === 'data') data.push(value);
  }

  if (data.length === 0) return null;

  const payload = data.join('\n');
  try {
    const parsed = JSON.parse(payload) as AgentEvent;
    // The server tags every payload, so a missing tag means this is not one of
    // ours (or the payload was truncated mid-frame).
    return parsed && typeof parsed === 'object' && typeof parsed.type === 'string' ? parsed : null;
  } catch {
    return null;
  }
}

export interface SseDecoder {
  /** Feed decoded text. Complete frames are emitted immediately. */
  push(chunk: string): void;
  /** Emit a trailing frame that never got its blank line. */
  flush(): void;
}

/**
 * A streaming decoder. Text arrives in arbitrary chunks — a chunk boundary can
 * fall inside a JSON string — so frames are only parsed once complete.
 */
export function createSseDecoder(onEvent: (event: AgentEvent) => void): SseDecoder {
  let buffer = '';

  const drain = (flush: boolean): void => {
    for (;;) {
      const match = FRAME_END.exec(buffer);
      if (!match || match.index === undefined) break;
      const frame = buffer.slice(0, match.index);
      buffer = buffer.slice(match.index + match[0].length);
      const event = parseSseFrame(frame);
      if (event) onEvent(event);
    }
    if (flush && buffer.trim().length > 0) {
      const event = parseSseFrame(buffer);
      buffer = '';
      if (event) onEvent(event);
    }
  };

  return {
    push(chunk: string): void {
      buffer += chunk;
      drain(false);
    },
    flush(): void {
      drain(true);
    },
  };
}
