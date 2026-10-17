import { useState } from 'react';
import { Plus, Trash2, Settings, Puzzle, ExternalLink, Download, Upload, Filter } from 'lucide-react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';
import { Select } from '../components/ui/Select';

interface PluginInfo {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  enabled?: boolean;
  category: string;
  homepage?: string;
}

const installedPlugins = [
  { id: '1', name: 'Auth Plugin', version: '1.2.0', author: 'NoCode Team', description: 'Authentication and authorization', enabled: true, category: 'Auth', homepage: 'https://github.com/nocode/auth-plugin' },
  { id: '2', name: 'Email Plugin', version: '2.1.0', author: 'Community', description: 'Send emails via multiple providers', enabled: true, category: 'Communication', homepage: 'https://github.com/nocode/email-plugin' },
  { id: '3', name: 'Storage Plugin', version: '1.0.0', author: 'NoCode Team', description: 'File storage with multiple backends', enabled: true, category: 'Storage', homepage: 'https://github.com/nocode/storage-plugin' },
  { id: '4', name: 'Analytics Plugin', version: '3.0.0', author: 'Analytics Co', description: 'Track events and metrics', enabled: false, category: 'Analytics', homepage: 'https://github.com/analytics/plugin' },
  { id: '5', name: 'Payment Plugin', version: '1.5.0', author: 'PayCorp', description: 'Process payments via Stripe', enabled: false, category: 'Payments', homepage: 'https://github.com/paycorp/plugin' },
];

const marketplacePlugins = [
  { id: 'm1', name: 'PDF Generator', version: '1.0.0', author: 'DocuSoft', description: 'Generate PDFs from templates', category: 'Documents', downloads: 12500, rating: 4.8, price: 'Free' },
  { id: 'm2', name: 'Chart Builder', version: '2.0.0', author: 'VizCorp', description: 'Advanced charting components', category: 'Visualization', downloads: 8900, rating: 4.6, price: '$29/mo' },
  { id: 'm3', name: 'Workflow Scheduler', version: '1.1.0', author: 'TaskFlow', description: 'Cron-like scheduling for workflows', category: 'Automation', downloads: 5600, rating: 4.9, price: 'Free' },
  { id: 'm4', name: 'Multi-tenant', version: '1.0.0', author: 'SaaSKit', description: 'Multi-tenancy support', category: 'Architecture', downloads: 3200, rating: 4.5, price: '$99/mo' },
];

