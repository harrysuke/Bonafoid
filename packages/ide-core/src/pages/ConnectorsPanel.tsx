import { useEffect, useState } from 'react';
import { PlugZap, RefreshCw, Plus, Trash2, Activity, Send, KeyRound } from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { Modal } from '../components/ui/Modal';
import { connectorSupported, useConnectorStore } from '../stores/connectorStore';
import { useUIStore } from '../stores/uiStore';
import { statusLabel } from '../utils/saas';

/** Connector catalog, instances, connection tests and JSON proxy invokes. */
export function ConnectorsPanel() {
  const supported = connectorSupported();
  const {
    catalog, connectors, testResult, invokeResult, busy, error,
    load, create, remove, test, invoke, clearError,
  } = useConnectorStore();
  const notify = useUIStore((state) => state.addNotification);

  const [showCreate, setShowCreate] = useState(false);
  const [kind, setKind] = useState('http_rest');
  const [name, setName] = useState('');
  const [configText, setConfigText] = useState('{\n  "url": "http://localhost:8080"\n}');
  const [secret, setSecret] = useState('');
  const [invokeId, setInvokeId] = useState<string | null>(null);
  const [method, setMethod] = useState('GET');
  const [path, setPath] = useState('/');
  const [bodyText, setBodyText] = useState('');

  useEffect(() => {
    if (supported) void load();
  }, [load, supported]);

  if (!supported) {
    return (
      <Card className="mx-auto max-w-lg text-center">
        <PlugZap className="mx-auto mb-3 h-8 w-8 text-gray-400" />
        <h2 className="mb-2 font-semibold text-gray-900 dark:text-white">
          Connectors need the hosted IDE
        </h2>
        <p className="text-sm text-gray-600 dark:text-gray-400">
          This host has no connector registry or credential vault.
        </p>
      </Card>
    );
  }

  const doCreate = async () => {
    let config: Record<string, unknown>;
    try {
      const parsed: unknown = JSON.parse(configText);
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        throw new Error('config must be a JSON object');
      }
      config = parsed as Record<string, unknown>;
    } catch (cause) {
      notify({ type: 'error', title: 'Invalid config', message: cause instanceof Error ? cause.message : String(cause) });
      return;
    }
    const ok = await create({
      kind,
      name: name.trim(),
      config,
      ...(secret.trim() ? { secret: secret.trim() } : {}),
    });
    if (ok) {
      notify({ type: 'success', title: 'Connector created' });
      setName('');
      setSecret(''); // Write-only: clear the input immediately after submit.
      setShowCreate(false);
    }
  };

  const doInvoke = async () => {
    if (!invokeId) return;
    let body: unknown;
    if (bodyText.trim()) {
      try {
        body = JSON.parse(bodyText) as unknown;
      } catch {
        notify({ type: 'error', title: 'Invalid JSON body' });
        return;
      }
    }
    await invoke(invokeId, { method, path, ...(body === undefined ? {} : { body }) });
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <PlugZap className="h-6 w-6 text-primary-600" />
          <h1 className="text-xl font-semibold text-gray-900 dark:text-white">Connectors</h1>
        </div>
        <div className="flex gap-2">
          <Button size="sm" onClick={() => setShowCreate(true)}><Plus className="mr-1 h-4 w-4" /> New connector</Button>
          <Button variant="secondary" size="sm" onClick={() => void load()} disabled={busy}>
            <RefreshCw className="mr-1 h-4 w-4" /> Refresh
          </Button>
        </div>
      </div>

      {error && (
        <div role="alert" className="flex items-center justify-between rounded bg-red-50 p-3 text-sm text-red-700">
          <span>{error}</span>
          <Button variant="link" size="sm" onClick={clearError}>Dismiss</Button>
        </div>
      )}

      <Card>
        <h2 className="mb-2 font-semibold">Supported kinds</h2>
        <div className="flex flex-wrap gap-2">
          {catalog.map((item) => (
            <span key={item.kind} className="rounded bg-gray-100 px-2 py-1 text-xs dark:bg-slate-700" title={item.description}>
              {statusLabel(item.kind)}
            </span>
          ))}
        </div>
      </Card>

      {connectors.length === 0 && (
        <Card className="text-center text-sm text-gray-500">No connectors configured yet.</Card>
      )}

      <div className="grid gap-4 md:grid-cols-2">
        {connectors.map((connector) => (
          <Card key={connector.id}>
            <div className="flex items-start justify-between gap-2">
              <div>
                <h3 className="font-semibold">{connector.name}</h3>
                <p className="text-xs text-gray-500">{statusLabel(connector.kind)} · {connector.id}</p>
              </div>
              <span className="rounded bg-gray-100 px-2 py-0.5 text-xs dark:bg-slate-700">
                {connector.enabled ? 'enabled' : 'disabled'}
              </span>
            </div>
            <pre className="mt-3 overflow-x-auto rounded bg-gray-50 p-2 text-xs dark:bg-slate-900">
              {JSON.stringify(connector.config)}
            </pre>
            {connector.secret_set && (
              <p className="mt-2 flex items-center gap-1 text-xs text-green-600">
                <KeyRound className="h-3 w-3" /> Secret sealed (value hidden)
              </p>
            )}
            <div className="mt-4 flex flex-wrap gap-2">
              <Button size="sm" variant="secondary" onClick={() => void test(connector.id)}>
                <Activity className="mr-1 h-4 w-4" /> Test
              </Button>
              <Button size="sm" variant="secondary" onClick={() => setInvokeId(connector.id)}>
                <Send className="mr-1 h-4 w-4" /> Invoke
              </Button>
              <Button size="sm" variant="danger" onClick={() => void remove(connector.id)}>
                <Trash2 className="mr-1 h-4 w-4" /> Delete
              </Button>
            </div>
          </Card>
        ))}
      </div>

      {testResult && (
        <Card className={testResult.ok ? 'border-green-300' : 'border-red-300'}>
          <p className="text-sm">
            <strong>{testResult.ok ? 'Test passed' : 'Test failed'}</strong> · {testResult.message} · {testResult.latency_ms} ms
          </p>
        </Card>
      )}
      {invokeResult && (
        <Card>
          <p className="mb-1 text-sm font-medium">Invoke result: HTTP {invokeResult.status}</p>
          <pre className="overflow-x-auto rounded bg-gray-50 p-2 text-xs dark:bg-slate-900">
            {JSON.stringify(invokeResult.body, null, 2)}
          </pre>
        </Card>
      )}

      <Modal open={showCreate} title="New connector" onClose={() => setShowCreate(false)}>
        <div className="space-y-3">
          <Select
            label="Kind"
            value={kind}
            onChange={(e) => setKind(e.target.value)}
            options={catalog.map((item) => ({ value: item.kind, label: statusLabel(item.kind) }))}
          />
          <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} />
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            Config (JSON)
            <textarea
              className="mt-1 w-full rounded-lg border border-gray-300 bg-white p-2 font-mono text-xs dark:border-slate-600 dark:bg-slate-800"
              rows={4}
              value={configText}
              onChange={(e) => setConfigText(e.target.value)}
            />
          </label>
          <Input
            label="Secret (write-only)"
            type="password"
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
            helperText="Sealed immediately; it is never shown again."
          />
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button loading={busy} disabled={!name.trim()} onClick={() => void doCreate()}>Create</Button>
          </div>
        </div>
      </Modal>

      <Modal open={invokeId !== null} title="Invoke connector" onClose={() => setInvokeId(null)}>
        <div className="space-y-3">
          <div className="grid grid-cols-2 gap-3">
            <Select
              label="Method"
              value={method}
              onChange={(e) => setMethod(e.target.value)}
              options={['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].map((value) => ({ value, label: value }))}
            />
            <Input label="Path" value={path} onChange={(e) => setPath(e.target.value)} />
          </div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            JSON body (optional)
            <textarea
              className="mt-1 w-full rounded-lg border border-gray-300 bg-white p-2 font-mono text-xs dark:border-slate-600 dark:bg-slate-800"
              rows={3}
              value={bodyText}
              onChange={(e) => setBodyText(e.target.value)}
            />
          </label>
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setInvokeId(null)}>Close</Button>
            <Button loading={busy} onClick={() => void doInvoke()}>Send</Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
