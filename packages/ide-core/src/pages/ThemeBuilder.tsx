/**
 * Theme builder (M7 #5).
 *
 * Reads and writes themes through the project store, so edits autosave with the
 * rest of the project and reach codegen via the active theme. All values come
 * from the generated `DesignTokens` model (see utils/themeModel.ts).
 */

import { useMemo, useState } from 'react';
import { Check, Copy, Palette, Plus, Trash2 } from 'lucide-react';
import type { Theme } from '../types/model.generated';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { Input } from '../components/ui/Input';
import { useProjectStore } from '../stores/projectStore';
import {
  COLOR_ROLES,
  COLOR_SHADES,
  FONT_ROLES,
  FONT_SIZE_ROLES,
  fontFamilyOf,
  fontSizeOf,
  normalizeTheme,
  shadeOf,
} from '../utils/themeModel';
import type { ColorRole } from '../utils/themeModel';

type Tab = 'colors' | 'typography' | 'spacing' | 'borders' | 'shadows';

const TABS: { id: Tab; label: string }[] = [
  { id: 'colors', label: 'Colors' },
  { id: 'typography', label: 'Typography' },
  { id: 'spacing', label: 'Spacing' },
  { id: 'borders', label: 'Borders' },
  { id: 'shadows', label: 'Shadows' },
];