export function PluginManager() {
  const [activeTab, setActiveTab] = useState<'installed' | 'marketplace' | 'develop'>('installed');
  const [searchTerm, setSearchTerm] = useState('');
  const [selectedPlugin, setSelectedPlugin] = useState<PluginInfo | null>(null);

  const filteredInstalled = installedPlugins.filter(p => 
    p.name.toLowerCase().includes(searchTerm.toLowerCase()) ||
    p.category.toLowerCase().includes(searchTerm.toLowerCase())
  );

  const filteredMarketplace = marketplacePlugins.filter(p => 
    p.name.toLowerCase().includes(searchTerm.toLowerCase()) ||
    p.category.toLowerCase().includes(searchTerm.toLowerCase())
  );

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <div className="flex items-center gap-4">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Plugin Manager</h2>
          <Input value={searchTerm} onChange={(e) => setSearchTerm(e.target.value)} placeholder="Search plugins..." className="w-64" />
        </div>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm"><Upload className="w-4 h-4 mr-1" /> Upload Plugin</Button>
          <Button variant="primary" size="sm"><Plus className="w-4 h-4 mr-1" /> Create Plugin</Button>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Left Panel - Tabs */}
        <div className="w-56 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Categories</h3>
          <div className="space-y-1">
            {['installed', 'marketplace', 'develop'].map((tab) => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab as any)}
                className={`w-full text-left p-3 rounded-lg transition-colors ${
                  activeTab === tab
                    ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300'
                    : 'hover:bg-gray-100 dark:hover:bg-slate-700/50 text-gray-700 dark:text-gray-300'
                }`}
              >
                <div className="flex items-center gap-2">
                  {tab === 'installed' && <Puzzle className="w-5 h-5" />}
                  {tab === 'marketplace' && <Download className="w-5 h-5" />}
                  {tab === 'develop' && <Settings className="w-5 h-5" />}
                  <span className="capitalize">{tab}</span>
                </div>
              </button>
            ))}
          </div>
        </div>

        {/* Center Panel - Plugin List */}
        <div className="flex-1 p-6 overflow-y-auto bg-gray-100 dark:bg-slate-900">
          {activeTab === 'installed' && (
            <div className="space-y-4">
              <div className="flex items-center justify-between mb-6">
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Installed Plugins</h2>
                <div className="flex items-center gap-2">
                  <Input value={searchTerm} onChange={(e) => setSearchTerm(e.target.value)} placeholder="Search..." className="w-48" />
                  <Button variant="ghost" size="sm"><Filter className="w-4 h-4" /></Button>
                </div>
              </div>
              <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
                {filteredInstalled.map((plugin) => (
                  <Card key={plugin.id} className="p-4 hover:shadow-md transition-shadow">
                    <div className="flex items-start justify-between mb-3">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 bg-primary-100 dark:bg-primary-900/30 rounded-lg flex items-center justify-center">
                          <Puzzle className="w-5 h-5 text-primary-600 dark:text-primary-400" />
                        </div>
                        <div>
                          <h3 className="font-semibold text-gray-900 dark:text-white">{plugin.name}</h3>
                          <p className="text-sm text-gray-500 dark:text-gray-400">v{plugin.version} by {plugin.author}</p>
                        </div>
                      </div>
                      <span className={`px-2 py-1 text-xs font-medium rounded-full ${plugin.enabled ? 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400' : 'bg-gray-100 text-gray-800 dark:bg-gray-900/30 dark:text-gray-400'}`}>
                        {plugin.enabled ? 'Enabled' : 'Disabled'}
                      </span>
                    </div>
                    <p className="text-sm text-gray-600 dark:text-gray-400 mb-3">{plugin.description}</p>
                    <div className="flex items-center justify-between pt-3 border-t border-gray-200 dark:border-slate-700">
                      <span className="text-xs text-gray-500 dark:text-gray-400">{plugin.category}</span>
                      <div className="flex items-center gap-2">
                        <button className={`px-2 py-1 text-xs rounded ${plugin.enabled ? 'bg-red-100 text-red-700 hover:bg-red-200' : 'bg-green-100 text-green-700 hover:bg-green-200'}`}>
                          {plugin.enabled ? 'Disable' : 'Enable'}
                        </button>
                        <Button variant="ghost" size="sm" onClick={() => setSelectedPlugin(plugin)}><Settings className="w-4 h-4" /></Button>
                        <Button variant="ghost" size="sm"><Trash2 className="w-4 h-4" /></Button>
                      </div>
                    </div>
                  </Card>
                ))}
              </div>
            </div>
          )}

          {activeTab === 'marketplace' && (
            <div className="space-y-4">
              <div className="flex items-center justify-between mb-6">
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Marketplace</h2>
                <div className="flex items-center gap-2">
                  <Input value={searchTerm} onChange={(e) => setSearchTerm(e.target.value)} placeholder="Search marketplace..." className="w-48" />
                  <Select value="all" options={[{value: 'all', label: 'All Categories'}, {value: 'free', label: 'Free'}, {value: 'paid', label: 'Paid'}]} className="w-36" />
                </div>
              </div>
              <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
                {filteredMarketplace.map((plugin) => (
                  <Card key={plugin.id} className="p-4 hover:shadow-md transition-shadow h-full">
                    <div className="flex items-start justify-between mb-3">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 bg-green-100 dark:bg-green-900/30 rounded-lg flex items-center justify-center">
                          <Puzzle className="w-5 h-5 text-green-600 dark:text-green-400" />
                        </div>
                        <div>
                          <h3 className="font-semibold text-gray-900 dark:text-white">{plugin.name}</h3>
                          <p className="text-sm text-gray-500 dark:text-gray-400">v{plugin.version} by {plugin.author}</p>
                        </div>
                      </div>
                      <span className="text-xs font-medium px-2 py-1 rounded-full bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400">{plugin.price}</span>
                    </div>
                    <p className="text-sm text-gray-600 dark:text-gray-400 mb-3">{plugin.description}</p>
                    <div className="flex items-center gap-3 text-xs text-gray-500 dark:text-gray-400 mb-3">
                      <span><ExternalLink className="w-3 h-3 mr-1" /> {plugin.downloads.toLocaleString()} downloads</span>
                      <span>⭐ {plugin.rating}</span>
                    </div>
                    <div className="pt-3 border-t border-gray-200 dark:border-slate-700">
                      <Button variant="primary" className="w-full">Install</Button>
                    </div>
                  </Card>
                ))}
              </div>
            </div>
          )}

          {activeTab === 'develop' && (
            <div className="space-y-6">
              <div className="flex items-center justify-between">
                <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Plugin Development</h2>
                <Button variant="primary"><Plus className="w-4 h-4 mr-1" /> Create New Plugin</Button>
              </div>
              <Card className="p-6">
                <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Create a New Plugin</h3>
                <p className="text-gray-600 dark:text-gray-400 mb-6">Plugins extend the No-Code IDE with custom components, actions, data sources, and more.</p>
                <div className="grid gap-4 md:grid-cols-2">
                  <div className="space-y-4">
                    <h4 className="font-medium text-gray-900 dark:text-white">Plugin Types</h4>
                    <ul className="space-y-2 text-sm text-gray-600 dark:text-gray-400">
                      <li>• Component - Custom UI components</li>
                      <li>• Action - Custom workflow actions</li>
                      <li>• DataSource - Custom data connectors</li>
                      <li>• FieldType - Custom form field types</li>
                      <li>• WorkflowNode - Custom workflow nodes</li>
                      <li>• AuthProvider - Custom authentication</li>
                      <li>• Hook - Lifecycle hooks</li>
                    </ul>
                  </div>
                  <div className="space-y-4">
                    <h4 className="font-medium text-gray-900 dark:text-white">Getting Started</h4>
                    <ol className="space-y-2 text-sm text-gray-600 dark:text-gray-400 list-decimal list-inside">
                      <li>Run <code className="bg-gray-100 dark:bg-gray-800 px-1 rounded">npm create nocode-plugin</code></li>
                      <li>Choose plugin type and template</li>
                      <li>Develop with TypeScript/React</li>
                      <li>Test in the IDE</li>
                      <li>Publish to marketplace</li>
                    </ol>
                  </div>
                </div>
              </Card>
            </div>
          )}
        </div>

        {/* Right Panel - Plugin Details */}
        <div className="w-80 border-l border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          {selectedPlugin ? (
            <div className="space-y-4">
              <h3 className="font-semibold text-gray-900 dark:text-white">Plugin Details</h3>
              <Card className="p-4">
                <div className="flex items-center gap-3 mb-4">
                  <div className="w-12 h-12 bg-primary-100 dark:bg-primary-900/30 rounded-lg flex items-center justify-center">
                    <Puzzle className="w-6 h-6 text-primary-600 dark:text-primary-400" />
                  </div>
                  <div>
                    <h4 className="font-semibold text-gray-900 dark:text-white">{selectedPlugin.name}</h4>
                    <p className="text-sm text-gray-500 dark:text-gray-400">v{selectedPlugin.version}</p>
                  </div>
                </div>
                <p className="text-sm text-gray-600 dark:text-gray-400 mb-4">{selectedPlugin.description}</p>
                <div className="space-y-2 text-sm">
                  <div className="flex justify-between"><span className="text-gray-500">Author:</span><span className="font-medium">{selectedPlugin.author}</span></div>
                  <div className="flex justify-between"><span className="text-gray-500">Version:</span><span className="font-medium">{selectedPlugin.version}</span></div>
                  <div className="flex justify-between"><span className="text-gray-500">Category:</span><span className="font-medium">{selectedPlugin.category}</span></div>
                  <div className="flex justify-between"><span className="text-gray-500">Status:</span><span className={`font-medium ${activeTab === 'installed' && selectedPlugin.enabled ? 'text-green-600' : 'text-gray-600'}`}>{activeTab === 'installed' ? (selectedPlugin.enabled ? 'Enabled' : 'Disabled') : 'Not Installed'}</span></div>
                </div>
                <div className="flex gap-2 mt-4">
                  {activeTab === 'installed' && (
                    <>
                      <Button variant={selectedPlugin.enabled ? 'secondary' : 'primary'} className="w-full" onClick={() => {}}>
                        {selectedPlugin.enabled ? 'Disable' : 'Enable'}
                      </Button>
                      <Button variant="ghost" className="w-full"><Settings className="w-4 h-4 mr-1" /> Configure</Button>
                    </>
                  )}
                  {activeTab === 'marketplace' && (
                    <Button variant="primary" className="w-full">Install</Button>
                  )}
                </div>
              </Card>
            </div>
          ) : (
            <div className="text-center py-12 text-gray-500 dark:text-gray-400">
              <Puzzle className="w-12 h-12 mx-auto mb-4 text-gray-300 dark:text-gray-600" />
              <p>Select a plugin to view details</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}