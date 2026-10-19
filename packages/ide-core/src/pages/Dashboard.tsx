import { useCallback, useEffect, useState } from 'react';
import {
  Database,
  FileText,
  GitBranch,
  Layout,
  Plus,
  Rows,
} from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { getPlatform } from '../platform/registry';
import type { PlatformProject } from '../platform/types';
import { useProjectStore } from '../stores/projectStore';

function count(value: Record<string, unknown> | undefined): number {
  return Object.keys(value ?? {}).length;
}

/** Relative time for the "last updated" column. */
function relativeTime(iso: string | undefined): string {
  if (!iso) return 'unknown';
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return 'unknown';
  const seconds = Math.max(0, Math.round((Date.now() - then) / 1000));
  const units: [number, string][] = [
    [60, 'second'],
    [60, 'minute'],
    [24, 'hour'],
    [7, 'day'],
    [4.348, 'week'],
  ];
  let value = seconds;
  let label = 'second';
  for (const [step, unit] of units) {
    if (value < step) break;
    value /= step;
    label = unit;
  }
  const rounded = Math.floor(value);
  if (rounded <= 0) return 'just now';
  return `${rounded} ${label}${rounded === 1 ? '' : 's'} ago`;
}

export function Dashboard() {
  const navigate = useNavigate();
  const project = useProjectStore((s) => s.project);
  const isDirty = useProjectStore((s) => s.isDirty);

  const [projects, setProjects] = useState<PlatformProject[] | null>(null);
  const [listingError, setListingError] = useState<string | null>(null);

  const platform = getPlatform();
  const canListProjects = typeof platform.listProjects === 'function';

  // Recent projects live on the server. The desktop host has no project list
  // (it opens one project at a time from disk), so it gets an honest empty
  // state rather than invented rows.
  const refresh = useCallback(async () => {
    if (!canListProjects) {
      setProjects(null);
      return;
    }
    try {
      const list = (await platform.listProjects?.()) ?? [];
      setProjects(list);
      setListingError(null);
    } catch (error) {
      setProjects(null);
      setListingError(error instanceof Error ? error.message : 'Could not load projects.');
    }
  }, [canListProjects, platform]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const stats = [
    { title: 'Forms', value: count(project?.forms), icon: FileText, color: 'text-blue-600', bg: 'bg-blue-100', href: '/forms' },
    { title: 'Workflows', value: count(project?.workflows), icon: GitBranch, color: 'text-purple-600', bg: 'bg-purple-100', href: '/workflows' },
    { title: 'Data Models', value: count(project?.entities), icon: Database, color: 'text-green-600', bg: 'bg-green-100', href: '/data' },
    { title: 'Pages', value: count(project?.pages), icon: Layout, color: 'text-orange-600', bg: 'bg-orange-100', href: '/pages' },
  ];

  const quickActions = [
    { title: 'Create Form', description: 'Build data collection forms', icon: FileText, href: '/forms' },
    { title: 'Design Workflow', description: 'Visual process automation', icon: GitBranch, href: '/workflows' },
    { title: 'Model Data', description: 'Define entities and relations', icon: Database, href: '/data' },
    { title: 'Build Pages', description: 'Create responsive UI', icon: Layout, href: '/pages' },
  ];

const openProject = async (id: string) => {
    await platform.openProjectById?.(id);
    navigate('/');
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white">
            {project?.name ?? 'Dashboard'}
          </h1>
          <p className="text-gray-600 dark:text-gray-400 mt-1">
            {project?.description ?? 'Overview of the current project'}
          </p>
        </div>
        {isDirty && (
          <span className="text-xs text-amber-600 dark:text-amber-400">Unsaved changes</span>
        )}
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {stats.map((stat) => (
          <Card key={stat.title} className="p-6">
            <button
              type="button"
              onClick={() => navigate(stat.href)}
              className="w-full flex items-center justify-between text-left"
            >
              <div>
                <p className="text-sm font-medium text-gray-600 dark:text-gray-400">{stat.title}</p>
                <p className="text-3xl font-bold text-gray-900 dark:text-white mt-1">{stat.value}</p>
              </div>
              <div className={`p-3 rounded-lg ${stat.bg} dark:bg-opacity-30`}>
                <stat.icon className={`w-6 h-6 ${stat.color}`} />
              </div>
            </button>
          </Card>
        ))}
      </div>

      {/* Quick Actions */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {quickActions.map((action) => (
          <Card key={action.title} className="p-6 hover:shadow-md transition-shadow">
            <button
              type="button"
              onClick={() => navigate(action.href)}
              className="w-full flex items-start gap-4 text-left"
            >
              <div className="p-3 bg-primary-100 dark:bg-primary-900/30 rounded-lg">
                <action.icon className="w-6 h-6 text-primary-600 dark:text-primary-400" />
              </div>
              <div className="flex-1">
                <h3 className="font-semibold text-gray-900 dark:text-white">{action.title}</h3>
                <p className="text-sm text-gray-600 dark:text-gray-400 mt-1">{action.description}</p>
              </div>
            </button>
          </Card>
        ))}
      </div>

      {/* Projects owned by this user */}
      <Card>
        <div className="p-6 border-b border-gray-200 dark:border-slate-700 flex items-center justify-between">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Your projects</h2>
          <Button variant="ghost" size="sm" onClick={() => navigate('/lists')}>
            <Rows className="w-4 h-4 mr-1" /> Lists
          </Button>
        </div>

        {!canListProjects && (
          <p className="p-6 text-sm text-gray-500 dark:text-gray-400">
            This host opens one project at a time. Use the desktop shell to switch
            projects, or open the web IDE to manage several.
          </p>
        )}

        {canListProjects && listingError && (
          <p className="p-6 text-sm text-red-600 dark:text-red-400">{listingError}</p>
        )}

        {canListProjects && !listingError && projects !== null && projects.length === 0 && (
          <p className="p-6 text-sm text-gray-500 dark:text-gray-400">
            No projects yet. Create one to get started.
          </p>
        )}

        {projects !== null && projects.length > 0 && (
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead>
                <tr className="border-b border-gray-200 dark:border-slate-700">
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Project
                  </th>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Last Updated
                  </th>
                  <th className="px-6 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                    Actions
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                {projects.map((entry) => (
                  <tr key={entry.id} className="hover:bg-gray-50 dark:hover:bg-slate-800/50">
                    <td className="px-6 py-4">
                      <div className="font-medium text-gray-900 dark:text-white">{entry.name}</div>
                      {entry.description && (
                        <div className="text-xs text-gray-500 dark:text-gray-400">
                          {entry.description}
                        </div>
                      )}
                    </td>
                    <td className="px-6 py-4 text-gray-600 dark:text-gray-400">
                      {relativeTime(entry.updatedAt)}
                    </td>
<td className="px-6 py-4 text-right">
                      <div className="flex items-center justify-end gap-2">
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => void openProject(entry.id)}
                        >
                          Open
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>

      <p className="text-xs text-gray-400 dark:text-gray-500">
        <Plus className="w-3 h-3 inline" /> Create and switch projects from the projects screen.
      </p>
    </div>
  );
}