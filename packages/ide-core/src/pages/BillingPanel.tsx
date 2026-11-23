import { useEffect, useState } from 'react';
import { CreditCard, RefreshCw, Plus, Receipt, Gauge, ExternalLink } from 'lucide-react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { Modal } from '../components/ui/Modal';
import { billingSupported, useBillingStore } from '../stores/billingStore';
import { useUIStore } from '../stores/uiStore';
import { formatMoney, statusLabel } from '../utils/saas';

/** Subscription, plans, invoices, entitlements and usage meters. */
export function BillingPanel() {
  const supported = billingSupported();
  const {
    plans, subscription, invoices, entitlements, usage, busy, error,
    load, createPlan, subscribe, cancel, recordUsage, checkout, portal, clearError,
  } = useBillingStore();
  const notify = useUIStore((state) => state.addNotification);

  const [planModal, setPlanModal] = useState(false);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [currency, setCurrency] = useState('USD');
  const [interval, setInterval] = useState('month');
  const [amount, setAmount] = useState('');
  const [meterKey, setMeterKey] = useState('');
  const [quantity, setQuantity] = useState('1');

  useEffect(() => {
    if (supported) void load();
  }, [load, supported]);

  if (!supported) {
    return (
      <Card className="mx-auto max-w-lg text-center">
        <CreditCard className="mx-auto mb-3 h-8 w-8 text-gray-400" />
        <h2 className="mb-2 font-semibold text-gray-900 dark:text-white">
          Billing needs the hosted IDE
        </h2>
        <p className="text-sm text-gray-600 dark:text-gray-400">
          This host has no billing service. Sign in to the web IDE to manage plans
          and subscriptions.
        </p>
      </Card>
    );
  }

  const doCreatePlan = async () => {
    const cents = Math.round(Number(amount) * 100);
    if (!name.trim() || !Number.isFinite(cents) || cents <= 0) return;
    const ok = await createPlan({
      name: name.trim(),
      description: description.trim() || undefined,
      currency,
      interval,
      amount_cents: cents,
    });
    if (ok) {
      notify({ type: 'success', title: 'Plan created' });
      setName('');
      setDescription('');
      setAmount('');
      setPlanModal(false);
    }
  };

  const doCheckout = async (planId: string) => {
    const session = await checkout(
      planId,
      `${window.location.origin}/billing?checkout=success`,
      `${window.location.origin}/billing?checkout=cancelled`,
    );
    if (session) window.location.href = session.url;
  };

  const doPortal = async () => {
    const session = await portal(`${window.location.origin}/billing`);
    if (session) window.location.href = session.url;
  };

  const doRecordUsage = async () => {
    const amountValue = Number(quantity);
    if (!meterKey.trim() || !Number.isFinite(amountValue) || amountValue < 0) return;
    if (await recordUsage(meterKey.trim(), amountValue)) {
      notify({ type: 'success', title: 'Usage recorded' });
      setQuantity('1');
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <CreditCard className="h-6 w-6 text-primary-600" />
          <h1 className="text-xl font-semibold text-gray-900 dark:text-white">Billing</h1>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" size="sm" onClick={() => setPlanModal(true)}>
            <Plus className="mr-1 h-4 w-4" /> New plan
          </Button>
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
        <h2 className="mb-2 font-semibold text-gray-900 dark:text-white">Current subscription</h2>
        {!subscription && (
          <p className="text-sm text-gray-500 dark:text-gray-400">
            No active subscription. Choose a plan below.
          </p>
        )}
        {subscription && (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p className="font-medium text-gray-900 dark:text-white">
                Plan {subscription.plan_id}
                <span className="ml-2 rounded bg-gray-100 px-2 py-0.5 text-xs dark:bg-slate-700">
                  {statusLabel(subscription.status)}
                </span>
              </p>
              <p className="text-xs text-gray-500 dark:text-gray-400">
                {subscription.seat_count} seat(s) · through {subscription.period_end}
                {subscription.cancel_at_period_end ? ' · cancels at period end' : ''}
              </p>
            </div>
            <div className="flex gap-2">
              <Button size="sm" variant="secondary" onClick={() => void doPortal()}>
                <ExternalLink className="mr-1 h-4 w-4" /> Billing portal
              </Button>
              {!subscription.cancel_at_period_end && (
                <Button size="sm" variant="danger" onClick={() => void cancel()}>
                  Cancel
                </Button>
              )}
            </div>
          </div>
        )}
      </Card>

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <h2 className="mb-3 font-semibold text-gray-900 dark:text-white">Plans</h2>
          {plans.length === 0 && <p className="text-sm text-gray-500">No plans available.</p>}
          <div className="space-y-2">
            {plans.map((plan) => (
              <div key={plan.id} className="flex items-center justify-between rounded border border-gray-200 p-3 dark:border-slate-700">
                <div>
                  <p className="font-medium text-gray-900 dark:text-white">{plan.name}</p>
                  <p className="text-xs text-gray-500 dark:text-gray-400">
                    {formatMoney(plan.amount_cents, plan.currency)} / {plan.interval}
                    {plan.description ? ` · ${plan.description}` : ''}
                  </p>
                </div>
                <div className="flex gap-1">
                  <Button size="sm" variant="secondary" onClick={() => void subscribe(plan.id)}>
                    Subscribe
                  </Button>
                  <Button size="sm" onClick={() => void doCheckout(plan.id)}>
                    Checkout
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </Card>

        <div className="space-y-6">
          <Card>
            <h2 className="mb-3 flex items-center gap-2 font-semibold text-gray-900 dark:text-white">
              <Gauge className="h-4 w-4" /> Usage
            </h2>
            <div className="flex gap-2">
              <Input value={meterKey} onChange={(e) => setMeterKey(e.target.value)} placeholder="meter_key" />
              <Input value={quantity} onChange={(e) => setQuantity(e.target.value)} type="number" className="w-24" />
              <Button size="sm" onClick={() => void doRecordUsage()} disabled={!meterKey.trim()}>Record</Button>
            </div>
            <div className="mt-3 space-y-1">
              {usage.map((meter) => (
                <div key={meter.meter_key} className="flex justify-between text-sm">
                  <span className="text-gray-700 dark:text-gray-300">{meter.meter_key}</span>
                  <span className="text-gray-500">{meter.quantity} · {meter.period}</span>
                </div>
              ))}
            </div>
          </Card>

          <Card>
            <h2 className="mb-2 font-semibold text-gray-900 dark:text-white">Entitlements</h2>
            <div className="flex flex-wrap gap-2">
              {entitlements.map((item) => (
                <span key={item.feature_key} className="rounded bg-gray-100 px-2 py-1 text-xs dark:bg-slate-700">
                  {item.feature_key}: {item.enabled ? (item.limit_value ?? 'enabled') : 'off'}
                </span>
              ))}
            </div>
          </Card>
        </div>
      </div>

      <Card>
        <h2 className="mb-3 flex items-center gap-2 font-semibold text-gray-900 dark:text-white">
          <Receipt className="h-4 w-4" /> Invoices
        </h2>
        {invoices.length === 0 && <p className="text-sm text-gray-500">No invoices yet.</p>}
        {invoices.map((invoice) => (
          <div key={invoice.id} className="flex justify-between border-t border-gray-100 py-2 text-sm dark:border-slate-700">
            <span className="text-gray-700 dark:text-gray-300">{invoice.id} · {invoice.issued_at}</span>
            <span>{formatMoney(invoice.amount_cents, invoice.currency)} · {statusLabel(invoice.status)}</span>
          </div>
        ))}
      </Card>

      <Modal open={planModal} title="Create plan" onClose={() => setPlanModal(false)}>
        <div className="space-y-3">
          <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} />
          <Input label="Description" value={description} onChange={(e) => setDescription(e.target.value)} />
          <div className="grid grid-cols-2 gap-3">
            <Input label="Currency" value={currency} onChange={(e) => setCurrency(e.target.value)} />
            <Select
              label="Interval"
              value={interval}
              onChange={(e) => setInterval(e.target.value)}
              options={[{ value: 'month', label: 'Month' }, { value: 'year', label: 'Year' }]}
            />
          </div>
          <Input label="Amount (in currency units)" type="number" value={amount} onChange={(e) => setAmount(e.target.value)} />
          <div className="flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setPlanModal(false)}>Cancel</Button>
            <Button loading={busy} onClick={() => void doCreatePlan()}>Create plan</Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
