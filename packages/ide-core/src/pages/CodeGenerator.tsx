/**
 * Code generator.
 *
 * Everything on this screen is real: the counts come from the project model,
 * the numbers after a run come from the generator's own report, and there are
 * no controls that pretend to change something they do not. Codegen always
 * produces the backend, frontend, database, docker and kubernetes trees, so
 * the screen describes the output rather than offering target checkboxes that
 * were never read.
 */

import { useState } from 'react';
import { FolderOpen, Loader2, Play, Terminal, X } from 'lucide-react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { useProjectStore } from '../stores/projectStore';
import { DB_DRIVERS } from '../types/project';
import { usePlatform } from '../platform/PlatformContext';
import type { GenerationReport } from '../types/project';

export function CodeGenerator() {
  const { project, generate, setDbDriver } = useProjectStore();
  const platform = usePlatform();

  const [outputDir, setOutputDir] = useState('./generated');
  const [status, setStatus] = useState<'idle' | 'generating' | 'success' | 'error'>('idle');
  const [logs, setLogs] = useState<string[]>([]);
  const [report, setReport] = useState<GenerationReport | null>(null);

  const driver = project?.settings.database.driver ?? 'sqlite';

  const addLog = (message: string) =>
    setLogs((previous) => [...previous, `[${new Date().toLocaleTimeString()}] ${message}`]);

  const browseOutputDir = async () => {
    try {
      const dir = await platform.pickDirectory();
      if (dir) setOutputDir(dir);
    } catch (error) {
      addLog(`ERROR: ${(error as Error).message}`);
    }
  };

  const runGeneration = async () => {
    if (!project) {
      addLog('ERROR: No project loaded');
      setStatus('error');
      return;
    }

    setStatus('generating');
    setLogs([]);
    setReport(null);
    addLog(`Generating into ${outputDir} for driver ${driver}...`);

    try {
      const result = await generate(outputDir);
      setReport(result);
      addLog(`Backend files:  ${result.backend_files}`);
      addLog(`Frontend files: ${result.frontend_files}`);
      addLog(`Migrations:      ${result.migrations}`);
      addLog(`Output written to ${result.output_dir}`);
      setStatus('success');
    } catch (error) {
      addLog(`ERROR: ${(error as Error).message}`);
      setStatus('error');
    }
  };

  const inventory = [
    { label: 'Data models', value: Object.keys(project?.entities ?? {}).length },
    { label: 'Forms', value: Object.keys(project?.forms ?? {}).length },
    { label: 'Pages', value: Object.keys(project?.pages ?? {}).length },
    { label: 'Workflows', value: Object.keys(project?.workflows ?? {}).length },
    { label: 'Themes', value: Object.keys(project?.themes ?? {}).length },
  ];

  const empty = inventory.every((entry) => entry.value === 0);

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <div className="flex items-center gap-3">
          <Terminal className="w-5 h-5 text-primary-600" />
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Code Generator</h2>
        </div>
        <Button
          variant="primary"
          size="sm"
          onClick={runGeneration}
          disabled={status === 'generating' || !project}
        >
          {status === 'generating' ? (
            <>
              <Loader2 className="w-4 h-4 mr-1 animate-spin" /> Generating...
            </>
          ) : (
            <>
              <Play className="w-4 h-4 mr-1" /> Generate Code
            </>
          )}
        </Button>
      </div>

      <div className="flex-1 flex overflow-hidden">
        <div className="w-80 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Generation Config</h3>

          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                Output Directory
              </label>
              <div className="flex gap-2">
                <Input value={outputDir} onChange={(e) => setOutputDir(e.target.value)} />
                <Button variant="secondary" size="sm" onClick={() => void browseOutputDir()}>
                  <FolderOpen className="w-4 h-4" />
                </Button>
              </div>
              <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                The generated backend is compiled against this database driver, so
                the preview build uses it too.
              </p>
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                Database
              </label>
              <Select
                value={driver}
                onChange={(e) => void setDbDriver(e.target.value)}
                options={DB_DRIVERS}
              />
            </div>
          </div>

          <Card className="p-4 mt-6">
            <h4 className="font-medium text-gray-900 dark:text-white mb-3">
              What will be generated
            </h4>
            {empty ? (
              <p className="text-sm text-gray-500 dark:text-gray-400">
                The project has no entities, forms or pages yet. Add some and come
                back.
              </p>
            ) : (
              <ul className="space-y-1 text-sm">
                {inventory.map((entry) => (
                  <li
                    key={entry.label}
                    className="flex items-center justify-between text-gray-700 dark:text-gray-300"
                  >
                    <span>{entry.label}</span>
                    <span className="font-mono">{entry.value}</span>
                  </li>
                ))}
              </ul>
            )}
            <p className="mt-3 text-xs text-gray-500 dark:text-gray-400">
              Codegen emits the backend, frontend, database, docker and
              kubernetes trees for every run.
            </p>
          </Card>
        </div>

        <div className="flex-1 p-6 overflow-y-auto bg-gray-100 dark:bg-slate-900">
          <div className="max-w-4xl mx-auto space-y-6">
            <div className="flex items-center justify-between">
              <h2 className="text-2xl font-bold text-gray-900 dark:text-white">Output</h2>
              <span
                className={`px-2 py-1 text-xs font-medium rounded-full ${
                  status === 'success'
                    ? 'bg-green-100 text-green-800'
                    : status === 'error'
                      ? 'bg-red-100 text-red-800'
                      : status === 'generating'
                        ? 'bg-yellow-100 text-yellow-800'
                        : 'bg-gray-100 text-gray-800'
                }`}
              >
                {status === 'generating'
                  ? 'Generating...'
                  : status === 'success'
                    ? 'Ready'
                    : status === 'error'
                      ? 'Failed'
                      : 'Idle'}
              </span>
            </div>

            {report && (
              <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                <div className="p-4 bg-blue-50 dark:bg-blue-900/30 rounded-lg">
                  <p className="text-2xl font-bold text-blue-600 dark:text-blue-400">
                    {report.backend_files}
                  </p>
                  <p className="text-sm text-blue-600 dark:text-blue-400">Backend Files</p>
                </div>
                <div className="p-4 bg-green-50 dark:bg-green-900/30 rounded-lg">
                  <p className="text-2xl font-bold text-green-600 dark:text-green-400">
                    {report.frontend_files}
                  </p>
                  <p className="text-sm text-green-600 dark:text-green-400">Frontend Files</p>
                </div>
                <div className="p-4 bg-yellow-50 dark:bg-yellow-900/30 rounded-lg">
                  <p className="text-2xl font-bold text-yellow-600 dark:text-yellow-400">
                    {report.migrations}
                  </p>
                  <p className="text-sm text-yellow-600 dark:text-yellow-400">Migrations</p>
                </div>
                <div className="p-4 bg-gray-50 dark:bg-slate-800 rounded-lg">
                  <p className="text-sm font-mono text-gray-700 dark:text-gray-300 break-all">
                    {report.output_dir}
                  </p>
                  <p className="text-sm text-gray-600 dark:text-gray-400">Output</p>
                </div>
              </div>
            )}

            <Card className="p-4">
              <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Running it</h3>
              <div className="space-y-1 font-mono text-sm bg-gray-900 dark:bg-gray-950 p-3 rounded-lg text-green-400 overflow-x-auto">
                <div>cd {report?.output_dir ?? outputDir}</div>
                <div>cd backend &amp;&amp; cargo run</div>
                <div>cd frontend &amp;&amp; trunk serve</div>
              </div>
              <p className="mt-3 text-xs text-gray-500 dark:text-gray-400">
                The preview panel builds and runs the same tree without leaving
                the IDE.
              </p>
            </Card>

            {logs.length > 0 && (
              <Card className="p-4">
                <div className="flex items-center justify-between mb-4">
                  <h3 className="font-semibold text-gray-900 dark:text-white">Generation Log</h3>
                  <Button variant="ghost" size="sm" onClick={() => setLogs([])}>
                    <X className="w-4 h-4" /> Clear
                  </Button>
                </div>
                <div className="font-mono text-xs bg-gray-900 dark:bg-gray-950 p-4 rounded-lg max-h-64 overflow-y-auto text-green-400">
                  {logs.map((log, index) => (
                    <div key={index} className="text-gray-300">
                      {log}
                    </div>
                  ))}
                </div>
              </Card>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}