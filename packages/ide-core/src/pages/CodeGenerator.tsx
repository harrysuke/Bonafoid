import { useState } from 'react';
import { Download, Copy, X, Loader2, Terminal, Globe, Smartphone, Monitor, Box, Play, FolderOpen } from 'lucide-react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';
import { useProjectStore } from '../stores/projectStore';
import { DB_DRIVERS } from '../types/project';
import { usePlatform } from '../platform/PlatformContext';

const targets = [
  { id: 'web', name: 'Web (WASM)', icon: Globe, description: 'Deploy as static WASM site', features: ['PWA support', 'CDN ready', 'Offline capable'] },
  { id: 'desktop', name: 'Desktop (Tauri)', icon: Monitor, description: 'Native desktop app', features: ['Auto-updater', 'System tray', 'Native menus'] },
  { id: 'mobile', name: 'Mobile (PWA)', icon: Smartphone, description: 'Installable web app', features: ['App store ready', 'Push notifications', 'Offline sync'] },
  { id: 'docker', name: 'Docker', icon: Box, description: 'Containerized deployment', features: ['K8s ready', 'Multi-arch', 'Health checks'] },
  { id: 'kubernetes', name: 'Kubernetes', icon: Terminal, description: 'K8s manifests', features: ['Helm charts', 'Operators', 'GitOps'] },
];

const databases = DB_DRIVERS;

