/**
 * Tests for the assistant store.
 *
 * The store is the only thing that turns a stream of frames into canvas state,
 * so these tests exist to prove the claims the drawer makes:
 *
 * - a `model_patch` reaches the project store, not just the timeline;
 * - a run that never saved cannot offer Build & Preview;
 * - a start that fails leaves the user with a message, not a spinner;
 * - a conflict is a choice (nothing was written), and a held lease is a notice
 *   with a countdown, not a failure.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { configurePlatform } from '../platform/registry';
import type { PlatformApi } from '../platform/types';
import type { AgentChatHandle, AgentChatRequest, AgentEvent } from '../types/agent';
import type { Project } from '../types/project';
import {
  useAgentStore,
  agentSupported,
  hasAgent,
  readCapabilities,
  resetAgentStore,
} from './agentStore';
import { useProjectStore } from './projectStore';

function emptyProject(): Project {
  return {
    id: 'p1',
    name: 'Expense tracker',
    slug: 'expense-tracker',
    description: null,
    version: '1.0.0',
    icon: null,
    settings: {},
    forms: {},
    entities: {},
    workflows: {},
    pages: {},
    themes: {},
    active_theme_id: null,
    plugins: [],
    environments: {},
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    published_at: null,
  } as unknown as Project;
}

function stubPlatform(overrides: Partial<PlatformApi> = {}): PlatformApi {
  return {
    kind: 'web',
    isDesktop: false,
    autoSeedSample: false,
    ...overrides,
  } as PlatformApi;
}

/**
 * A fake stream: `agentChat` records the callback and hands back a handle whose
 * `done` the test resolves.
 */
function fakeStream() {
  let emit: ((event: AgentEvent) => void) | null = null;
  let finish: () => void = () => {};
  const done = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const abort = vi.fn(() => finish());
  const chat = vi.fn(
    async (
      _projectId: string,
      _request: AgentChatRequest,
      onEvent: (event: AgentEvent) => void,
    ): Promise<AgentChatHandle> => {
      emit = onEvent;
      return { abort, done };
    },
  );
  return {
    chat,
    abort,
    send(event: AgentEvent) {
      if (!emit) throw new Error('the stream is not open yet');
      emit(event);
    },
    finish,
  };
}

