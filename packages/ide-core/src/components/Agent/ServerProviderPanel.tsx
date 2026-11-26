/**
 * Settings → AI for a host that stores provider keys server-side.
 *
 * The desktop shell owns exactly one provider and keeps it in its own app-data
 * file, so it renders `AgentSettingsPanel`'s form instead. A server deployment
 * has many rows in `agent_api_keys`, so this panel is a list plus an "add"
 * form, backed by `POST/PATCH/DELETE /api/v1/agent/providers`.
 *
 * Three rules this component keeps:
 *
 * 1. **A key is write-only.** It is typed into local state, sent once, and
 *    cleared immediately. No field renders a stored credential, because the
 *    server has no endpoint that returns one.
 * 2. **The server decides usability.** `unavailable` comes from the server's
 *    own catalog, so the panel shows the reason rather than guessing from
 *    `enabled` or from whether a key looks present.
 * 3. **A failed test is an answer, not an error.** `ok: false` renders inline
 *    with the provider's own words; only a rejected promise becomes a banner.
 */

import { useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  Check,
  ChevronLeft,
  KeyRound,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
  X,
} from 'lucide-react';
import { getPlatform } from '../../platform/registry';
import { useAgentStore } from '../../stores/agentStore';
import type { AgentPlatform, AgentProbeResult } from '../../types/agent';

/** What a status badge says, and how it looks. */
const STATUS_STYLE: Record<string, string> = {
  healthy:
    'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-300',
  invalid: 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-300',
  error: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300',
  unknown:
    'bg-gray-100 text-gray-600 dark:bg-slate-700 dark:text-gray-300',
};

const FIELD =
  'mt-1 w-full rounded-lg border border-gray-300 bg-white px-2 py-1.5 text-sm text-gray-900 focus:border-primary-500 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-white';
const LABEL =
  'text-[11px] font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400';
const BUTTON =
  'flex items-center gap-1 rounded-lg border border-gray-300 px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-600 dark:text-gray-200 dark:hover:bg-slate-700';

