/**
 * Tests for the SSE decoder.
 *
 * The decoder is the seam between the server's frame format and the drawer. A
 * chunk boundary can fall inside a JSON string, and a keep-alive comment can
 * arrive mid-frame; both used to be the kind of thing only a real browser
 * exercised.
 */

import { describe, expect, it, vi } from 'vitest';
import { createSseDecoder, parseSseFrame } from './sse';
import type { AgentEvent } from '../types/agent';

describe('parseSseFrame', () => {
  it('reads the data payload', () => {
    const frame =
      'event: tool_call\ndata: {"type":"tool_call","tool":"entity_create","args":{},"step":"data"}';
    const event = parseSseFrame(frame);
    expect(event?.type).toBe('tool_call');
  });

  it('ignores keep-alive comments', () => {
    expect(parseSseFrame(': ping')).toBeNull();
  });

  it('joins multi-line data with a newline', () => {
    const frame = 'data: {"type":"agent_thought",\ndata: "text":"two lines"}';
    const event = parseSseFrame(frame) as AgentEvent | null;
    expect(event?.type).toBe('agent_thought');
  });

  it('returns null for a payload that is not a tagged event', () => {
    expect(parseSseFrame('data: {"hello":true}')).toBeNull();
    expect(parseSseFrame('data: not json')).toBeNull();
  });
});

describe('createSseDecoder', () => {
  it('emits a frame only once it is complete', () => {
    const seen: AgentEvent[] = [];
    const decoder = createSseDecoder((event) => seen.push(event));

    decoder.push('event: step_started\ndata: {"type":"step_started","step":"data"');
    expect(seen).toHaveLength(0);

    decoder.push(',"title":"Data"}\n\n');
    expect(seen).toHaveLength(1);
    expect(seen[0].type).toBe('step_started');
  });

  it('emits several frames from one chunk', () => {
    const seen: AgentEvent[] = [];
    const decoder = createSseDecoder((event) => seen.push(event));

    decoder.push(
      'data: {"type":"step_started","step":"data","title":"Data"}\n\n' +
        'data: {"type":"step_started","step":"ui","title":"UI"}\n\n',
    );

    expect(seen.map((event) => event.type)).toEqual(['step_started', 'step_started']);
  });

  it('accepts CRLF separators from a rewriting proxy', () => {
    const seen: AgentEvent[] = [];
    const decoder = createSseDecoder((event) => seen.push(event));

    decoder.push('data: {"type":"agent_thought","text":"hi"}\r\n\r\n');

    expect(seen).toHaveLength(1);
  });

  it('skips a keep-alive between frames without breaking the stream', () => {
    const onEvent = vi.fn();
    const decoder = createSseDecoder(onEvent);

    decoder.push(': ping\n\n');
    decoder.push('data: {"type":"agent_thought","text":"hi"}\n\n');

    expect(onEvent).toHaveBeenCalledTimes(1);
  });

  it('flushes a trailing frame that never got its blank line', () => {
    const seen: AgentEvent[] = [];
    const decoder = createSseDecoder((event) => seen.push(event));

    decoder.push('data: {"type":"run_finished","status":"succeeded"}');
    expect(seen).toHaveLength(0);

    decoder.flush();
    expect(seen).toHaveLength(1);
    expect(seen[0].type).toBe('run_finished');
  });

  it('survives a malformed frame and keeps decoding', () => {
    const seen: AgentEvent[] = [];
    const decoder = createSseDecoder((event) => seen.push(event));

    decoder.push('data: {broken\n\n');
    decoder.push('data: {"type":"agent_thought","text":"still here"}\n\n');

    expect(seen).toHaveLength(1);
    expect(seen[0].type).toBe('agent_thought');
  });
});
