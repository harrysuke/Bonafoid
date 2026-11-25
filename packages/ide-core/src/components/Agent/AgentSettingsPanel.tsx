/**
 * Settings → AI, inside the drawer.
 *
 * The desktop shell is the only host that owns a provider, so this panel only
 * renders where `platform.agentSettings` exists. The web host keeps its key
 * server-side and shows nothing here.
 *
 * Two rules this component keeps:
 *
 * 1. **The key is write-only.** It is typed into local state, sent once, and
 *    cleared from state immediately after. Nothing renders `settings.has_key`
 *    except a placeholder, because the host never returns the key.
 * 2. **Saving re-probes the host.** `local` capability depends on whether a key
 *    is stored, so a save that did not re-probe would leave the drawer saying
 *    "Assistant unavailable" next to a working key.
 *
 * Model and base URL are left blank rather than pre-filled with defaults: the
 * defaults live in one place (`ProviderKind` in Rust), and a second copy here
 * would silently drift the first time one changed.
 */

import { useEffect, useState } from 'react';
import { AlertTriangle, Check, ChevronLeft, KeyRound, Loader2, Trash2 } from 'lucide-react';
import { getPlatform } from '../../platform/registry';
import { useAgentStore } from '../../stores/agentStore';
import type { AgentSettings } from '../../platform/types';

/** Fallback when a host reports no provider list. */
const FALLBACK_PROVIDERS = ['anthropic', 'openai', 'ollama'];