export function ServerProviderPanel({
  onBack,
  onSaved,
}: {
  onBack: () => void;
  onSaved: () => void;
}) {
  const platform = getPlatform();
  const providers = useAgentStore((s) => s.providers);
  const reason = useAgentStore((s) => s.providerReason);
  const selectedId = useAgentStore((s) => s.providerId);
  const loadProviders = useAgentStore((s) => s.loadProviders);
  const addProvider = useAgentStore((s) => s.addProvider);
  const updateProvider = useAgentStore((s) => s.updateProvider);
  const removeProvider = useAgentStore((s) => s.removeProvider);
  const testProvider = useAgentStore((s) => s.testProvider);

  const [platforms, setPlatforms] = useState<AgentPlatform[]>([]);
  const [adding, setAdding] = useState(false);
  const [draftPlatform, setDraftPlatform] = useState('');
  const [draftKey, setDraftKey] = useState('');
  const [draftBaseUrl, setDraftBaseUrl] = useState('');
  const [draftLabel, setDraftLabel] = useState('');
  /** Row id, or `add` — which control is mid-flight. */
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The last test result, shown under the row it belongs to. */
  const [probe, setProbe] = useState<{ id: string; result: AgentProbeResult } | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      if (typeof platform.agentPlatforms !== 'function') return;
      try {
        const list = await platform.agentPlatforms();
        if (cancelled) return;
        setPlatforms(list);
        // Pre-select the first platform that has no row yet: the common case is
        // "add one more", and a configured platform is already in the list.
        const configured = new Set(providers.map((row) => row.platform));
        const firstFree = list.find((row) => !configured.has(row.platform));
        setDraftPlatform(firstFree?.platform ?? list[0]?.platform ?? '');
      } catch (cause) {
        if (!cancelled) setError(String(cause));
      }
    })();
    return () => {
      cancelled = true;
    };
    // `providers` is read for the initial suggestion only; re-running on every
    // list change would reset the form while it is being filled in.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [platform]);

  const spec = useMemo(
    () => platforms.find((row) => row.platform === draftPlatform) ?? null,
    [platforms, draftPlatform],
  );

  const run = async (kind: string, action: () => Promise<string | null>): Promise<void> => {
    setBusy(kind);
    setNote(null);
    setError(null);
    try {
      const message = await action();
      if (message) setNote(message);
      onSaved();
    } catch (cause) {
      setError(typeof cause === 'string' ? cause : String(cause));
    } finally {
      setBusy(null);
    }
  };

  const submit = () =>
    run('add', async () => {
      await addProvider({
        platform: draftPlatform,
        // Absent, not empty: a keyless platform must not be sent a blank key,
        // and the server refuses an empty one for a platform that needs it.
        api_key: draftKey.trim() || undefined,
        label: draftLabel.trim() || undefined,
        base_url: draftBaseUrl.trim() || undefined,
      });
      setDraftKey('');
      setDraftBaseUrl('');
      setDraftLabel('');
      setAdding(false);
      return 'Provider added.';
    });

  const test = (id: string) =>
    run(id, async () => {
      const result = await testProvider(id);
      setProbe({ id, result });
      return null;
    });

  const toggle = (id: string, enabled: boolean) =>
    run(id, async () => {
      await updateProvider(id, { enabled });
      return enabled ? 'Provider enabled.' : 'Provider disabled.';
    });

  const remove = (id: string, label: string) =>
    run(id, async () => {
      await removeProvider(id);
      if (probe?.id === id) setProbe(null);
      return `${label} was removed.`;
    });

  // A platform that ships no default endpoint needs one from the operator.
  const needsBaseUrl = spec !== null && spec.base_url === '';
  const needsKey = spec !== null && !spec.keyless;

  return (
    <>
      <header className="flex items-center gap-2 border-b border-gray-200 px-4 py-3 dark:border-slate-700">
        <button
          type="button"
          onClick={onBack}
          className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-slate-700"
          aria-label="Back to the conversation"
        >
          <ChevronLeft className="w-4 h-4" />
        </button>
        <h3 className="text-sm font-semibold text-gray-900 dark:text-white">Settings → AI</h3>
        <button
          type="button"
          onClick={() => void loadProviders()}
          className="ml-auto rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-slate-700"
          aria-label="Reload the provider list"
        >
          <RefreshCw className="w-3.5 h-3.5" />
        </button>
      </header>

      <div className="flex-1 space-y-3 overflow-y-auto px-4 py-3">
        <p className="text-xs text-gray-500 dark:text-gray-400">
          Keys are sealed on the server and never returned to this page. A provider marked unusable
          says why, so a failed run is never the first you hear of a problem.
        </p>

        {providers.length === 0 && (
          <p className="rounded bg-gray-50 px-2 py-2 text-xs text-gray-600 dark:bg-slate-800 dark:text-gray-300">
            {reason ?? 'No provider is configured yet.'}
          </p>
        )}

        <ul className="space-y-2">
          {providers.map((row) => (
            <li
              key={row.id}
              className="rounded-lg border border-gray-200 px-3 py-2 dark:border-slate-700"
            >
              <div className="flex items-start gap-2">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="truncate text-sm font-medium text-gray-900 dark:text-white">
                      {row.label}
                    </span>
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                        STATUS_STYLE[row.status] ?? STATUS_STYLE.unknown
                      }`}
                    >
                      {row.status}
                    </span>
                    {row.keyless && (
                      <span className="rounded bg-sky-100 px-1.5 py-0.5 text-[10px] font-medium text-sky-700 dark:bg-sky-900/30 dark:text-sky-300">
                        no key needed
                      </span>
                    )}
                    {row.id === selectedId && (
                      <span className="rounded bg-primary-100 px-1.5 py-0.5 text-[10px] font-medium text-primary-700 dark:bg-primary-900/30 dark:text-primary-300">
                        in use
                      </span>
                    )}
                  </div>
                  <p className="mt-0.5 truncate font-mono text-[11px] text-gray-500 dark:text-gray-400">
                    {row.platform}
                    {row.model ? ` · ${row.model}` : ''}
                    {row.base_url ? ` · ${row.base_url}` : ''}
                  </p>
                  {row.unavailable && (
                    <p className="mt-1 flex items-start gap-1 text-[11px] text-amber-700 dark:text-amber-400">
                      <AlertTriangle className="mt-0.5 w-3 h-3 flex-shrink-0" />
                      <span className="min-w-0 break-words">{row.unavailable}</span>
                    </p>
                  )}
                  {probe?.id === row.id && (
                    <p
                      className={`mt-1 break-words text-[11px] ${
                        probe.result.ok
                          ? 'text-green-700 dark:text-green-400'
                          : 'text-red-700 dark:text-red-400'
                      }`}
                    >
                      {probe.result.ok
                        ? `Answered in ${probe.result.latency_ms} ms.`
                        : `${probe.result.status}: ${probe.result.detail}`}
                    </p>
                  )}
                </div>
                <div className="flex flex-shrink-0 items-center gap-1">
                  <button
                    type="button"
                    onClick={() => void test(row.id)}
                    disabled={busy !== null}
                    title="Test connection"
                    className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 disabled:opacity-50 dark:hover:bg-slate-700"
                  >
                    {busy === row.id ? (
                      <Loader2 className="w-3.5 h-3.5 animate-spin" />
                    ) : (
                      <KeyRound className="w-3.5 h-3.5" />
                    )}
                  </button>
                  <button
                    type="button"
                    onClick={() => void toggle(row.id, !row.enabled)}
                    disabled={busy !== null}
                    title={row.enabled ? 'Disable' : 'Enable'}
                    className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 disabled:opacity-50 dark:hover:bg-slate-700"
                  >
                    {row.enabled ? <Check className="w-3.5 h-3.5" /> : <X className="w-3.5 h-3.5" />}
                  </button>
                  <button
                    type="button"
                    onClick={() => void remove(row.id, row.label)}
                    disabled={busy !== null}
                    title="Remove"
                    className="rounded p-1 text-red-500 hover:bg-red-50 disabled:opacity-50 dark:hover:bg-red-900/20"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>
            </li>
          ))}
        </ul>

        {!adding ? (
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="flex w-full items-center justify-center gap-1 rounded-lg border border-dashed border-gray-300 px-3 py-2 text-sm font-medium text-gray-600 hover:bg-gray-50 dark:border-slate-600 dark:text-gray-300 dark:hover:bg-slate-700"
          >
            <Plus className="w-3.5 h-3.5" /> Add a provider
          </button>
        ) : (
          <div className="space-y-2 rounded-lg border border-gray-200 px-3 py-2 dark:border-slate-700">
            <label className="block">
              <span className={LABEL}>Platform</span>
              <select
                value={draftPlatform}
                onChange={(event) => setDraftPlatform(event.target.value)}
                className={FIELD}
              >
                {platforms.map((row) => (
                  <option key={row.platform} value={row.platform}>
                    {row.label}
                  </option>
                ))}
              </select>
            </label>

            {spec?.note && (
              <p className="text-[11px] text-gray-500 dark:text-gray-400">{spec.note}</p>
            )}

            {needsKey && (
              <label className="block">
                <span className={LABEL}>
                  {spec?.account_scoped ? 'Account id : API token' : 'API key'}
                </span>
                <input
                  type="password"
                  value={draftKey}
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => setDraftKey(event.target.value)}
                  placeholder={spec?.account_scoped ? 'account_id:api_token' : 'Required'}
                  className={`${FIELD} font-mono`}
                />
              </label>
            )}

            {needsBaseUrl && (
              <label className="block">
                <span className={LABEL}>Base URL</span>
                <input
                  value={draftBaseUrl}
                  onChange={(event) => setDraftBaseUrl(event.target.value)}
                  placeholder="https://gateway.example.com/v1"
                  className={`${FIELD} font-mono`}
                />
              </label>
            )}

            <label className="block">
              <span className={LABEL}>Label (optional)</span>
              <input
                value={draftLabel}
                onChange={(event) => setDraftLabel(event.target.value)}
                placeholder={spec?.label ?? 'Display name'}
                className={FIELD}
              />
            </label>

            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => void submit()}
                disabled={busy !== null || !draftPlatform}
                className="flex items-center gap-1 rounded-lg bg-primary-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-primary-700 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {busy === 'add' ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <Plus className="w-3.5 h-3.5" />
                )}
                Add
              </button>
              <button
                type="button"
                onClick={() => {
                  setAdding(false);
                  setDraftKey('');
                  setDraftBaseUrl('');
                  setDraftLabel('');
                  setError(null);
                }}
                disabled={busy !== null}
                className={BUTTON}
              >
                Cancel
              </button>
            </div>
          </div>
        )}

        {note && (
          <p className="rounded bg-green-50 px-2 py-1.5 text-xs text-green-700 dark:bg-green-900/20 dark:text-green-300">
            {note}
          </p>
        )}

        {error && (
          <p className="flex items-start gap-1.5 rounded bg-red-50 px-2 py-1.5 text-xs text-red-700 dark:bg-red-900/20 dark:text-red-300">
            <AlertTriangle className="mt-0.5 w-3.5 h-3.5 flex-shrink-0" />
            <span className="min-w-0 break-words">{error}</span>
          </p>
        )}
      </div>
    </>
  );
}