export function ThemeBuilder() {
  const project = useProjectStore((s) => s.project);
  const addTheme = useProjectStore((s) => s.addTheme);
  const removeTheme = useProjectStore((s) => s.removeTheme);
  const duplicateTheme = useProjectStore((s) => s.duplicateTheme);
  const updateThemeMeta = useProjectStore((s) => s.updateThemeMeta);
  const setActiveTheme = useProjectStore((s) => s.setActiveTheme);
  const setThemeColor = useProjectStore((s) => s.setThemeColor);
  const setThemeFontFamily = useProjectStore((s) => s.setThemeFontFamily);
  const setThemeFontSize = useProjectStore((s) => s.setThemeFontSize);

  const themes = useMemo(
    () =>
      Object.values(project?.themes ?? {})
        .map((theme) => normalizeTheme(theme as unknown as Theme))
        .sort((a, b) => a.name.localeCompare(b.name)),
    [project?.themes],
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('colors');

  const selected = themes.find((theme) => theme.id === selectedId) ?? themes[0] ?? null;
  const tokens = selected?.tokens;

  if (!project) {
    return (
      <div className="h-full flex items-center justify-center text-gray-500 dark:text-gray-400">
        Open a project to edit its themes.
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center justify-between p-4 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800">
        <div className="flex items-center gap-3 min-w-0">
          <Palette className="w-5 h-5 text-primary-600" />
          <h2 className="text-xl font-semibold text-gray-900 dark:text-white truncate">Theme Builder</h2>
          {selected && (
            <Input
              value={selected.name}
              onChange={(e) => updateThemeMeta(selected.id, { name: e.target.value })}
              className="w-56"
              aria-label="Theme name"
            />
          )}
        </div>
        <div className="flex items-center gap-2">
          {selected && project.active_theme_id !== selected.id && (
            <Button variant="secondary" size="sm" onClick={() => setActiveTheme(selected.id)}>
              <Check className="w-4 h-4 mr-1" /> Set active
            </Button>
          )}
          {selected && (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setSelectedId(duplicateTheme(selected.id))}
            >
              <Copy className="w-4 h-4 mr-1" /> Duplicate
            </Button>
          )}
          <Button
            variant="danger"
            size="sm"
            disabled={!selected || themes.length === 0}
            onClick={() => {
              if (!selected) return;
              removeTheme(selected.id);
              setSelectedId(null);
            }}
          >
            <Trash2 className="w-4 h-4 mr-1" /> Delete
          </Button>
          <Button variant="primary" size="sm" onClick={() => setSelectedId(addTheme('Theme'))}>
            <Plus className="w-4 h-4 mr-1" /> New theme
          </Button>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
        <div className="w-64 border-r border-gray-200 dark:border-slate-700 bg-gray-50 dark:bg-slate-800 p-4 overflow-y-auto">
          <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Themes</h3>
          {themes.length === 0 && (
            <p className="text-sm text-gray-500 dark:text-gray-400">
              No themes yet. Codegen uses the active theme&apos;s tokens.
            </p>
          )}
          <ul className="space-y-1">
            {themes.map((theme) => {
              const active = project.active_theme_id === theme.id;
              return (
                <li key={theme.id}>
                  <button
                    onClick={() => setSelectedId(theme.id)}
                    className={`w-full text-left px-3 py-2 rounded-lg transition-colors ${
                      selected?.id === theme.id
                        ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300'
                        : 'hover:bg-gray-100 dark:hover:bg-slate-700/50 text-gray-700 dark:text-gray-300'
                    }`}
                  >
                    <span className="flex items-center justify-between gap-2">
                      <span className="truncate text-sm">{theme.name}</span>
                      {active && <Check className="w-4 h-4 shrink-0" aria-label="Active theme" />}
                    </span>
                    <span className="mt-1 flex gap-1">
                      {COLOR_ROLES.slice(0, 5).map((role) => (
                        <span
                          key={role}
                          className="h-2 w-4 rounded"
                          style={{ backgroundColor: shadeOf(theme.tokens, role, 500) }}
                        />
                      ))}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        </div>

        <div className="flex-1 flex overflow-hidden">
          <div className="w-48 border-r border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 p-4">
            <ul className="space-y-1">
              {TABS.map((entry) => (
                <li key={entry.id}>
                  <button
                    onClick={() => setTab(entry.id)}
                    aria-current={tab === entry.id}
                    className={`w-full text-left px-3 py-2 rounded-lg text-sm transition-colors ${
                      tab === entry.id
                        ? 'bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300'
                        : 'hover:bg-gray-100 dark:hover:bg-slate-700/50 text-gray-700 dark:text-gray-300'
                    }`}
                  >
                    {entry.label}
                  </button>
                </li>
              ))}
            </ul>
          </div>

          <div className="flex-1 p-6 overflow-y-auto bg-gray-100 dark:bg-slate-900">
            {!tokens || !selected ? (
              <p className="text-gray-500 dark:text-gray-400">Create a theme to start editing tokens.</p>
            ) : (
              <div className="max-w-4xl mx-auto space-y-4">
                {tab === 'colors' &&
                  COLOR_ROLES.map((role) => (
                    <Card key={role} className="p-4">
                      <div className="flex items-center justify-between mb-3">
                        <h3 className="font-semibold text-gray-900 dark:text-white capitalize">{role}</h3>
                        <div className="flex items-center gap-2">
                          <span className="text-xs font-mono text-gray-500 dark:text-gray-400">
                            {tokens.colors[role].base}
                          </span>
                          <input
                            type="color"
                            aria-label={`${role} base color`}
                            value={normalizeHex(tokens.colors[role].base)}
                            onChange={(e) => setThemeColor(selected.id, role as ColorRole, e.target.value)}
                            className="w-9 h-9 rounded border-0 cursor-pointer"
                          />
                        </div>
                      </div>
                      <div className="grid grid-cols-10 gap-2">
                        {COLOR_SHADES.map((shade) => (
                          <div
                            key={shade}
                            title={`${role} ${shade}: ${shadeOf(tokens, role, shade)}`}
                            className="aspect-square rounded-lg border border-gray-200 dark:border-slate-700"
                            style={{ backgroundColor: shadeOf(tokens, role, shade) }}
                          >
                            <div className="flex h-full items-end justify-center pb-1 text-[10px] text-white/90">
                              {shade}
                            </div>
                          </div>
                        ))}
                      </div>
                    </Card>
                  ))}

                {tab === 'typography' && (
                  <>
                    <Card className="p-4">
                      <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Font families</h3>
                      <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
                        {FONT_ROLES.map((role) => (
                          <label key={role} className="text-sm text-gray-700 dark:text-gray-300">
                            <span className="capitalize">{role}</span>
                            <Input
                              value={fontFamilyOf(tokens, role)}
                              onChange={(e) => setThemeFontFamily(selected.id, role, e.target.value)}
                            />
                          </label>
                        ))}
                      </div>
                    </Card>
                    <Card className="p-4">
                      <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Font sizes</h3>
                      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                        {FONT_SIZE_ROLES.map((role) => (
                          <label key={role} className="text-sm text-gray-700 dark:text-gray-300">
                            <span>{role}</span>
                            <Input
                              value={fontSizeOf(tokens, role)}
                              onChange={(e) => setThemeFontSize(selected.id, role, e.target.value)}
                            />
                          </label>
                        ))}
                      </div>
                    </Card>
                  </>
                )}

                {tab === 'spacing' && (
                  <Card className="p-4">
                    <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Spacing scale</h3>
                    <div className="space-y-2">
                      {Object.entries(tokens.spacing.scale).map(([key, value]) => (
                        <div key={key} className="flex items-center gap-3">
                          <span className="w-10 text-xs font-mono text-gray-500 dark:text-gray-400">{key}</span>
                          <div
                            className="h-3 rounded bg-primary-500"
                            style={{ width: value, maxWidth: '100%' }}
                          />
                          <span className="text-xs font-mono text-gray-500 dark:text-gray-400">{value}</span>
                        </div>
                      ))}
                    </div>
                  </Card>
                )}

                {tab === 'borders' && (
                  <Card className="p-4">
                    <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Radii</h3>
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                      {Object.entries(tokens.borders.radii).map(([key, value]) => (
                        <div key={key} className="text-center">
                          <div
                            className="h-14 w-full border-2 border-primary-500"
                            style={{ borderRadius: value }}
                          />
                          <span className="mt-1 block text-xs font-mono text-gray-500 dark:text-gray-400">
                            {key}: {value}
                          </span>
                        </div>
                      ))}
                    </div>
                  </Card>
                )}

                {tab === 'shadows' && (
                  <Card className="p-4">
                    <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Shadows</h3>
                    <div className="space-y-3">
                      {Object.entries(tokens.shadows)
                        .filter(([key]) => key !== 'elevated')
                        .map(([key, value]) => (
                          <div key={key} className="flex items-center gap-3">
                            <div className="h-10 w-24 rounded bg-white dark:bg-slate-700" style={{ boxShadow: value }} />
                            <span className="text-xs font-mono text-gray-500 dark:text-gray-400">
                              {key}: {value}
                            </span>
                          </div>
                        ))}
                    </div>
                  </Card>
                )}
              </div>
            )}
          </div>

          <div className="w-80 border-l border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 p-4 overflow-y-auto">
            <h3 className="font-semibold text-gray-900 dark:text-white mb-3">Live preview</h3>
            {!tokens || !selected ? (
              <p className="text-sm text-gray-500 dark:text-gray-400">No theme selected.</p>
            ) : (
              <div
                className="space-y-4 p-4 rounded-lg"
                style={{
                  backgroundColor: tokens.colors.background.primary,
                  color: tokens.colors.text.primary,
                  fontFamily: fontFamilyOf(tokens, 'sans'),
                  fontSize: fontSizeOf(tokens, 'base'),
                  borderRadius: tokens.borders.radii.lg,
                  borderColor: tokens.colors.border.primary,
                }}
              >
                <div className="space-y-2">
                  <h4 className="font-semibold">Buttons</h4>
                  <div className="flex flex-wrap gap-2">
                    <span
                      className="px-3 py-1.5 rounded font-medium"
                      style={{
                        backgroundColor: tokens.colors.action.primary.background,
                        color: tokens.colors.action.primary.text,
                        borderRadius: tokens.borders.radii.md,
                      }}
                    >
                      Primary
                    </span>
                    <span
                      className="px-3 py-1.5 rounded font-medium"
                      style={{
                        backgroundColor: tokens.colors.action.secondary.background,
                        color: tokens.colors.action.secondary.text,
                        borderRadius: tokens.borders.radii.md,
                      }}
                    >
                      Secondary
                    </span>
                    <span
                      className="px-3 py-1.5 rounded font-medium"
                      style={{
                        backgroundColor: tokens.colors.action.danger.background,
                        color: tokens.colors.action.danger.text,
                        borderRadius: tokens.borders.radii.md,
                      }}
                    >
                      Danger
                    </span>
                  </div>
                </div>

                <div className="space-y-2">
                  <h4 className="font-semibold">Typography</h4>
                  <p
                    className="font-bold"
                    style={{
                      fontSize: fontSizeOf(tokens, 'display_sm'),
                      color: tokens.colors.text.primary,
                    }}
                  >
                    Display heading
                  </p>
                  <p style={{ color: tokens.colors.text.secondary }}>Body text uses the configured sans family.</p>
                  <p
                    className="font-mono"
                    style={{ fontSize: fontSizeOf(tokens, 'sm'), color: tokens.colors.text.tertiary }}
                  >
                    mono 0.875rem
                  </p>
                </div>

                <div
                  className="p-3"
                  style={{
                    backgroundColor: tokens.colors.background.secondary,
                    border: `1px solid ${tokens.colors.border.primary}`,
                    borderRadius: tokens.borders.radii.md,
                    boxShadow: tokens.shadows.sm,
                  }}
                >
                  <p className="font-semibold">Card</p>
                  <p style={{ color: tokens.colors.text.secondary, fontSize: fontSizeOf(tokens, 'sm') }}>
                    Uses the theme&apos;s background, border and shadow tokens.
                  </p>
                </div>

                <div className="space-y-2">
                  <h4 className="font-semibold">Palette</h4>
                  <div className="grid grid-cols-7 gap-1">
                    {COLOR_ROLES.map((role) => (
                      <div key={role} className="text-center">
                        <div
                          className="h-8 rounded"
                          style={{ backgroundColor: shadeOf(tokens, role, 500) }}
                          title={`${role}: ${shadeOf(tokens, role, 500)}`}
                        />
                        <span className="text-[10px] text-gray-500 dark:text-gray-400">{role.slice(0, 4)}</span>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

/** `<input type="color">` only accepts `#rrggbb`. */
function normalizeHex(value: string): string {
  return /^#[0-9a-f]{6}$/i.test(value) ? value : '#000000';
}