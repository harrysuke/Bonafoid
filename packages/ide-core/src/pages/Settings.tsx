/**
 * Project settings.
 *
 * Only settings that actually affect the generated application or the IDE are
 * shown. Tabs for authentication, email, API, storage and notifications were
 * removed: their inputs were inert and nothing in codegen reads
 * `settings.authentication`, `settings.email` and friends, so editing them
 * would have looked like configuration while changing nothing.
 */

import { useState } from 'react';
import { Save, Database, Globe, Code, AlertTriangle } from 'lucide-react';
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
];

/** Drivers the generated backend is validated against end to end. */
const VALIDATED_DRIVERS = new Set(['postgres', 'sqlite']);

export function Settings() {
  const [activeTab, setActiveTab] = useState('general');
  const [saved, setSaved] = useState(false);
  const {
    project,
    isDirty,
    setDbDriver,
    setFrontendFramework,
    setDatabaseUrl,
    updateProjectMeta,
    saveProject,
  } = useProjectStore();

  const driver = project?.settings.database.driver ?? 'sqlite';
  const url = project?.settings.database.url ?? '';
  const framework = project?.settings.frontend.framework ?? 'leptos';
  const bundler = project?.settings.frontend.bundler ?? 'trunk';

  const handleSave = async () => {
    await saveProject();
    setSaved(true);
    setTimeout(() => setSaved(false), 2500);
  };

  if (!project) {
    return (
      <div className="flex items-center justify-center h-64 text-gray-500 dark:text-gray-400">
        Open a project to configure it.
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">Project Settings</h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">
            These values are written into the generated application.
          </p>
        </div>
        <div className="flex items-center gap-3">
          {isDirty && (
            <span className="text-sm text-amber-600 dark:text-amber-400">Unsaved changes</span>
          )}
          {saved && (
            <span className="text-sm text-emerald-600 dark:text-emerald-400">Saved</span>
          )}
          <Button variant="primary" onClick={() => void handleSave()}>
            <Save className="w-4 h-4 mr-2" /> Save Changes
          </Button>
        </div>
      </div>

      <div className="flex gap-6">
        <div className="w-48 shrink-0">
          <nav className="space-y-1">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                aria-current={activeTab === tab.id}
                className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-sm transition-colors ${
                  activeTab === tab.id
                    ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300'
                    : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-700/50'
                }`}
              >
                <tab.icon className="w-4 h-4" /> {tab.label}
              </button>
            ))}
          </nav>
        </div>

        <Card className="flex-1 p-6">
          {activeTab === 'general' && (
            <div className="space-y-6 max-w-2xl">
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Project Name
                </label>
                <Input
                  value={project.name}
                  onChange={(e) => updateProjectMeta({ name: e.target.value })}
                />
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Renaming also rewrites the slug, which the generated crate name
                  is derived from.
                </p>
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Slug
                </label>
                <Input value={project.slug} readOnly />
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Description
                </label>
                <textarea
                  className="input h-24"
                  value={project.description ?? ''}
                  onChange={(e) => updateProjectMeta({ description: e.target.value })}
                />
              </div>
            </div>
          )}

          {activeTab === 'database' && (
            <div className="space-y-6 max-w-2xl">
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Database Driver
                </label>
                <Select
                  value={driver}
                  onChange={(e) => void setDbDriver(e.target.value)}
                  options={DB_DRIVERS}
                />
                {!VALIDATED_DRIVERS.has(driver) && (
                  <p className="flex items-start gap-2 text-sm text-amber-600 dark:text-amber-400 mt-2">
                    <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
                    Only PostgreSQL and SQLite are validated end to end. Other
                    drivers generate, but previews are not verified against them.
                  </p>
                )}
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Connection URL
                </label>
                <Input
                  value={url}
                  onChange={(e) => setDatabaseUrl(e.target.value)}
                  placeholder={driver === 'sqlite' ? 'sqlite://app.db' : 'postgres://user@host/db'}
                />
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Stored in project.json and compiled into the generated backend.
                  Previews never receive this URL: a preview runs generated code,
                  so it uses a throwaway database instead.
                </p>
              </div>
            </div>
          )}

          {activeTab === 'frontend' && (
            <div className="space-y-6 max-w-2xl">
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Framework
                </label>
                <Select
                  value={framework}
                  onChange={(e) => void setFrontendFramework(e.target.value)}
                  options={FRONTEND_FRAMEWORKS}
                />
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Generated frontends are Leptos (Rust compiled to WebAssembly).
                </p>
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Bundler
                </label>
                <Select value={bundler} options={BUNDLERS} disabled />
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Trunk is the supported build tool for the Leptos frontend.
                </p>
              </div>
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}