import { useEffect, useState } from 'react';
import {
  Building2, RefreshCw, Plus, Users, LogIn, Ban, CheckCircle2, Trash2,
} from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { Modal } from '../components/ui/Modal';
import { useTenantStore, tenantSupported } from '../stores/tenantStore';
import { useUIStore } from '../stores/uiStore';
import { statusLabel } from '../utils/saas';

/** Workspace management: tenants, membership and the active-tenant switch. */
export function TenantsPanel() {
  const supported = tenantSupported();
  const {
    tenants, members, memberTenantId, busy, error,
    load, create, update, switchTenant, loadMembers, addMember, removeMember, clearError,
  } = useTenantStore();
  const notify = useUIStore((state) => state.addNotification);

  const [name, setName] = useState('');
  const [slug, setSlug] = useState('');
  const [memberModal, setMemberModal] = useState<string | null>(null);
  const [userId, setUserId] = useState('');
  const [role, setRole] = useState('member');

  useEffect(() => {
    if (supported) void load();
  }, [load, supported]);

  if (!supported) {
    return (
      <Card className="mx-auto max-w-lg text-center">
        <Building2 className="mx-auto mb-3 h-8 w-8 text-gray-400" />
        <h2 className="mb-2 font-semibold text-gray-900 dark:text-white">
          Workspaces need the hosted IDE
        </h2>
        <p className="text-sm text-gray-600 dark:text-gray-400">
          This host has no accounts or tenant service. Sign in to the web IDE to
          create and switch workspaces.
        </p>
      </Card>
    );
  }

  const doCreate = async () => {
    if (!name.trim() || !slug.trim()) return;
    if (await create(name.trim(), slug.trim())) {
      notify({ type: 'success', title: 'Workspace created', message: name.trim() });
      setName('');
      setSlug('');
    }
  };

  const doSwitch = async (id: string) => {
    if (await switchTenant(id)) {
      notify({ type: 'info', title: 'Workspace switched', message: id });
    }
  };

  const doStatus = async (id: string, current: string) => {
    const next = current === 'active' ? 'suspended' : 'active';
    if (await update(id, name.trim() || tenants.find((row) => row.id === id)?.name || '', next)) {
      notify({ type: 'success', title: 'Workspace updated', message: statusLabel(next) });
    }
  };

  const doAddMember = async () => {
    if (!memberModal || !userId.trim()) return;
    if (await addMember(memberModal, userId.trim(), role)) {
      notify({ type: 'success', title: 'Member added' });
      setUserId('');
      setRole('member');
      setMemberModal(null);
    }
  };

  const doRemoveMember = async (id: string) => {
    if (!memberTenantId) return;
    if (await removeMember(memberTenantId, id)) {
      notify({ type: 'info', title: 'Member removed' });
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Building2 className="h-6 w-6 text-primary-600" />
          <h1 className="text-xl font-semibold text-gray-900 dark:text-white">Workspaces</h1>
        </div>
        <Button variant="secondary" size="sm" onClick={() => void load()} disabled={busy}>
          <RefreshCw className="mr-1 h-4 w-4" /> Refresh
        </Button>
      </div>

      {error && (
        <div role="alert" className="flex items-center justify-between rounded bg-red-50 p-3 text-sm text-red-700">
          <span>{error}</span>
          <Button variant="link" size="sm" onClick={clearError}>Dismiss</Button>
        </div>
      )}

      <div className="grid gap-6 lg:grid-cols-3">
        <Card>
          <h2 className="mb-3 font-semibold text-gray-900 dark:text-white">New workspace</h2>
          <div className="space-y-3">
            <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} placeholder="Acme Inc." />
            <Input label="Slug" value={slug} onChange={(e) => setSlug(e.target.value)} placeholder="acme" />
            <Button
              className="w-full"
              loading={busy}
              disabled={!name.trim() || !slug.trim()}
              onClick={() => void doCreate()}
            >
              <Plus className="mr-1 h-4 w-4" /> Create workspace
            </Button>
          </div>
        </Card>

        <div className="space-y-4 lg:col-span-2">
          {tenants.length === 0 && (
            <Card className="text-center text-sm text-gray-500 dark:text-gray-400">
              No workspaces yet. Create one to get started.
            </Card>
          )}
          {tenants.map((tenant) => (
            <Card key={tenant.id}>
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <h3 className="font-semibold text-gray-900 dark:text-white">{tenant.name}</h3>
                  <p className="text-xs text-gray-500 dark:text-gray-400">
                    {tenant.slug} · {tenant.role ?? 'member'} · {tenant.plan_id ?? 'no plan'}
                  </p>
                </div>
                <span className="rounded bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-600 dark:bg-slate-700 dark:text-gray-300">
                  {statusLabel(tenant.status)}
                </span>
              </div>
              <div className="mt-4 flex flex-wrap gap-2">
                <Button size="sm" variant="secondary" onClick={() => void doSwitch(tenant.id)}>
                  <LogIn className="mr-1 h-4 w-4" /> Switch
                </Button>
                <Button size="sm" variant="secondary" onClick={() => void loadMembers(tenant.id)}>
                  <Users className="mr-1 h-4 w-4" /> Members
                </Button>
                <Button size="sm" variant="secondary" onClick={() => setMemberModal(tenant.id)}>
                  <Plus className="mr-1 h-4 w-4" /> Add member
                </Button>
                <Button size="sm" variant="ghost" onClick={() => void doStatus(tenant.id, tenant.status)}>
                  {tenant.status === 'active'
                    ? <><Ban className="mr-1 h-4 w-4" /> Suspend</>
                    : <><CheckCircle2 className="mr-1 h-4 w-4" /> Activate</>}
                </Button>
              </div>
              {memberTenantId === tenant.id && (
                <div className="mt-4 border-t border-gray-200 pt-3 dark:border-slate-700">
                  <h4 className="mb-2 text-sm font-medium text-gray-700 dark:text-gray-300">Members</h4>
                  {members.length === 0 && (
                    <p className="text-xs text-gray-500">No members found.</p>
                  )}
                  {members.map((member) => (
                    <div key={member.user_id} className="flex items-center justify-between py-1 text-sm">
                      <span className="truncate text-gray-700 dark:text-gray-300">
                        {member.email} <span className="text-xs text-gray-400">({member.role})</span>
                      </span>
                      <Button variant="ghost" size="sm" onClick={() => void doRemoveMember(member.user_id)}>
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </Card>
          ))}
        </div>
      </div>

      <Modal open={memberModal !== null} title="Add member" onClose={() => setMemberModal(null)}>
        <div className="space-y-3">
          <Input
            label="User id"
            value={userId}
            onChange={(e) => setUserId(e.target.value)}
            placeholder="User UUID"
          />
          <Select
            label="Role"
            value={role}
            onChange={(e) => setRole(e.target.value)}
            options={[
              { value: 'member', label: 'Member' },
              { value: 'admin', label: 'Admin' },
              { value: 'owner', label: 'Owner' },
            ]}
          />
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setMemberModal(null)}>Cancel</Button>
            <Button loading={busy} disabled={!userId.trim()} onClick={() => void doAddMember()}>
              Add member
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
