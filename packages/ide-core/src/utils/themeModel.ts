/**
 * Theme builder model (M7 #5).
 *
 * Maps the editor's theme list onto the generated `Theme` model from
 * `types/model.generated.ts` (ts-rust-bindings output for
 * `crates/model/src/theme.rs`), so a saved theme round-trips through the
 * server and reaches codegen with the same token shape.
 *
 * A `DesignTokens` tree has no partial state: every field is required, so the
 * builder always edits a complete token set produced by `defaultDesignTokens`
 * and derives color scales from the edited base color.
 */

import type {
  ActionColorSet,
  ColorScale,
  ColorTokens,
  DesignTokens,
  FontFamilies,
  FontSizes,
  Theme,
  TypographyTokens,
} from '../types/model.generated';
import { generateId } from './helpers';

/** Color roles a user can edit, in the order the builder shows them. */
export const COLOR_ROLES = [
  'primary',
  'secondary',
  'success',
  'warning',
  'error',
  'info',
  'neutral',
] as const;

export type ColorRole = (typeof COLOR_ROLES)[number];

/** Shade keys generated for every color scale. */
export const COLOR_SHADES = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900] as const;

/** Font slots exposed by the typography editor. */
export const FONT_ROLES = ['sans', 'serif', 'mono'] as const;

export type FontRole = (typeof FONT_ROLES)[number];

/** Font size slots exposed by the typography editor. */
export const FONT_SIZE_ROLES = [
  'xs',
  'sm',
  'base',
  'lg',
  'xl',
  'xxl',
  'xxxl',
  'display_xs',
  'display_sm',
  'display_base',
  'display_lg',
  'display_xl',
  'display_xxl',
] as const;

export type FontSizeRole = (typeof FONT_SIZE_ROLES)[number];

const DEFAULT_FONT_FAMILIES: Record<FontRole, string> = {
  sans: 'Inter',
  serif: 'Georgia',
  mono: 'JetBrains Mono',
};

const DEFAULT_FONT_SIZES: Record<FontSizeRole, string> = {
  xs: '0.75rem',
  sm: '0.875rem',
  base: '1rem',
  lg: '1.125rem',
  xl: '1.25rem',
  xxl: '1.5rem',
  xxxl: '2rem',
  display_xs: '2rem',
  display_sm: '2.5rem',
  display_base: '3rem',
  display_lg: '3.75rem',
  display_xl: '4.5rem',
  display_xxl: '6rem',
};

const BASE_COLORS: Record<ColorRole, string> = {
  primary: '#0ea5e9',
  secondary: '#64748b',
  success: '#22c55e',
  warning: '#f59e0b',
  error: '#ef4444',
  info: '#0ea5e9',
  neutral: '#64748b',
};

function hexToRgb(hex: string): [number, number, number] | null {
  const value = hex.trim().replace('#', '');
  const full =
    value.length === 3
      ? value
          .split('')
          .map((char) => char + char)
          .join('')
      : value;
  if (full.length !== 6 || !/^[0-9a-f]{6}$/i.test(full)) return null;
  return [
    parseInt(full.slice(0, 2), 16),
    parseInt(full.slice(2, 4), 16),
    parseInt(full.slice(4, 6), 16),
  ];
}

function toHex(channel: number): string {
  const clamped = Math.max(0, Math.min(255, Math.round(channel)));
  return clamped.toString(16).padStart(2, '0');
}

/**
 * Mix a color toward white (positive ratio) or black (negative ratio).
 * Returns the input unchanged when it is not a hex color.
 */
export function mixColor(hex: string, ratio: number): string {
  const rgb = hexToRgb(hex);
  if (!rgb) return hex;
  const target = ratio >= 0 ? 255 : 0;
  const weight = Math.abs(ratio);
  return `#${rgb.map((channel) => toHex(channel + (target - channel) * weight)).join('')}`;
}

/**
 * Derive a 50..900 scale from one base color: lighter shades above the base,
 * darker ones below. Deterministic, so a saved theme keeps its exact values.
 */
