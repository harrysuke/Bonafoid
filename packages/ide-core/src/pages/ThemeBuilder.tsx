import { useState } from 'react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { Input } from '../components/ui/Input';

const colorScales = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900];

export function ThemeBuilder() {
  const [activeTab, setActiveTab] = useState<'colors' | 'typography' | 'spacing' | 'components'>('colors');
  const [theme, setTheme] = useState({
    name: 'Default Theme',
    colors: {
      primary: { base: '#0ea5e9', scale: {} as Record<number, string> },
      secondary: { base: '#64748b', scale: {} as Record<number, string> },
      success: { base: '#22c55e', scale: {} as Record<number, string> },
      warning: { base: '#f59e0b', scale: {} as Record<number, string> },
      error: { base: '#ef4444', scale: {} as Record<number, string> },
    },
    typography: {
      fontFamilies: { sans: 'Inter', serif: 'Georgia', mono: 'JetBrains Mono' },
      fontSizes: { xs: '0.75rem', sm: '0.875rem', base: '1rem', lg: '1.125rem', xl: '1.25rem', '2xl': '1.5rem', '3xl': '1.875rem', '4xl': '2.25rem' },
    },
    spacing: { base: '0.25rem', scale: {} },
    borderRadius: { none: '0', sm: '0.125rem', md: '0.375rem', lg: '0.5rem', xl: '0.75rem', '2xl': '1rem', full: '9999px' },
    shadows: { sm: '0 1px 2px 0 rgb(0 0 0 / 0.05)', md: '0 4px 6px -1px rgb(0 0 0 / 0.1)', lg: '0 10px 15px -3px rgb(0 0 0 / 0.1)' },
  });

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <div className="flex items-center gap-4">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white">Theme Builder</h2>
          <Input value={theme.name} onChange={(e) => setTheme({...theme, name: e.target.value})} placeholder="Theme name" className="w-64" />
        </div>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm">Import</Button>
          <Button variant="secondary" size="sm">Export</Button>
          <Button variant="primary" size="sm">Save Theme</Button>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Left Panel - Tabs */}
        <div className="w-56 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Categories</h3>
          <div className="space-y-1">
            {['colors', 'typography', 'spacing', 'borders', 'shadows', 'components'].map((tab) => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab as any)}
                className={`w-full text-left p-3 rounded-lg transition-colors ${
                  activeTab === tab
                    ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300'
                    : 'hover:bg-gray-100 dark:hover:bg-slate-700/50 text-gray-700 dark:text-gray-300'
                }`}
              >
                {tab.charAt(0).toUpperCase() + tab.slice(1)}
              </button>
            ))}
          </div>
        </div>

        {/* Center Panel - Editor */}
        <div className="flex-1 p-6 overflow-y-auto bg-gray-100 dark:bg-slate-900">
          <div className="max-w-4xl mx-auto space-y-6">
            <div className="flex items-center justify-between">
              <h2 className="text-2xl font-bold text-gray-900 dark:text-white">{activeTab.charAt(0).toUpperCase() + activeTab.slice(1)}</h2>
            </div>

            {activeTab === 'colors' && (
              <div className="space-y-6">
                {Object.entries(theme.colors).map(([colorName, color]) => (
                  <Card key={colorName} className="p-4">
                    <div className="flex items-center justify-between mb-4">
                      <h3 className="font-semibold text-gray-900 dark:text-white capitalize">{colorName}</h3>
                      <input type="color" value={color.base} onChange={(e) => setTheme({...theme, colors: {...theme.colors, [colorName]: {...color, base: e.target.value}}})} className="w-10 h-10 rounded border-0 cursor-pointer" />
                    </div>
                    <div className="grid grid-cols-10 gap-2">
                      {colorScales.map((shade) => (
                        <div key={shade} className="relative aspect-square rounded-lg border border-gray-200 dark:border-slate-700" style={{ backgroundColor: color.scale[shade] || color.base }}>
                          <div className="absolute bottom-1 left-1 right-1 text-xs text-white/90 text-center">{shade}</div>
                        </div>
                      ))}
                    </div>
                  </Card>
                ))}
              </div>
            )}

            {activeTab === 'typography' && (
              <div className="space-y-6">
                <Card className="p-4">
                  <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Font Families</h3>
                  <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                    {Object.entries(theme.typography.fontFamilies).map(([key, value]) => (
                      <div key={key} className="space-y-2">
                        <label className="text-sm font-medium text-gray-700 dark:text-gray-300 capitalize">{key}</label>
                        <Input value={value} onChange={(e) => setTheme({...theme, typography: {...theme.typography, fontFamilies: {...theme.typography.fontFamilies, [key]: e.target.value}}})} placeholder={key} />
                      </div>
                    ))}
                  </div>
                </Card>

                <Card className="p-4">
                  <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Font Sizes</h3>
                  <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
                    {Object.entries(theme.typography.fontSizes).map(([key, value]) => (
                      <div key={key} className="space-y-2">
                        <label className="text-sm font-medium text-gray-700 dark:text-gray-300">{key}</label>
                        <Input value={value} onChange={(e) => setTheme({...theme, typography: {...theme.typography, fontSizes: {...theme.typography.fontSizes, [key]: e.target.value}}})} placeholder={key} />
                      </div>
                    ))}
                  </div>
                </Card>
              </div>
            )}

            {['spacing', 'borders', 'shadows'].includes(activeTab) && (
              <div className="space-y-6">
                <Card className="p-4">
                  <h3 className="font-semibold text-gray-900 dark:text-white mb-4">{activeTab.charAt(0).toUpperCase() + activeTab.slice(1)}</h3>
                  <p className="text-gray-500 dark:text-gray-400">Configuration for {activeTab} coming soon...</p>
                </Card>
              </div>
            )}
          </div>
        </div>

        {/* Right Panel - Preview */}
        <div className="w-80 border-l border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-4">Live Preview</h3>
          <Card className="p-4">
            <div className="space-y-4">
              <div className="space-y-2">
                <h4 className="font-semibold text-gray-900 dark:text-white">Buttons</h4>
                <div className="flex flex-wrap gap-2">
                  <button className="px-4 py-2 bg-primary-600 text-white rounded-lg">Primary</button>
                  <button className="px-4 py-2 bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white rounded-lg">Secondary</button>
                  <button className="px-4 py-2 bg-green-600 text-white rounded-lg">Success</button>
                  <button className="px-4 py-2 bg-red-600 text-white rounded-lg">Danger</button>
                </div>
              </div>
              <div className="space-y-2">
                <h4 className="font-semibold text-gray-900 dark:text-white">Form Inputs</h4>
                <input type="text" placeholder="Text input" className="w-full px-3 py-2 border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800" />
                <select className="w-full px-3 py-2 border border-gray-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800">
                  <option>Select option</option>
                  <option>Option 1</option>
                  <option>Option 2</option>
                </select>
              </div>
              <div className="space-y-2">
                <h4 className="font-semibold text-gray-900 dark:text-white">Typography</h4>
                <h1 className="text-4xl font-bold text-gray-900 dark:text-white">Heading 1</h1>
                <h2 className="text-3xl font-bold text-gray-900 dark:text-white">Heading 2</h2>
                <p className="text-gray-700 dark:text-gray-300">Body text with the configured font family and size.</p>
                <p className="text-sm text-gray-500 dark:text-gray-400">Small text with the configured font family and size.</p>
              </div>
              <div className="space-y-2">
                <h4 className="font-semibold text-gray-900 dark:text-white">Cards</h4>
                <div className="p-4 border border-gray-200 dark:border-slate-700 rounded-lg bg-white dark:bg-slate-800">
                  <h4 className="font-semibold text-gray-900 dark:text-white mb-1">Card Title</h4>
                  <p className="text-gray-700 dark:text-gray-300 text-sm">Card content goes here with the theme's colors and spacing.</p>
                </div>
              </div>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}