/** Let `send()` get past its `await agentChat(...)`. */
async function tick(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe('agentStore', () => {
  beforeEach(() => {
    resetAgentStore();
    useAgentStore.setState({ prompt: '', open: false });
    useProjectStore.setState({
      project: emptyProject(),
      projectPath: 'api://projects/project-1',
      isDirty: false,
      error: null,
    });
  });

  it('is unavailable when the host has no agent', async () => {
    configurePlatform(stubPlatform());
    expect(agentSupported()).toBe(false);
    expect(await hasAgent()).toBe(false);
  });

  it('is available when the host can stream a run', async () => {
    configurePlatform(stubPlatform({ agentChat: vi.fn() as never }));
    expect(await hasAgent()).toBe(true);
  });

  it('is available in desktop local mode, which has no agentChat', async () => {
    // The exact case the boolean gate used to hide: a Tauri command is the
    // transport, so `agentChat` is absent and `local` is true (plan §15.4.3).
    configurePlatform(
      stubPlatform({
        agentCapabilities: async () => ({
          local: true,
          remote: false,
          providers: ['anthropic'],
          preview: false,
        }),
      }),
    );
    expect(await hasAgent()).toBe(true);
    expect(await readCapabilities()).toMatchObject({ local: true, preview: false });
  });

  it('is unavailable when the probe reports neither mode', async () => {
    configurePlatform(
      stubPlatform({
        agentCapabilities: async () => ({
          local: false,
          remote: false,
          providers: [],
          preview: false,
        }),
      }),
    );
    expect(await hasAgent()).toBe(false);
  });

  it('keeps the conversation id the server returns and sends it back', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'Build an expense tracker' });
    const pending = useAgentStore.getState().send('pipeline');
    await tick();
    stream.send({
      type: 'run_started',
      run_id: 'run-1',
      mode: 'pipeline',
      provider: 'anthropic',
      model: 'claude',
      conversation_id: 'conversation-1',
    });
    expect(useAgentStore.getState().conversationId).toBe('conversation-1');

    stream.send({ type: 'run_finished', status: 'succeeded', usage: { tokens_in: 1, tokens_out: 1, tool_calls: 0, turns: 1 }, revision: 2 });
    stream.finish();
    await pending;

    // The follow-up must continue the same thread.
    useAgentStore.setState({ prompt: 'now make the threshold 1000' });
    const second = useAgentStore.getState().send('pipeline');
    await tick();
    expect(stream.chat).toHaveBeenCalledTimes(2);
    expect(stream.chat.mock.calls[1][1]).toMatchObject({ conversation_id: 'conversation-1' });
    stream.finish();
    await second;
  });

  it('restores the last conversation on mount', async () => {
    configurePlatform(
      stubPlatform({
        agentChat: vi.fn() as never,
        agentConversations: async () => [
          {
            id: 'conversation-1',
            project_id: 'project-1',
            title: 'Build an expense tracker',
            message_count: 3,
            created_at: '2026-01-01T00:00:00Z',
            updated_at: '2026-01-01T00:01:00Z',
          },
        ],
        agentConversation: async () => ({
          id: 'conversation-1',
          project_id: 'project-1',
          title: 'Build an expense tracker',
          message_count: 3,
          created_at: '2026-01-01T00:00:00Z',
          updated_at: '2026-01-01T00:01:00Z',
          messages: [
            { seq: 1, role: 'user', text: 'Build an expense tracker', created_at: 't' },
            { seq: 2, role: 'assistant', text: 'Created the Expense entity.', created_at: 't' },
            { seq: 3, role: 'tool', text: '{"entity_id":"e1"}', tool: 'entity_create', created_at: 't' },
          ],
        }),
      }),
    );

    await useAgentStore.getState().restore();

    const state = useAgentStore.getState();
    expect(state.conversationId).toBe('conversation-1');
    expect(state.conversations).toHaveLength(1);
    expect(state.entries.map((entry) => entry.kind)).toEqual(['user', 'thought', 'tool']);
    expect(state.entries[0].text).toBe('Build an expense tracker');
    expect(state.entries[2].tool).toBe('entity_create');
  });

  it('starting a new conversation clears the timeline and the id', async () => {
    configurePlatform(
      stubPlatform({
        agentChat: vi.fn() as never,
        agentConversations: async () => [],
      }),
    );
    useAgentStore.setState({
      conversationId: 'conversation-1',
      entries: [{ id: 'x', kind: 'thought', text: 'old' }],
    });

    useAgentStore.getState().newConversation();

    expect(useAgentStore.getState().conversationId).toBeNull();
    expect(useAgentStore.getState().entries).toHaveLength(0);
  });

  it('refuses to start without a server-backed project', async () => {
    configurePlatform(stubPlatform({ agentChat: vi.fn() as never }));
    useProjectStore.setState({ projectPath: null });

    useAgentStore.setState({ prompt: 'build me something' });
    await useAgentStore.getState().send();

    expect(useAgentStore.getState().status).toBe('idle');
    expect(useAgentStore.getState().error).toMatch(/Open a project/);
  });

  it('streams a run and applies patches to the canvas', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'Build an expense tracker' });
    const pending = useAgentStore.getState().send('pipeline');
    await tick();

    expect(stream.chat).toHaveBeenCalledTimes(1);
    expect(stream.chat.mock.calls[0][1]).toEqual({
      prompt: 'Build an expense tracker',
      mode: 'pipeline',
    });

    stream.send({
      type: 'run_started',
      run_id: 'run-1',
      mode: 'pipeline',
      provider: 'anthropic',
      model: 'claude',
    });
    stream.send({
      type: 'step_started',
      step: 'data',
      title: 'Modelling the database',
    });
    stream.send({
      type: 'tool_call',
      tool: 'entity_create',
      args: { name: 'Expense' },
      step: 'data',
    });
    stream.send({
      type: 'model_patch',
      domain: 'entities',
      value: { e1: { id: 'e1', name: 'Expense' } },
      revision: 1,
    });
    stream.send({
      type: 'tool_result',
      tool: 'entity_create',
      ok: true,
      summary: '{"entity_id":"e1"}',
      duration_ms: 3,
    });
    stream.send({ type: 'validation', valid: true, issues: [] });
    stream.send({
      type: 'run_finished',
      status: 'succeeded',
      usage: { tokens_in: 10, tokens_out: 5, tool_calls: 1, turns: 2 },
      revision: 2,
    });
    stream.finish();
    await pending;

    const state = useAgentStore.getState();
    expect(state.status).toBe('succeeded');
    expect(state.runId).toBe('run-1');
    expect(state.model).toBe('anthropic/claude');
    expect(state.revision).toBe(2);
    expect(state.readyToPreview).toBe(true);
    expect(state.usage?.tokens_in).toBe(10);

    // The tool row settled instead of staying pending.
    const tool = state.entries.find((entry) => entry.kind === 'tool');
    expect(tool?.ok).toBe(true);
    expect(tool?.durationMs).toBe(3);

    // The canvas got the patch, and it is not dirty: the server saved it.
    const project = useProjectStore.getState();
    expect(project.project?.entities).toEqual({
      e1: { id: 'e1', name: 'Expense' },
    });
    expect(project.isDirty).toBe(false);
  });

  it('does not offer a preview when validation failed', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'build' });
    const pending = useAgentStore.getState().send('auto');
    await tick();

    stream.send({
      type: 'validation',
      valid: false,
      issues: [
        {
          domain: 'pages',
          object_id: 'p1',
          message: 'duplicate path',
          severity: 'error',
        },
      ],
    });
    stream.send({
      type: 'run_finished',
      status: 'validation_failed',
      usage: { tokens_in: 1, tokens_out: 1, tool_calls: 0, turns: 1 },
      revision: 1,
      error: '1 integrity error(s) remain; the project was not saved',
    });
    stream.finish();
    await pending;

    const state = useAgentStore.getState();
    expect(state.status).toBe('validation_failed');
    expect(state.readyToPreview).toBe(false);
    expect(state.issues).toHaveLength(1);
    expect(state.error).toMatch(/not saved/);
  });

  it('reports a start failure instead of hanging', async () => {
    configurePlatform(
      stubPlatform({
        agentChat: vi.fn().mockRejectedValue(new Error('503 the assistant is not configured')),
      }),
    );

    useAgentStore.setState({ prompt: 'build' });
    await useAgentStore.getState().send();

    const state = useAgentStore.getState();
    expect(state.status).toBe('failed');
    expect(state.error).toMatch(/503/);
  });

  it('marks a stream that ended without a result as failed', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'build' });
    const pending = useAgentStore.getState().send();
    await tick();

    // The connection drops: `done` resolves but no `run_finished` arrived.
    stream.finish();
    await pending;

    const state = useAgentStore.getState();
    expect(state.status).toBe('failed');
    expect(state.error).toMatch(/connection closed/i);
  });

  it('aborts the run when the user stops it', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'build' });
    const pending = useAgentStore.getState().send();
    await tick();

    useAgentStore.getState().cancel();
    stream.finish();
    await pending;

    expect(stream.abort).toHaveBeenCalled();
    expect(useAgentStore.getState().status).toBe('cancelled');
  });

  it('adopts a full project snapshot without marking it dirty', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'build' });
    const pending = useAgentStore.getState().send();
    await tick();

    const snapshot = { ...emptyProject(), name: 'Renamed by the agent' };
    stream.send({ type: 'project_updated', project: snapshot, revision: 4 });
    stream.finish();
    await pending;

    const project = useProjectStore.getState();
    expect(project.project?.name).toBe('Renamed by the agent');
    expect(project.isDirty).toBe(false);
    expect(useAgentStore.getState().revision).toBe(4);
  });

  it('streams text into a buffer, not the timeline', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'build' });
    const pending = useAgentStore.getState().send();
    await tick();

    stream.send({
      type: 'run_started',
      run_id: 'run-1',
      mode: 'auto',
      provider: 'anthropic',
      model: 'claude',
    });
    const rows = useAgentStore.getState().entries.length;

    stream.send({ type: 'agent_delta', text: 'Creating ', step: 'auto', seq: 1 });
    stream.send({ type: 'agent_delta', text: 'the entity.', step: 'auto', seq: 2 });

    const mid = useAgentStore.getState();
    expect(mid.streaming).toBe('Creating the entity.');
    // A row per delta would re-render the whole timeline many times a second.
    expect(mid.entries).toHaveLength(rows);

    // The complete text then arrives as narration: one row, buffer cleared.
    stream.send({ type: 'agent_thought', text: 'Creating the entity.' });
    const after = useAgentStore.getState();
    expect(after.streaming).toBe('');
    expect(after.entries.filter((entry) => entry.kind === 'thought')).toHaveLength(1);

    stream.finish();
    await pending;
  });

  it('caps the streaming buffer so a runaway generation cannot grow the store', () => {
    const chunk = 'x'.repeat(4_096);
    const store = useAgentStore.getState();
    for (let seq = 1; seq <= 3; seq += 1) {
      store.applyEvent({ type: 'agent_delta', text: chunk, step: 'auto', seq });
    }
    const { streaming } = useAgentStore.getState();
    expect(streaming.length).toBeLessThanOrEqual(8 * 1_024 + 1);
    expect(streaming.endsWith('…')).toBe(true);
  });

  it('freezes streamed text when a tool call interrupts it', () => {
    const store = useAgentStore.getState();
    store.applyEvent({ type: 'agent_delta', text: 'Reading the model…', step: 'data', seq: 1 });
    store.applyEvent({ type: 'tool_call', tool: 'project_get', args: {}, step: 'data' });

    const state = useAgentStore.getState();
    // Nothing the user watched may disappear just because the turn moved on.
    expect(state.streaming).toBe('');
    expect(state.entries.find((entry) => entry.kind === 'thought')?.text).toBe(
      'Reading the model…',
    );
  });

  it('reports a conflict as a choice, and never offers a preview', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'add a note entity' });
    const pending = useAgentStore.getState().send();
    await tick();

    stream.send({
      type: 'conflict',
      objects: [{ domain: 'entities', object_id: 'e1' }],
      current_revision: 7,
    });
    stream.send({
      type: 'run_finished',
      status: 'conflict',
      usage: { tokens_in: 3, tokens_out: 2, tool_calls: 1, turns: 1 },
      revision: 7,
      rebased: false,
    });
    stream.finish();
    await pending;

    const state = useAgentStore.getState();
    expect(state.status).toBe('conflict');
    expect(state.conflicts).toEqual([{ domain: 'entities', object_id: 'e1' }]);
    // Nothing was written, so there is no revision to preview.
    expect(state.readyToPreview).toBe(false);
    expect(state.entries.some((entry) => entry.kind === 'conflict')).toBe(true);
  });

  it('says when a stale save was merged instead of failing', async () => {
    const stream = fakeStream();
    configurePlatform(stubPlatform({ agentChat: stream.chat as never }));

    useAgentStore.setState({ prompt: 'add a note entity' });
    const pending = useAgentStore.getState().send();
    await tick();

    stream.send({
      type: 'run_finished',
      status: 'succeeded',
      usage: { tokens_in: 3, tokens_out: 2, tool_calls: 1, turns: 1 },
      revision: 9,
      rebased: true,
    });
    stream.finish();
    await pending;

    const state = useAgentStore.getState();
    expect(state.status).toBe('succeeded');
    expect(state.rebased).toBe(true);
    expect(state.readyToPreview).toBe(true);
    expect(state.conflicts).toHaveLength(0);
  });

  it('treats a held lease as a notice with a countdown, not a failure', async () => {
    const locked = Object.assign(new Error('another run is generating this project'), {
      code: 'project_locked',
      status: 409,
      details: {
        error: 'project_locked',
        message: 'another run (run-9) is generating this project; it expires in 42s',
        holder: { run_id: 'run-9' },
        expires_in_seconds: 42,
      },
    });
    configurePlatform(stubPlatform({ agentChat: vi.fn().mockRejectedValue(locked) as never }));

    useAgentStore.setState({ prompt: 'build' });
    await useAgentStore.getState().send();

    const state = useAgentStore.getState();
    // Not `failed`: nothing broke, another tab simply holds the project.
    expect(state.status).toBe('idle');
    expect(state.error).toBeNull();
    expect(state.lock?.holder).toBe('run-9');
    expect(state.lock?.expiresAt).toBeGreaterThan(Date.now());
  });

  it('retries the prompt that was sent, not whatever is in the composer', async () => {
    const stream = fakeStream();
    const locked = Object.assign(new Error('locked'), {
      code: 'project_locked',
      details: { holder: { run_id: 'run-9' }, expires_in_seconds: 5 },
    });
    const chat = vi.fn().mockRejectedValueOnce(locked).mockImplementation(stream.chat);
    configurePlatform(stubPlatform({ agentChat: chat as never }));

    useAgentStore.setState({ prompt: 'build an expense tracker' });
    await useAgentStore.getState().send();
    expect(useAgentStore.getState().lock).not.toBeNull();

    // The composer is editable while a lock is showing, so retry must not read it.
    useAgentStore.setState({ prompt: 'something else entirely' });
    const pending = useAgentStore.getState().retry();
    await tick();

    expect(chat).toHaveBeenCalledTimes(2);
    expect(chat.mock.calls[1][1]).toEqual({
      prompt: 'build an expense tracker',
      mode: 'pipeline',
    });
    expect(useAgentStore.getState().lock).toBeNull();

    stream.finish();
    await pending;
  });

  it('starts in local mode, where the project is the file on disk', async () => {
    const stream = fakeStream();
    const updateProject = vi.fn(async () => undefined);
    const capabilities = {
      local: true,
      remote: false,
      providers: ['anthropic'],
      preview: false,
    };
    configurePlatform(
      stubPlatform({
        agentChat: stream.chat as never,
        updateProject,
        agentCapabilities: async () => capabilities,
      }),
    );
    useAgentStore.setState({ capabilities });
    // A desktop project that has never been saved: no server id, no path, and
    // pending edits that must not pop a Save dialog.
    useProjectStore.setState({ projectPath: null, isDirty: true });

    useAgentStore.setState({ prompt: 'build an expense tracker' });
    const pending = useAgentStore.getState().send('pipeline');
    await tick();

    expect(stream.chat).toHaveBeenCalledTimes(1);
    // Empty id: the local host resolves the project from its own working copy.
    expect(stream.chat.mock.calls[0][0]).toBe('');
    expect(updateProject).not.toHaveBeenCalled();

    stream.send({
      type: 'run_finished',
      status: 'succeeded',
      usage: { tokens_in: 1, tokens_out: 1, tool_calls: 0, turns: 1 },
      revision: 1,
    });
    stream.finish();
    await pending;
    expect(useAgentStore.getState().status).toBe('succeeded');
  });

  it('re-probes the host after the key is saved', async () => {
    // `local` depends on a stored key, which the user can add without a reload,
    // so the once-per-session cache has to be invalidatable (plan §15.4.3).
    let local = false;
    configurePlatform(
      stubPlatform({
        agentCapabilities: async () => ({ local, remote: false, providers: [], preview: false }),
      }),
    );

    await useAgentStore.getState().probe();
    expect(useAgentStore.getState().capabilities?.local).toBe(false);

    local = true;
    await useAgentStore.getState().reprobe();
    expect(useAgentStore.getState().capabilities?.local).toBe(true);
  });
});