export function buildColorScale(base: string): Record<string, string> {
  const scale: Record<string, string> = {};
  // Spread the mix across the whole range so no step saturates. Mixing by
  // "steps away from 500" pushed 50/100/200 past white, which gave three
  // identical shades at the light end of the ramp.
  const lightest = COLOR_SHADES[0];
  const darkest = COLOR_SHADES[COLOR_SHADES.length - 1];
  for (const shade of COLOR_SHADES) {
    if (shade === 500) {
      scale[String(shade)] = base;
    } else if (shade < 500) {
      const towardWhite = ((500 - shade) / (500 - lightest)) * 0.9;
      scale[String(shade)] = mixColor(base, towardWhite);
    } else {
      const towardBlack = ((shade - 500) / (darkest - 500)) * 0.85;
      scale[String(shade)] = mixColor(base, -towardBlack);
    }
  }
  return scale;
}

function colorScale(base: string): ColorScale {
  return { base, scale: buildColorScale(base), contrast_text: null };
}

/** Action color set derived from a scale base. */
function actionColors(base: string): ActionColorSet {
  return {
    background: base,
    background_hover: mixColor(base, 0.12),
    background_active: mixColor(base, -0.12),
    border: mixColor(base, -0.2),
    text: '#ffffff',
    disabled_background: mixColor(base, 0.6),
    disabled_border: mixColor(base, 0.5),
    disabled_text: mixColor(base, 0.55),
  };
}

function defaultColors(): ColorTokens {
  return {
    primary: colorScale(BASE_COLORS.primary),
    secondary: colorScale(BASE_COLORS.secondary),
    success: colorScale(BASE_COLORS.success),
    warning: colorScale(BASE_COLORS.warning),
    error: colorScale(BASE_COLORS.error),
    info: colorScale(BASE_COLORS.info),
    neutral: colorScale(BASE_COLORS.neutral),
    action: {
      primary: actionColors(BASE_COLORS.primary),
      secondary: actionColors(BASE_COLORS.secondary),
      tertiary: actionColors(BASE_COLORS.secondary),
      danger: actionColors(BASE_COLORS.error),
      ghost: actionColors('#94a3b8'),
    },
    background: {
      primary: '#ffffff',
      secondary: '#f8fafc',
      tertiary: '#f1f5f9',
      inverse: '#0f172a',
      hover: '#f1f5f9',
      active: '#e2e8f0',
      disabled: '#f8fafc',
      overlay: 'rgba(15, 23, 42, 0.45)',
      modal: '#ffffff',
      popover: '#ffffff',
      tooltip: '#0f172a',
    },
    text: {
      primary: '#0f172a',
      secondary: '#334155',
      tertiary: '#64748b',
      disabled: '#94a3b8',
      inverse: '#f8fafc',
      placeholder: '#94a3b8',
      link: BASE_COLORS.primary,
      link_hover: mixColor(BASE_COLORS.primary, -0.15),
      on_primary: '#ffffff',
      on_secondary: '#ffffff',
      on_success: '#ffffff',
      on_warning: '#0f172a',
      on_error: '#ffffff',
      on_info: '#ffffff',
    },
    border: {
      primary: '#e2e8f0',
      secondary: '#cbd5e1',
      tertiary: '#94a3b8',
      divider: '#e2e8f0',
      focus: BASE_COLORS.primary,
      success: BASE_COLORS.success,
      warning: BASE_COLORS.warning,
      error: BASE_COLORS.error,
    },
    custom: {},
  };
}

function fontFamily(roles: Partial<Record<FontRole, string>> = {}): FontFamilies {
  return {
    sans: roles.sans ?? DEFAULT_FONT_FAMILIES.sans,
    serif: roles.serif ?? DEFAULT_FONT_FAMILIES.serif,
    mono: roles.mono ?? DEFAULT_FONT_FAMILIES.mono,
    display: null,
    custom: {},
  };
}

function defaultTypography(): TypographyTokens {
  return {
    font_families: fontFamily(),
    font_sizes: {
      ...DEFAULT_FONT_SIZES,
      xxxl: '2rem',
      display_xs: '2rem',
      display_sm: '2.5rem',
      display_base: '3rem',
      display_lg: '3.75rem',
      display_xl: '4.5rem',
      display_xxl: '6rem',
    },
    font_weights: {
      thin: 100,
      extra_light: 200,
      light: 300,
      normal: 400,
      medium: 500,
      semi_bold: 600,
      bold: 700,
      extra_bold: 800,
      black: 900,
    },
    line_heights: {
      none: '1',
      tight: '1.25',
      snug: '1.375',
      normal: '1.5',
      relaxed: '1.625',
      loose: '2',
    },
    letter_spacings: {
      tighter: '-0.05em',
      tight: '-0.025em',
      normal: '0',
      wide: '0.025em',
      wider: '0.05em',
      widest: '0.1em',
    },
    text_styles: {},
  };
}