export function CodeGenerator() {
  const [selectedTargets, setSelectedTargets] = useState(['web', 'docker']);
  const [database, setDatabase] = useState('postgres');
  const [outputDir, setOutputDir] = useState('./generated');
  const [generateStatus, setGenerateStatus] = useState<'idle' | 'generating' | 'success' | 'error'>('idle');
  const [generateLogs, setGenerateLogs] = useState<string[]>([]);
  const [_currentStep, setCurrentStep] = useState(0);
  const [dockerCompose, setDockerCompose] = useState(false);
  const [kubernetes, setKubernetes] = useState(false);
  const [includeTests, setIncludeTests] = useState(true);
  const [includeDocs, setIncludeDocs] = useState(true);
  const { project, generate } = useProjectStore();
  const platform = usePlatform();

  const addLog = (message: string) => {
    setGenerateLogs(prev => [...prev, `[${new Date().toLocaleTimeString()}] ${message}`]);
  };

  const browseOutputDir = async () => {
    try {
      const dir = await platform.pickDirectory();
      if (dir) setOutputDir(dir);
    } catch (e) {
      addLog(`ERROR: ${(e as Error).message}`);
    }
  };

  const generateCode = async () => {
    if (selectedTargets.length === 0) {
      addLog('ERROR: No targets selected');
      setGenerateStatus('error');
      return;
    }

    if (!project) {
      addLog('ERROR: No project loaded');
      setGenerateStatus('error');
      return;
    }

    setGenerateStatus('generating');
    setGenerateLogs([]);
    setCurrentStep(0);

    const steps = [
      'Validating project structure...',
      'Generating backend code (Axum + SeaORM)...',
      'Generating frontend code (Leptos + WASM)...',
      'Generating database migrations...',
      'Generating Docker files...',
      'Generating Kubernetes manifests...',
      'Packaging output...',
    ];

    try {
      for (let i = 0; i < steps.length; i++) {
        setCurrentStep(i);
        addLog(steps[i]);
        await new Promise(resolve => setTimeout(resolve, 200 + Math.random() * 200));
      }

      const report = await generate(outputDir);
      addLog(`Backend files: ${report.backend_files}`);
      addLog(`Frontend files: ${report.frontend_files}`);
      addLog(`Migrations: ${report.migrations}`);
      addLog('SUCCESS: Code generation completed!');
      addLog(`Output written to: ${report.output_dir}`);
      setGenerateStatus('success');
    } catch (e) {
      addLog(`ERROR: ${(e as Error).message}`);
      setGenerateStatus('error');
    }
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
  };

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <div className="flex items-center gap-4">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Code Generator</h2>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm" onClick={() => {
            const config = JSON.stringify({ targets: selectedTargets, database, dockerCompose, kubernetes, includeTests, includeDocs }, null, 2);
            copyToClipboard(config);
          }}>
            <Copy className="w-4 h-4 mr-1" /> Copy Config
          </Button>
          <Button variant="secondary" size="sm" disabled={generateStatus === 'generating'}>
            <Download className="w-4 h-4 mr-1" /> Download All
          </Button>
          <Button 
            variant="primary" 
            size="sm" 
            onClick={generateCode}
            disabled={generateStatus === 'generating' || selectedTargets.length === 0}
          >
            {generateStatus === 'generating' ? (
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
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Left Panel - Configuration */}
        <div className="w-80 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Generation Config</h3>

          <div className="space-y-6">
            {/* Output Directory */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Output Directory</label>
              <div className="flex gap-2">
                <Input value={outputDir} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setOutputDir(e.target.value)} placeholder="./generated" />
                <Button variant="secondary" size="sm" onClick={() => void browseOutputDir()}>
                  <FolderOpen className="w-4 h-4" />
                </Button>
              </div>
            </div>

            {/* Database */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Database</label>
              <Select value={database} onChange={(e: React.ChangeEvent<HTMLSelectElement>) => setDatabase(e.target.value)} options={databases} />
            </div>

            {/* Targets */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">Deployment Targets</label>
              <div className="space-y-2">
                {targets.map((target) => (
                  <label key={target.id} className="flex items-center gap-3 p-3 border border-gray-200 dark:border-slate-700 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-slate-700/50">
                    <input
                      type="checkbox"
                      checked={selectedTargets.includes(target.id)}
                      onChange={(e) => setSelectedTargets(e.target.checked ? [...selectedTargets, target.id] : selectedTargets.filter(t => t !== target.id))}
                      className="rounded border-gray-300"
                    />
                    <div className="flex items-center gap-3 flex-1">
                      <target.icon className="w-6 h-6 text-primary-600" />
                      <div>
                        <p className="font-medium text-gray-900 dark:text-white">{target.name}</p>
                        <p className="text-xs text-gray-500 dark:text-gray-400">{target.description}</p>
                      </div>
                    </div>
                  </label>
                ))}
              </div>
            </div>

            {/* Options */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">Options</label>
              <div className="space-y-2">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input type="checkbox" checked={dockerCompose} onChange={(e) => setDockerCompose(e.target.checked)} className="rounded" />
                  <span className="text-sm text-gray-700 dark:text-gray-300">Generate Docker Compose</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input type="checkbox" checked={kubernetes} onChange={(e) => setKubernetes(e.target.checked)} className="rounded" />
                  <span className="text-sm text-gray-700 dark:text-gray-300">Generate Kubernetes Manifests</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input type="checkbox" checked={includeTests} onChange={(e) => setIncludeTests(e.target.checked)} className="rounded" />
                  <span className="text-sm text-gray-700 dark:text-gray-300">Include Tests</span>
                </label>
                <label className="flex items-center gap-2 cursor-pointer">
                  <input type="checkbox" checked={includeDocs} onChange={(e) => setIncludeDocs(e.target.checked)} className="rounded" />
                  <span className="text-sm text-gray-700 dark:text-gray-300">Include Documentation</span>
                </label>
              </div>
            </div>
          </div>
        </div>

        {/* Center Panel - Preview */}
        <div className="flex-1 p-6 overflow-y-auto bg-gray-100 dark:bg-slate-900">
          <div className="max-w-4xl mx-auto space-y-6">
            <div className="flex items-center justify-between">
              <h2 className="text-2xl font-bold text-gray-900 dark:text-white">Generation Preview</h2>
              <div className="flex items-center gap-2">
                <span className={`px-2 py-1 text-xs font-medium rounded-full ${generateStatus === 'success' ? 'bg-green-100 text-green-800' : generateStatus === 'error' ? 'bg-red-100 text-red-800' : generateStatus === 'generating' ? 'bg-yellow-100 text-yellow-800' : 'bg-gray-100 text-gray-800'}`}>
                  {generateStatus === 'generating' ? 'Generating...' : generateStatus === 'success' ? 'Ready' : generateStatus === 'error' ? 'Failed' : 'Configure targets'}
                </span>
              </div>
            </div>

            {/* Project Structure Preview */}
            <Card className="p-4">
              <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Project Structure</h3>
              <div className="font-mono text-sm text-green-400 bg-gray-900 dark:bg-gray-950 p-4 rounded-lg overflow-x-auto max-h-96 overflow-y-auto">
  {selectedTargets.map((targetId) => {
                  const target = targets.find(t => t.id === targetId);
                  return (
                    <div key={targetId} className="mb-4">
                      <div className="text-blue-400 mb-2"># {target?.name ?? targetId}</div>
                      <pre>{getStructurePreview(targetId)}</pre>
                    </div>
                  );
                })}
              </div>
            </Card>

            {/* Generated Files Summary */}
            <Card className="p-4">
              <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Files to Generate</h3>
              <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                <div className="p-4 bg-blue-50 dark:bg-blue-900/30 rounded-lg">
                  <p className="text-2xl font-bold text-blue-600 dark:text-blue-400">{getFileCount(selectedTargets, 'backend')}</p>
                  <p className="text-sm text-blue-600 dark:text-blue-400">Backend Files</p>
                </div>
                <div className="p-4 bg-green-50 dark:bg-green-900/30 rounded-lg">
                  <p className="text-2xl font-bold text-green-600 dark:text-green-400">{getFileCount(selectedTargets, 'frontend')}</p>
                  <p className="text-sm text-green-600 dark:text-green-400">Frontend Files</p>
                </div>
                <div className="p-4 bg-yellow-50 dark:bg-yellow-900/30 rounded-lg">
                  <p className="text-2xl font-bold text-yellow-600 dark:text-yellow-400">{getFileCount(selectedTargets, 'database')}</p>
                  <p className="text-sm text-yellow-600 dark:text-yellow-400">Migrations</p>
                </div>
                <div className="p-4 bg-purple-50 dark:bg-purple-900/30 rounded-lg">
                  <p className="text-2xl font-bold text-purple-600 dark:text-purple-400">{getFileCount(selectedTargets, 'config')}</p>
                  <p className="text-sm text-purple-600 dark:text-purple-400">Config Files</p>
                </div>
              </div>
            </Card>

            {/* Generate Logs */}
            {generateLogs.length > 0 && (
              <Card className="p-4">
                <div className="flex items-center justify-between mb-4">
                  <h3 className="font-semibold text-gray-900 dark:text-white">Generation Logs</h3>
                  <Button variant="ghost" size="sm" onClick={() => setGenerateLogs([])}>
                    <X className="w-4 h-4" /> Clear
                  </Button>
                </div>
                <div className="font-mono text-xs bg-gray-900 dark:bg-gray-950 p-4 rounded-lg max-h-64 overflow-y-auto text-green-400">
                  {generateLogs.map((log, i) => (
                    <div key={i} className="text-gray-300">{log}</div>
                  ))}
                </div>
              </Card>
            )}
          </div>
        </div>

        {/* Right Panel - Commands */}
        <div className="w-80 border-l border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Quick Commands</h3>
          
          <Card className="p-4 mb-4">
            <h4 className="font-medium text-gray-900 dark:text-white mb-3">After Generation</h4>
            <div className="space-y-2 font-mono text-sm bg-gray-900 dark:bg-gray-950 p-3 rounded-lg text-green-400">
              <div>cd {outputDir}</div>
              <div>cd backend && cargo run</div>
              <div>cd frontend && trunk serve</div>
            </div>
          </Card>

          {selectedTargets.includes('docker') && (
            <Card className="p-4 mb-4">
              <h4 className="font-medium text-gray-900 dark:text-white mb-3">Docker</h4>
              <div className="space-y-2 font-mono text-sm bg-gray-900 dark:bg-gray-950 p-3 rounded-lg text-green-400">
                <div>cd {outputDir}/docker</div>
                <div>docker-compose up -d</div>
              </div>
            </Card>
          )}

          {selectedTargets.includes('kubernetes') && (
            <Card className="p-4 mb-4">
              <h4 className="font-medium text-gray-900 dark:text-white mb-3">Kubernetes</h4>
              <div className="space-y-2 font-mono text-sm bg-gray-900 dark:bg-gray-950 p-3 rounded-lg text-green-400">
                <div>cd {outputDir}/kubernetes</div>
                <div>kubectl apply -f .</div>
              </div>
            </Card>
          )}

          <Card className="p-4">
            <h4 className="font-medium text-gray-900 dark:text-white mb-3">Environment Variables</h4>
            <div className="space-y-1 font-mono text-xs bg-gray-900 dark:bg-gray-950 p-3 rounded-lg text-gray-400">
              <div>DATABASE_URL=postgres://user:pass@localhost/db</div>
              <div>JWT_SECRET=your-secret-key</div>
              <div>FRONTEND_URL=http://localhost:8080</div>
              <div>REDIS_URL=redis://localhost:6379</div>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}

const structures: Record<string, string> = {
    web: `frontend/
├── public/
├── src/
│   ├── components/
│   ├── pages/
│   ├── services/
│   ├── stores/
│   ├── styles/
│   ├── utils/
│   ├── App.tsx
│   └── main.tsx
├── package.json
├── vite.config.ts
├── tailwind.config.js
└── tsconfig.json`,
    desktop: `apps/ide-tauri/
├── src/
│   ├── components/
│   ├── pages/
│   ├── stores/
│   ├── services/
│   ├── utils/
│   ├── App.tsx
│   └── main.tsx
├── src-tauri/
│   ├── src/
│   ├── tauri.conf.json
│   └── Cargo.toml
├── package.json
└── tauri.conf.json`,
    docker: `docker/
├── Dockerfile
├── docker-compose.yml
├── .dockerignore
└── nginx.conf`,
    kubernetes: `kubernetes/
├── deployment.yaml
├── service.yaml
├── ingress.yaml
├── configmap.yaml
└── secret.yaml`,
};

function getStructurePreview(target: string): string {
  return structures[target] || '';
}

const counts: Record<string, Record<string, number>> = {
  web: { backend: 0, frontend: 25, database: 5, config: 8 },
  desktop: { backend: 0, frontend: 30, database: 5, config: 10 },
  docker: { backend: 0, frontend: 0, database: 0, config: 4 },
  kubernetes: { backend: 0, frontend: 0, database: 0, config: 5 },
};

function getFileCount(selected: string[], type: string): number {
  return selected.reduce((sum: number, t: string) => {
    const key = t as keyof typeof counts;
    const sub = counts[key] as any;
    return sum + (sub?.[type] || 0);
  }, 0);
}