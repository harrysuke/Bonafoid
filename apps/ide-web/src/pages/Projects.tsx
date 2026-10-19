import { useCallback, useEffect, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { FileText, FolderOpen, LogOut, Plus, Trash2, Copy, Download, Upload } from 'lucide-react';
import type { PlatformProject } from '@nocode/ide-core';
import { getPlatform } from '@nocode/ide-core';
import { ApiRequestError, projectsApi } from '../api/client';
import { logout } from '../auth/session';

export function Projects() {
  const navigate = useNavigate();
  const [projects, setProjects] = useState<PlatformProject[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const platform = getPlatform();
      setProjects((await platform.listProjects?.()) ?? []);
      setError(null);
    } catch (caught) {
      setError(caught instanceof ApiRequestError ? caught.message : 'Could not load projects.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const openProject = async (id: string) => {
    setBusyId(id);
    setError(null);
    try {
      const platform = getPlatform();
      await platform.openProjectById?.(id);
      navigate('/');
    } catch (caught) {
      setError(caught instanceof ApiRequestError ? caught.message : 'Could not open the project.');
    } finally {
      setBusyId(null);
    }
  };

  const createProject = async (name: string) => {
    setCreating(true);
    setError(null);
    try {
      const platform = getPlatform();
      await platform.createProject?.(name);
      await refresh();
    } catch (caught) {
      setError(caught instanceof ApiRequestError ? caught.message : 'Could not create the project.');
    } finally {
      setCreating(false);
    }
  };

  const removeProject = async (id: string) => {
    setBusyId(id);
    try {
      // Through the platform adapter, not projectsApi directly: it also clears
      // the working copy when the deleted project is the one currently open.
      await getPlatform().deleteProject?.(id);
      await refresh();
    } catch (caught) {
      setError(caught instanceof ApiRequestError ? caught.message : 'Could not delete the project.');
    } finally {
      setBusyId(null);
    }
  };

  const duplicateProject = async (id: string) => {
    setBusyId(id);
    try {
      await projectsApi.duplicate(id);
      await refresh();
    } catch (caught) {
      setError(caught instanceof ApiRequestError ? caught.message : 'Could not duplicate it.');
    } finally {
      setBusyId(null);
    }
  };

  const importProject = async (file: File) => {
    setBusyId('import');
    try {
      const project = JSON.parse(await file.text()) as unknown;
      await projectsApi.import(project);
      await refresh();
    } catch (caught) {
      setError(
        caught instanceof ApiRequestError ? caught.message : 'That file is not a valid project.',
      );
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-slate-900">
      <header className="bg-white dark:bg-slate-800 border-b border-gray-200 dark:border-slate-700">
        <div className="max-w-4xl mx-auto px-6 py-4 flex items-center justify-between">
          <h1 className="text-lg font-bold text-gray-900 dark:text-white">Projects</h1>
          <button
            type="button"
            onClick={() => void logout().then(() => navigate('/login', { replace: true }))}
            className="flex items-center gap-2 text-sm text-gray-500 hover:text-gray-800 dark:text-gray-400 dark:hover:text-gray-200"
          >
            <LogOut className="w-4 h-4" /> Sign out
          </button>
        </div>
      </header>

      <main className="max-w-4xl mx-auto px-6 py-8 space-y-6">
        {error && (
          <p role="alert" className="text-sm text-red-600 dark:text-red-400">
            {error}
          </p>
        )}

        <CreateProjectRow busy={creating} onCreate={createProject} />

        <div className="flex items-center gap-3 text-sm text-gray-500 dark:text-gray-400">
          <label className="flex items-center gap-2 cursor-pointer hover:text-gray-700 dark:hover:text-gray-200">
            <Upload className="w-4 h-4" /> Import project
            <input
              type="file"
              accept="application/json,.json"
              className="hidden"
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (file) void importProject(file);
                event.target.value = '';
              }}
            />
          </label>
          {busyId === 'import' && <span className="text-xs">Importing…</span>}
        </div>

        {loading ? (
          <p className="text-sm text-gray-500 dark:text-gray-400">Loading projects…</p>
        ) : projects.length === 0 ? (
          <p className="text-sm text-gray-500 dark:text-gray-400">
            No projects yet. Create your first one above.
          </p>
        ) : (
          <ul className="space-y-2">
            {projects.map((project) => (
              <li
                key={project.id}
                className="flex items-center justify-between gap-3 rounded-xl border border-gray-200 bg-white px-4 py-3 dark:border-slate-700 dark:bg-slate-800"
              >
                <div className="flex items-center gap-3 min-w-0">
                  <FileText className="w-5 h-5 text-primary-500 shrink-0" />
                  <div className="min-w-0">
                    <p className="truncate font-medium text-gray-900 dark:text-white">
                      {project.name}
                    </p>
                    <p className="truncate text-xs text-gray-400 dark:text-gray-500">
                      {project.description || 'No description'}
                      {project.updatedAt ? ` · updated ${project.updatedAt.slice(0, 10)}` : ''}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-1">
                  <a
                    href={projectsApi.exportUrl(project.id)}
                    target="_blank"
                    rel="noreferrer"
                    title="Export JSON"
                    className="p-2 text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
                  >
                    <Download className="w-4 h-4" />
                  </a>
                  <button
                    type="button"
                    onClick={() => void duplicateProject(project.id)}
                    disabled={busyId === project.id}
                    title="Duplicate"
                    className="p-2 text-gray-400 hover:text-gray-700 disabled:opacity-50 dark:hover:text-gray-200"
                  >
                    <Copy className="w-4 h-4" />
                  </button>
                  <button
                    type="button"
                    onClick={() => void removeProject(project.id)}
                    disabled={busyId === project.id}
                    title="Delete"
                    className="p-2 text-gray-400 hover:text-red-600 disabled:opacity-50"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                  <button
                    type="button"
                    onClick={() => void openProject(project.id)}
                    disabled={busyId === project.id}
                    className="flex items-center gap-1 rounded-lg bg-primary-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-primary-700 disabled:opacity-60"
                  >
                    <FolderOpen className="w-4 h-4" /> Open
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}

        <p className="text-xs text-gray-400 dark:text-gray-500">
          Looking for the editor? Open a project first. <Link to="/" className="underline">Editor</Link>
        </p>
      </main>
    </div>
  );
}

function CreateProjectRow({
  busy,
  onCreate,
}: {
  busy: boolean;
  onCreate: (name: string) => Promise<void>;
}) {
  const [name, setName] = useState('');

  return (
    <form
      className="flex flex-wrap gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        const trimmed = name.trim();
        if (!trimmed) return;
        void onCreate(trimmed).then(() => setName(''));
      }}
    >
      <input
        value={name}
        onChange={(event) => setName(event.target.value)}
        placeholder="New project name"
        aria-label="New project name"
        className="flex-1 min-w-[12rem] px-3 py-2 rounded-lg border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-gray-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-primary-500"
      />
      <button
        type="submit"
        disabled={busy}
        className="flex items-center gap-2 px-4 py-2 rounded-lg bg-primary-600 text-white font-medium hover:bg-primary-700 disabled:opacity-60"
      >
        <Plus className="w-4 h-4" /> Create
      </button>
    </form>
  );
}