/** The token set a brand-new theme starts from. */
export function defaultDesignTokens(): DesignTokens {
  return {
    colors: defaultColors(),
    typography: defaultTypography(),
    spacing: {
      base: '0.25rem',
      scale: {
        '0': '0',
        '1': '0.25rem',
        '2': '0.5rem',
        '3': '0.75rem',
        '4': '1rem',
        '5': '1.25rem',
        '6': '1.5rem',
        '8': '2rem',
        '10': '2.5rem',
        '12': '3rem',
        '16': '4rem',
      },
      component: { xs: '0.25rem', sm: '0.5rem', md: '0.75rem', lg: '1rem', xl: '1.5rem', xxl: '2rem' },
      layout: { container_padding: '1rem', section_gap: '3rem', component_gap: '1rem', inline_gap: '0.5rem' },
    },
    sizing: {
      widths: { auto: 'auto', px: '1px', full: '100%', screen: '100vw', min: 'min-content', max: 'max-content', scale: {} },
      heights: { auto: 'auto', px: '1px', full: '100%', screen: '100vh', min: 'min-content', max: 'max-content', scale: {} },
      max_widths: { none: 'none', xs: '20rem', sm: '24rem', md: '32rem', lg: '48rem', xl: '64rem', xxl: '80rem', full: '100%', screen: '100vw' },
      container: { sm: '640px', md: '768px', lg: '1024px', xl: '1280px', xxl: '1536px', full: '100%' },
    },
    borders: {
      radii: {
        none: '0',
        xs: '0.125rem',
        sm: '0.25rem',
        md: '0.375rem',
        lg: '0.5rem',
        xl: '0.75rem',
        xxl: '1rem',
        full: '9999px',
        circle: '50%',
      },
      widths: { none: '0', thin: '1px', base: '1px', thick: '2px', thicker: '4px' },
      styles: {
        solid: 'solid',
        dashed: 'dashed',
        dotted: 'dotted',
        double: 'double',
        groove: 'groove',
        ridge: 'ridge',
        inset: 'inset',
        outset: 'outset',
        none: 'none',
        hidden: 'hidden',
      },
    },
    shadows: {
      none: 'none',
      xs: '0 1px 2px 0 rgba(15, 23, 42, 0.05)',
      sm: '0 1px 3px 0 rgba(15, 23, 42, 0.1)',
      md: '0 4px 6px -1px rgba(15, 23, 42, 0.1)',
      lg: '0 10px 15px -3px rgba(15, 23, 42, 0.1)',
      xl: '0 20px 25px -5px rgba(15, 23, 42, 0.1)',
      xxl: '0 25px 50px -12px rgba(15, 23, 42, 0.25)',
      inner: 'inset 0 2px 4px 0 rgba(15, 23, 42, 0.06)',
      outline: '0 0 0 3px rgba(14, 165, 233, 0.4)',
      focus: '0 0 0 3px rgba(14, 165, 233, 0.35)',
      elevated: {
        '1': '0 1px 3px 0 rgba(15, 23, 42, 0.08)',
        '2': '0 10px 15px -3px rgba(15, 23, 42, 0.12)',
        '3': '0 25px 50px -12px rgba(15, 23, 42, 0.3)',
      },
    },
    transitions: {
      durations: { instant: '0ms', fast: '150ms', normal: '250ms', slow: '400ms', slower: '600ms' },
      easings: {
        linear: 'linear',
        ease_in: 'cubic-bezier(0.4, 0, 1, 1)',
        ease_out: 'cubic-bezier(0, 0, 0.2, 1)',
        ease_in_out: 'cubic-bezier(0.4, 0, 0.2, 1)',
        ease_out_back: 'cubic-bezier(0.34, 1.56, 0.64, 1)',
        ease_in_back: 'cubic-bezier(0.36, 0, 0.66, -0.56)',
        ease_out_circ: 'cubic-bezier(0, 0.55, 0.45, 1)',
        ease_in_circ: 'cubic-bezier(0.55, 0, 1, 0.45)',
      },
      properties: {
        all: 'all',
        background: 'background-color, background-image',
        border: 'border-color, border-width',
        box_shadow: 'box-shadow',
        colors: 'color, background-color, border-color',
        opacity: 'opacity',
        transform: 'transform',
      },
    },
    z_indices: {
      auto: 'auto',
      base: '0',
      dropdown: '10',
      sticky: '100',
      fixed: '200',
      modal_backdrop: '1000',
      modal: '1100',
      popover: '1050',
      tooltip: '1150',
      toast: '1200',
    },
    breakpoints: { xs: '0px', sm: '640px', md: '768px', lg: '1024px', xl: '1280px', xxl: '1536px' },
    custom: {},
  };
}

