import { useEffect, useState } from 'react';
import {
  ShieldCheck, RefreshCw, Ban, CheckCircle2, Flag, Trash2, UserCog, ScrollText,
} from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Input } from '../components/ui/Input';
import { Modal } from '../components/ui/Modal';
import { adminSupported, useAdminStore } from '../stores/adminStore';
import { useUIStore } from '../stores/uiStore';
import { formatMoney, statusLabel } from '../utils/saas';
import type { AdminUser } from '../platform/types';

type Tab = 'overview' | 'tenants' | 'users' | 'flags' | 'audit';
const TABS: Tab[] = ['overview', 'tenants', 'users', 'flags', 'audit'];

/** Platform super-admin panel. All authorization is enforced server-side. */
export function AdminPanel() {
  const supported = adminSupported();
  const {
    tenants, users, analytics, usage, flags, audit, lastImpersonation, busy, error,
    load, setTenantStatus, updateUser, impersonate, createFlag, updateFlag,
    deleteFlag, setOverride, clearError,
  } = useAdminStore();
  const notify = useUIStore((state) => state.addNotification);

  const [tab, setTab] = useState<Tab>('overview');
  const [search, setSearch] = useState('');
  const [flagKey, setFlagKey] = useState('');
  const [impersonateId, setImpersonateId] = useState<string | null>(null);
  const [reason, setReason] = useState('');
  const [override, setOverrideState] = useState<{ flag: string; tenant: string } | null>(null);

  useEffect(() => {
    if (supported) void load();
  }, [load, supported]);

  if (!supported) {
    return (
      <Card className="mx-auto max-w-lg text-center">
        <ShieldCheck className="mx-auto mb-3 h-8 w-8 text-gray-400" />
        <h2 className="mb-2 font-semibold text-gray-900 dark:text-white">
          Administration needs the hosted IDE
        </h2>
        <p className="text-sm text-gray-600 dark:text-gray-400">
          This host has no platform admin service.
        </p>
      </Card>
    );
  }

  const visibleTenants = tenants.filter((row) =>
    row.name.toLowerCase().includes(search.trim().toLowerCase()),
  );
  const visibleUsers = users.filter((row) =>
    row.email.toLowerCase().includes(search.trim().toLowerCase()),
  );

  const doUpdateUser = async (user: AdminUser, isActive: boolean) => {
    if (await updateUser(user.id, { is_active: isActive })) {
      notify({ type: 'success', title: 'User updated' });
    }
  };

  const doImpersonate = async () => {
    if (!impersonateId) return;
    if (await impersonate(impersonateId, reason.trim() || undefined)) {
      notify({ type: 'info', title: 'Impersonation recorded', message: 'Audit only — no session swapped.' });
      setImpersonateId(null);
      setReason('');
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <ShieldCheck className="h-6 w-6 text-primary-600" />
          <h1 className="text-xl font-semibold text-gray-900 dark:text-white">Administration</h1>
        </div>
        <div className="flex gap-2">
          <Input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search" className="w-48" />
          <Button variant="secondary" size="sm" onClick={() => void load()} disabled={busy}>
            <RefreshCw className="mr-1 h-4 w-4" /> Refresh
          </Button>
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        {TABS.map((item) => (
          <Button key={item} size="sm" variant={tab === item ? 'primary' : 'secondary'} onClick={() => setTab(item)}>
            {statusLabel(item)}
          </Button>
        ))}
      </div>

      {error && (
        <div role="alert" className="flex items-center justify-between rounded bg-red-50 p-3 text-sm text-red-700">
          <span>{error}</span>
          <Button variant="link" size="sm" onClick={clearError}>Dismiss</Button>
        </div>
      )}
      {lastImpersonation && (
        <div className="rounded bg-blue-50 p-3 text-sm text-blue-700 dark:bg-slate-800 dark:text-blue-300">
          Last impersonation: {lastImpersonation.impersonation_id} — {lastImpersonation.note}
        </div>
      )}

      {tab === 'overview' && (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <Card><p className="text-xs text-gray-500">MRR</p><p className="text-lg font-semibold">{formatMoney(analytics?.mrr_cents ?? 0)}</p></Card>
          <Card><p className="text-xs text-gray-500">ARR</p><p className="text-lg font-semibold">{formatMoney(analytics?.arr_cents ?? 0)}</p></Card>
          <Card><p className="text-xs text-gray-500">Active subscriptions</p><p className="text-lg font-semibold">{analytics?.active_subscriptions ?? 0}</p></Card>
          <Card><p className="text-xs text-gray-500">Churn</p><p className="text-lg font-semibold">{((analytics?.churn_rate ?? 0) * 100).toFixed(1)}%</p></Card>
          <Card className="sm:col-span-2 lg:col-span-4">
            <h2 className="mb-2 font-semibold">Platform usage</h2>
            <div className="flex flex-wrap gap-4 text-sm">
              <span className="rounded bg-gray-100 px-2 py-1 dark:bg-slate-700">Projects: {usage?.projects ?? 0}</span>
              <span className="rounded bg-gray-100 px-2 py-1 dark:bg-slate-700">Integrations: {usage?.integrations ?? 0}</span>
              <span className="rounded bg-gray-100 px-2 py-1 dark:bg-slate-700">Deployments: {usage?.deployments ?? 0}</span>
              <span className="rounded bg-gray-100 px-2 py-1 dark:bg-slate-700">Usage events: {usage?.usage_events ?? 0}</span>
              <span className="rounded bg-gray-100 px-2 py-1 dark:bg-slate-700">Members: {usage?.tenant_members ?? 0}</span>
              <span className="rounded bg-gray-100 px-2 py-1 dark:bg-slate-700">Audit entries: {usage?.audit_log ?? 0}</span>
            </div>
          </Card>
        </div>
      )}

      {tab === 'tenants' && (
        <Card>
          <h2 className="mb-3 font-semibold">Tenants</h2>
          {visibleTenants.map((tenant) => (
            <div key={tenant.id} className="flex items-center justify-between border-t border-gray-100 py-2 dark:border-slate-700">
              <div>
                <p className="text-sm font-medium">{tenant.name} <span className="text-xs text-gray-400">({tenant.slug})</span></p>
                <p className="text-xs text-gray-500">{statusLabel(tenant.status)}</p>
              </div>
              <Button size="sm" variant="ghost" onClick={() => void setTenantStatus(
                tenant.id,
                tenant.status === 'active' ? 'suspended' : 'active',
              )}>
                {tenant.status === 'active' ? <Ban className="mr-1 h-4 w-4" /> : <CheckCircle2 className="mr-1 h-4 w-4" />}
                {tenant.status === 'active' ? 'Suspend' : 'Activate'}
              </Button>
            </div>
          ))}
        </Card>
      )}

      {tab === 'users' && (
        <Card>
          <h2 className="mb-3 font-semibold">Users</h2>
          {visibleUsers.map((user) => (
            <div key={user.id} className="flex flex-wrap items-center justify-between gap-2 border-t border-gray-100 py-2 text-sm dark:border-slate-700">
              <span className="truncate">{user.email} <span className="text-xs text-gray-400">{user.platform_role ?? 'member'}</span></span>
              <div className="flex gap-1">
                <Button size="sm" variant="ghost" onClick={() => void doUpdateUser(user, !user.is_active)}>
                  <UserCog className="mr-1 h-4 w-4" /> {user.is_active ? 'Deactivate' : 'Activate'}
                </Button>
                <Button size="sm" variant="secondary" onClick={() => setImpersonateId(user.id)}>
                  Impersonate
                </Button>
              </div>
            </div>
          ))}
        </Card>
      )}

      {tab === 'flags' && (
        <Card>
          <h2 className="mb-3 flex items-center gap-2 font-semibold"><Flag className="h-4 w-4" /> Platform flags</h2>
          <div className="flex gap-2">
            <Input value={flagKey} onChange={(e) => setFlagKey(e.target.value)} placeholder="flag.key" />
            <Button size="sm" disabled={!flagKey.trim()} onClick={async () => {
              if (await createFlag({ key: flagKey.trim() })) { setFlagKey(''); notify({ type: 'success', title: 'Flag created' }); }
            }}>Create</Button>
          </div>
          {flags.map((flag) => (
            <div key={flag.key} className="flex flex-wrap items-center justify-between gap-2 border-t border-gray-100 py-2 text-sm dark:border-slate-700">
              <span>{flag.key} <span className="text-xs text-gray-400">{flag.rollout_percentage}% · {flag.enabled ? 'on' : 'off'}</span></span>
              <div className="flex gap-1">
                <Button size="sm" variant="ghost" onClick={() => void updateFlag(flag.key, { enabled: !flag.enabled })}>
                  {flag.enabled ? 'Disable' : 'Enable'}
                </Button>
                <Button size="sm" variant="secondary" onClick={() => setOverrideState({ flag: flag.key, tenant: '' })}>Override</Button>
                <Button size="sm" variant="danger" onClick={() => void deleteFlag(flag.key)}><Trash2 className="h-4 w-4" /></Button>
              </div>
            </div>
          ))}
        </Card>
      )}

      {tab === 'audit' && (
        <Card>
          <h2 className="mb-3 flex items-center gap-2 font-semibold"><ScrollText className="h-4 w-4" /> Audit log</h2>
          {audit.length === 0 && <p className="text-sm text-gray-500">No audit entries.</p>}
          {audit.map((entry) => (
            <div key={entry.id} className="border-t border-gray-100 py-2 text-xs dark:border-slate-700">
              <span className="font-medium">{entry.action}</span> · {entry.target_type} · {entry.actor_email ?? entry.actor_user_id} · {entry.created_at}
            </div>
          ))}
        </Card>
      )}

      <Modal open={impersonateId !== null} title="Record impersonation" onClose={() => setImpersonateId(null)}>
        <div className="space-y-3">
          <Input label="Reason" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="Support request" />
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setImpersonateId(null)}>Cancel</Button>
            <Button onClick={() => void doImpersonate()}>Record</Button>
          </div>
        </div>
      </Modal>

      <Modal open={override !== null} title="Set flag override" onClose={() => setOverrideState(null)}>
        <div className="space-y-3">
          <Input label="Tenant id" value={override?.tenant ?? ''} onChange={(e) => setOverrideState(override ? { ...override, tenant: e.target.value } : null)} />
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setOverrideState(null)}>Cancel</Button>
            <Button disabled={!override?.tenant} onClick={async () => {
              if (!override) return;
              if (await setOverride(override.flag, override.tenant, true)) { setOverrideState(null); notify({ type: 'success', title: 'Override set' }); }
            }}>Enable for tenant</Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