export function AgentSettingsPanel({
  onBack,
  onSaved,
}: {
  onBack: () => void;
  onSaved: () => void;
}) {
  const platform = getPlatform();
  const capabilities = useAgentStore((s) => s.capabilities);

  const [settings, setSettings] = useState<AgentSettings | null>(null);
  const [provider, setProvider] = useState('anthropic');
  const [model, setModel] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [busy, setBusy] = useState<null | 'save' | 'test' | 'clear'>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      if (typeof platform.agentSettings !== 'function') return;
      try {
        const loaded = await platform.agentSettings();
        if (cancelled) return;
        setSettings(loaded);
        setProvider(loaded.provider ?? FALLBACK_PROVIDERS[0]);
        setModel(loaded.model ?? '');
        setBaseUrl(loaded.base_url ?? '');
      } catch (cause) {
        if (!cancelled) setError(String(cause));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [platform]);

  if (typeof platform.agentSettings !== 'function') {
    return (
      <>
        <PanelHeader onBack={onBack} />
        <div className="flex-1 px-4 py-3 text-sm text-gray-500 dark:text-gray-400">
          This host keeps the provider key on its server; there is nothing to configure here.
        </div>
      </>
    );
  }

  const providers = capabilities?.providers?.length ? capabilities.providers : FALLBACK_PROVIDERS;
  // Ollama runs on this machine and needs no credential; every hosted API does.
  const needsKey = provider !== 'ollama';

  const run = async (
    kind: 'save' | 'test' | 'clear',
    action: () => Promise<string | null>,
  ): Promise<void> => {
    setBusy(kind);
    setMessage(null);
    setError(null);
    try {
      const note = await action();
      if (note) setMessage(note);
      onSaved();
    } catch (cause) {
      // A Tauri rejection is a plain string, not an Error.
      setError(typeof cause === 'string' ? cause : String(cause));
    } finally {
      setBusy(null);
    }
  };

  const save = () =>
    run('save', async () => {
      if (typeof platform.agentSettingsSave !== 'function') return null;
      const next = await platform.agentSettingsSave({
        provider,
        model: model.trim() || null,
        base_url: baseUrl.trim() || null,
        // Null, not '': the command keeps the stored key when this is absent.
        api_key: apiKey.trim() || null,
      });
      setSettings(next);
      setApiKey('');
      return 'Saved.';
    });

  const test = () =>
    run('test', async () => {
      if (typeof platform.agentTestConnection !== 'function') return null;
      const reply = await platform.agentTestConnection();
      return `The provider replied: ${reply}`;
    });

  const clearKey = () =>
    run('clear', async () => {
      if (typeof platform.agentKeyClear !== 'function') return null;
      const next = await platform.agentKeyClear();
      setSettings(next);
      setApiKey('');
      return 'The stored key was deleted.';
    });

  return (
    <>
    <PanelHeader onBack={onBack} />
    <div className="flex-1 space-y-3 overflow-y-auto px-4 py-3">
      <p className="text-xs text-gray-500 dark:text-gray-400">
        The assistant runs on this machine. The key is encrypted in the app-data directory and is
        never sent anywhere except to the provider you choose.
      </p>

      <label className="block">
        <span className="text-[11px] font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">
          Provider
        </span>
        <select
          value={provider}
          onChange={(event) => setProvider(event.target.value)}
          className="mt-1 w-full rounded-lg border border-gray-300 bg-white px-2 py-1.5 text-sm text-gray-900 focus:border-primary-500 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-white"
        >
          {providers.map((kind) => (
            <option key={kind} value={kind}>
              {kind}
            </option>
          ))}
        </select>
      </label>

      <label className="block">
        <span className="text-[11px] font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">
          Model
        </span>
        <input
          value={model}
          onChange={(event) => setModel(event.target.value)}
          placeholder="Provider default"
          className="mt-1 w-full rounded-lg border border-gray-300 bg-white px-2 py-1.5 text-sm text-gray-900 focus:border-primary-500 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-white"
        />
      </label>

      <label className="block">
        <span className="text-[11px] font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">
          Base URL
        </span>
        <input
          value={baseUrl}
          onChange={(event) => setBaseUrl(event.target.value)}
          placeholder="Provider default"
          className="mt-1 w-full rounded-lg border border-gray-300 bg-white px-2 py-1.5 text-sm text-gray-900 focus:border-primary-500 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-white"
        />
      </label>

      <label className="block">
        <span className="flex items-center gap-1 text-[11px] font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">
          <KeyRound className="w-3 h-3" /> API key
          {settings?.has_key && (
            <span className="ml-1 rounded bg-green-100 px-1.5 py-0.5 text-[10px] font-medium normal-case tracking-normal text-green-700 dark:bg-green-900/30 dark:text-green-300">
              saved
            </span>
          )}
        </span>
        <input
          type="password"
          value={apiKey}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setApiKey(event.target.value)}
          // Never echoes the stored value: the host cannot return it.
          placeholder={
            settings?.has_key ? 'Stored — leave blank to keep it' : needsKey ? 'Required' : 'Not needed for Ollama'
          }
          className="mt-1 w-full rounded-lg border border-gray-300 bg-white px-2 py-1.5 font-mono text-sm text-gray-900 focus:border-primary-500 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-white"
        />
      </label>

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => void save()}
          disabled={busy !== null}
          className="flex items-center gap-1 rounded-lg bg-primary-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-primary-700 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy === 'save' ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
          ) : (
            <Check className="w-3.5 h-3.5" />
          )}
          Save
        </button>
        <button
          type="button"
          onClick={() => void test()}
          disabled={busy !== null}
          className="flex items-center gap-1 rounded-lg border border-gray-300 px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-600 dark:text-gray-200 dark:hover:bg-slate-700"
        >
          {busy === 'test' ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
          ) : (
            <KeyRound className="w-3.5 h-3.5" />
          )}
          Test connection
        </button>
        {settings?.has_key && (
          <button
            type="button"
            onClick={() => void clearKey()}
            disabled={busy !== null}
            className="flex items-center gap-1 rounded-lg px-2 py-1.5 text-sm font-medium text-red-600 hover:bg-red-50 disabled:cursor-not-allowed disabled:opacity-50 dark:text-red-400 dark:hover:bg-red-900/20"
          >
            {busy === 'clear' ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Trash2 className="w-3.5 h-3.5" />
            )}
            Delete key
          </button>
        )}
      </div>

      {message && (
        <p className="rounded bg-green-50 px-2 py-1.5 text-xs text-green-700 dark:bg-green-900/20 dark:text-green-300">
          {message}
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

/** The panel's own title bar, because it covers the drawer's header. */
function PanelHeader({ onBack }: { onBack: () => void }) {
  return (
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
    </header>
  );
}