/** A new theme with default tokens, ready to be edited. */
export function newTheme(name: string): Theme {
  const now = new Date().toISOString();
  return {
    id: generateId(),
    name: name.trim() || 'Untitled Theme',
    description: null,
    version: '1.0.0',
    tokens: defaultDesignTokens(),
    components: {},
    global_styles: '',
    dark_mode: { enabled: true, strategy: 'class', selector: '.dark', tokens: null },
    created_at: now,
    updated_at: now,
  };
}

/** Replace a role's base color and regenerate its scale and action colors. */
export function setColorBase(tokens: DesignTokens, role: ColorRole, base: string): DesignTokens {
  const colors = tokens.colors;
  if (role === 'primary' || role === 'secondary') {
    // Action colors follow the brand roles so buttons stay consistent.
    const action = { ...colors.action };
    action[role === 'primary' ? 'primary' : 'secondary'] = actionColors(base);
    if (role === 'primary') action.tertiary = actionColors(base);
    return {
      ...tokens,
      colors: {
        ...colors,
        primary: role === 'primary' ? colorScale(base) : colors.primary,
        secondary: role === 'secondary' ? colorScale(base) : colors.secondary,
        action,
        text: role === 'primary' ? { ...colors.text, link: base } : colors.text,
        border: role === 'primary' ? { ...colors.border, focus: base } : colors.border,
      },
    };
  }
  return { ...tokens, colors: { ...colors, [role]: colorScale(base) } };
}

/** Set one shade of a role's scale without touching the base color. */
export function setScaleShade(
  tokens: DesignTokens,
  role: ColorRole,
  shade: number,
  value: string,
): DesignTokens {
  const target = tokens.colors[role];
  return {
    ...tokens,
    colors: {
      ...tokens.colors,
      [role]: { ...target, scale: { ...target.scale, [String(shade)]: value } },
    },
  };
}

/** Set a font family slot (sans/serif/mono). */
export function setFontFamily(tokens: DesignTokens, role: FontRole, value: string): DesignTokens {
  const families = tokens.typography.font_families as unknown as Record<string, string>;
  return {
    ...tokens,
    typography: {
      ...tokens.typography,
      font_families: { ...families, [role]: value } as unknown as FontFamilies,
    },
  };
}

/** Set a font size slot. */
export function setFontSize(tokens: DesignTokens, role: FontSizeRole, value: string): DesignTokens {
  const sizes = tokens.typography.font_sizes as unknown as Record<string, string>;
  return {
    ...tokens,
    typography: {
      ...tokens.typography,
      font_sizes: { ...sizes, [role]: value } as unknown as FontSizes,
    },
  };
}

/** Font family currently configured for a slot. */
export function fontFamilyOf(tokens: DesignTokens, role: FontRole): string {
  const families = tokens.typography.font_families as unknown as Record<string, string>;
  return families[role] ?? DEFAULT_FONT_FAMILIES[role];
}

/** Font size currently configured for a slot. */
export function fontSizeOf(tokens: DesignTokens, role: FontSizeRole): string {
  const sizes = tokens.typography.font_sizes as unknown as Record<string, string>;
  return sizes[role] ?? DEFAULT_FONT_SIZES[role];
}

/** Shade value for a role, falling back to its base color. */
export function shadeOf(tokens: DesignTokens, role: ColorRole, shade: number): string {
  const scale = tokens.colors[role];
  return scale.scale[String(shade)] ?? scale.base;
}

/** True when a theme is a complete DesignTokens tree. */
export function hasDesignTokens(theme: Theme | { tokens?: unknown } | null | undefined): boolean {
  const tokens = (theme as Theme | undefined)?.tokens as DesignTokens | undefined;
  return Boolean(
    tokens &&
      tokens.colors &&
      tokens.typography &&
      tokens.spacing &&
      tokens.borders &&
      tokens.shadows,
  );
}

/**
 * Fill in any missing token sections on a theme read from storage.
 * Older saves stored a partial tree; codegen needs every section.
 */
export function normalizeTheme(theme: Theme): Theme {
  if (hasDesignTokens(theme)) return theme;
  return { ...theme, tokens: defaultDesignTokens() };
}