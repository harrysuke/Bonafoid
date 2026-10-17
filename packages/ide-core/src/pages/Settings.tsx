import { useState } from 'react';
import { Save, Database, Shield, Mail, Globe, Users, Bell, Palette, Terminal, Code } from 'lucide-react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { useProjectStore } from '../stores/projectStore';
import { DB_DRIVERS, BUNDLERS, FRONTEND_FRAMEWORKS } from '../types/project';

const tabs = [
  { id: 'general', label: 'General', icon: Globe },
  { id: 'database', label: 'Database', icon: Database },
  { id: 'frontend', label: 'Frontend', icon: Code },
  { id: 'auth', label: 'Authentication', icon: Shield },
  { id: 'email', label: 'Email', icon: Mail },
  { id: 'api', label: 'API', icon: Terminal },
  { id: 'storage', label: 'Storage', icon: Users },
  { id: 'features', label: 'Features', icon: Palette },
  { id: 'notifications', label: 'Notifications', icon: Bell },
];

export function Settings() {
  const [activeTab, setActiveTab] = useState('general');
  const [saved, setSaved] = useState(false);
  const { project, setDbDriver, setFrontendFramework, saveProject } = useProjectStore();

  const driver = project?.settings.database.driver ?? 'postgres';
  const url = project?.settings.database.url ?? '';
  const framework = project?.settings.frontend.framework ?? 'leptos';
  const bundler = project?.settings.frontend.bundler ?? 'trunk';
  const db = project?.settings.database as
    | { max_connections?: number; min_connections?: number }
    | undefined;

  const handleSave = async () => {
    await saveProject();
    setSaved(true);
    setTimeout(() => setSaved(false), 2500);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Project Settings</h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">Configure your project settings</p>
        </div>
        <Button variant="primary" onClick={handleSave}>
          <Save className="w-4 h-4 mr-2" /> Save Changes
        </Button>
      </div>

      {saved && (
        <div className="fixed top-4 right-4 z-50 bg-green-500 text-white px-4 py-2 rounded-lg shadow-lg animate-in">
          Settings saved successfully!
        </div>
      )}

      <div className="flex gap-6">
        {/* Sidebar */}
        <div className="w-56 flex-shrink-0">
          <nav className="space-y-1">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors ${
                  activeTab === tab.id
                    ? 'bg-primary-50 text-primary-700 dark:bg-primary-900/30 dark:text-primary-400'
                    : 'text-gray-600 hover:bg-gray-100 hover:text-gray-900 dark:text-gray-400 dark:hover:bg-slate-700/50 dark:hover:text-white'
                }`}
              >
                <tab.icon className="w-5 h-5" />
                {tab.label}
              </button>
            ))}
          </nav>
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          <Card className="h-full">
            <div className="p-6 border-b border-gray-200 dark:border-slate-700">
              <h2 className="text-xl font-semibold text-gray-900 dark:text-white capitalize">{activeTab} Settings</h2>
            </div>
            <div className="p-6 overflow-y-auto">
              {activeTab === 'general' && (
                <div className="space-y-6 max-w-2xl">
                  <div>
                    <label className="label">Project Name</label>
                    <Input value={project?.name ?? ''} readOnly placeholder="Enter project name" />
                  </div>
                  <div>
                    <label className="label">Project Slug</label>
                    <Input value={project?.slug ?? ''} readOnly placeholder="URL-friendly slug" />
                  </div>
                  <div>
                    <label className="label">Description</label>
                    <textarea
                      className="input h-24"
                      readOnly
                      value={project?.description ?? ''}
                      placeholder="Describe your project..."
                    />
                  </div>
                  <div>
                    <label className="label">Version</label>
                    <Input value={project?.version ?? ''} readOnly placeholder="Semantic version" />
                  </div>
                </div>
              )}

              {activeTab === 'database' && (
                <div className="space-y-6 max-w-2xl">
                  <div>
                    <label className="label">Database Driver</label>
                    <Select
                      value={driver}
                      onChange={(e: React.ChangeEvent<HTMLSelectElement>) =>
                        void setDbDriver(e.target.value)
                      }
                      options={DB_DRIVERS}
                    />
                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                      The generated backend uses SeaORM; only PostgreSQL and SQLite are
                      validated end-to-end.
                    </p>
                  </div>
                  <div>
                    <label className="label">Connection URL</label>
                    <Input
                      value={url}
                      readOnly
                      placeholder="Database URL"
                    />
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="label">Max Connections</label>
                      <Input type="number" value={String(db?.max_connections ?? 20)} readOnly />
                    </div>
                    <div>
                      <label className="label">Min Connections</label>
                      <Input type="number" value={String(db?.min_connections ?? 5)} readOnly />
                    </div>
                  </div>
                </div>
              )}

              {activeTab === 'frontend' && (
                <div className="space-y-6 max-w-2xl">
                  <div>
                    <label className="label">Framework</label>
                    <Select
                      value={framework}
                      onChange={(e: React.ChangeEvent<HTMLSelectElement>) =>
                        void setFrontendFramework(e.target.value)
                      }
                      options={FRONTEND_FRAMEWORKS}
                    />
                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                      Generated frontends are implemented in Leptos (Rust compiled to
                      WebAssembly).
                    </p>
                  </div>
                  <div>
                    <label className="label">Bundler</label>
                    <Select value={bundler} options={BUNDLERS} disabled />
                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                      Trunk is the supported build tool for the Leptos frontend.
                    </p>
                  </div>
                </div>
              )}

              {activeTab === 'auth' && (
                <div className="space-y-6 max-w-2xl">
                  <div className="flex items-center gap-2">
                    <input type="checkbox" id="auth-enabled" className="rounded" checked />
                    <label htmlFor="auth-enabled" className="font-medium text-gray-900 dark:text-white">Enable Authentication</label>
                  </div>
                  <div>
                    <label className="label">JWT Secret</label>
                    <Input type="password" value="super-secret-key-change-in-production" placeholder="JWT signing secret" />
                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">Change this in production!</p>
                  </div>
                  <div>
                    <label className="label">Token Expiration (seconds)</label>
                    <Input type="number" value="86400" />
                  </div>
                  <div>
                    <label className="label">Password Min Length</label>
                    <Input type="number" value="8" />
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input type="checkbox" className="rounded" checked />
                        <span className="text-sm text-gray-700 dark:text-gray-300">Require uppercase</span>
                      </label>
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input type="checkbox" className="rounded" checked />
                        <span className="text-sm text-gray-700 dark:text-gray-300">Require lowercase</span>
                      </label>
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input type="checkbox" className="rounded" checked />
                        <span className="text-sm text-gray-700 dark:text-gray-300">Require numbers</span>
                      </label>
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input type="checkbox" className="rounded" checked />
                        <span className="text-sm text-gray-700 dark:text-gray-300">Require symbols</span>
                      </label>
                    </div>
                    <div className="space-y-2">
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input type="checkbox" className="rounded" />
                        <span className="text-sm text-gray-700 dark:text-gray-300">Enable MFA</span>
                      </label>
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input type="checkbox" className="rounded" />
                        <span className="text-sm text-gray-700 dark:text-gray-300">Require MFA</span>
                      </label>
                    </div>
                  </div>
                </div>
              )}

              {activeTab === 'email' && (
                <div className="space-y-6 max-w-2xl">
                  <div>
                    <label className="label">Email Driver</label>
                    <Select value="smtp" options={[{value: 'smtp', label: 'SMTP'}, {value: 'sendgrid', label: 'SendGrid'}, {value: 'mailgun', label: 'Mailgun'}, {value: 'ses', label: 'AWS SES'}]} />
                  </div>
                  <div>
                    <label className="label">From Name</label>
                    <Input value="No-Code IDE" />
                  </div>
                  <div>
                    <label className="label">From Email</label>
                    <Input type="email" value="noreply@example.com" />
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="label">SMTP Host</label>
                      <Input value="smtp.example.com" />
                    </div>
                    <div>
                      <label className="label">SMTP Port</label>
                      <Input type="number" value="587" />
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="label">Username</label>
                      <Input value="user@example.com" />
                    </div>
                    <div>
                      <label className="label">Password</label>
                      <Input type="password" value="********" />
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <input type="checkbox" id="tls" className="rounded" checked />
                    <label htmlFor="tls" className="text-sm text-gray-700 dark:text-gray-300">Use TLS</label>
                  </div>
                </div>
              )}

              {activeTab === 'features' && (
                <div className="space-y-6 max-w-2xl">
                  <p className="text-gray-600 dark:text-gray-400">Enable or disable features for your project</p>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    {[
                      { id: 'auth', name: 'Authentication', enabled: true },
                      { id: 'api', name: 'REST API', enabled: true },
                      { id: 'workflows', name: 'Workflows', enabled: true },
                      { id: 'realtime', name: 'Real-time', enabled: false },
                      { id: 'files', name: 'File Storage', enabled: true },
                      { id: 'email', name: 'Email', enabled: true },
                      { id: 'webhooks', name: 'Webhooks', enabled: false },
                      { id: 'scheduler', name: 'Task Scheduler', enabled: false },
                      { id: 'audit', name: 'Audit Log', enabled: true },
                      { id: 'softDelete', name: 'Soft Delete', enabled: true },
                      { id: 'multiTenant', name: 'Multi-tenancy', enabled: false },
                      { id: 'localization', name: 'Localization', enabled: false },
                      { id: 'theming', name: 'Theming', enabled: true },
                      { id: 'plugins', name: 'Plugins', enabled: true },
                      { id: 'marketplace', name: 'Marketplace', enabled: false },
                      { id: 'ai', name: 'AI Assistance', enabled: false },
                    ].map((feature) => (
                      <label key={feature.id} className="flex items-center gap-3 p-4 border border-gray-200 dark:border-slate-700 rounded-lg hover:bg-gray-50 dark:hover:bg-slate-700/50 cursor-pointer">
                        <input
                          type="checkbox"
                          className="rounded border-gray-300"
                          defaultChecked={feature.enabled}
                        />
                        <span className="font-medium text-gray-900 dark:text-white">{feature.name}</span>
                      </label>
                    ))}
                  </div>
                </div>
              )}

              {['api', 'storage', 'notifications'].includes(activeTab) && (
                <div className="max-w-2xl">
                  <p className="text-gray-600 dark:text-gray-400">{activeTab.charAt(0).toUpperCase() + activeTab.slice(1)} settings coming soon...</p>
                </div>
              )}
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}
