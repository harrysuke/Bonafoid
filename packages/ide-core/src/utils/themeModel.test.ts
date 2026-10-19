/**
 * Tests for the theme token model.
 *
 * The token tree is written into project.json and read back by codegen, so a
 * scale that changes shape between sessions is a visible regression in the
 * generated app.
 */

import { describe, expect, it } from 'vitest';
import {
  COLOR_ROLES,
  COLOR_SHADES,
  buildColorScale,
  defaultDesignTokens,
  fontFamilyOf,
  fontSizeOf,
  hasDesignTokens,
  mixColor,
  newTheme,
  normalizeTheme,
  setColorBase,
  setFontFamily,
  setFontSize,
  setScaleShade,
  shadeOf,
} from './themeModel';

describe('mixColor', () => {
  it('moves toward white and black', () => {
    expect(mixColor('#000000', 1)).toBe('#ffffff');
    expect(mixColor('#ffffff', -1)).toBe('#000000');
    expect(mixColor('#808080', 0)).toBe('#808080');
  });

  it('expands shorthand hex', () => {
    expect(mixColor('#fff', 1)).toBe('#ffffff');
    expect(mixColor('#000', -1)).toBe('#000000');
  });

  it('returns non-hex input unchanged rather than corrupting it', () => {
    expect(mixColor('not-a-color', 0.5)).toBe('not-a-color');
    expect(mixColor('var(--x)', -0.2)).toBe('var(--x)');
  });

  it('clamps out-of-range channels', () => {
    expect(mixColor('#ffffff', -1)).toBe('#000000');
  });
});

describe('buildColorScale', () => {
  it('produces every shade', () => {
    const scale = buildColorScale('#0ea5e9');
    for (const shade of COLOR_SHADES) {
      expect(scale[String(shade)]).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  it('puts the base color at 500', () => {
    expect(buildColorScale('#0ea5e9')['500']).toBe('#0ea5e9');
  });

  it('gets monotonically darker as the shade number rises', () => {
    const scale = buildColorScale('#0ea5e9');
    const luminance = (hex: string) => {
      const r = parseInt(hex.slice(1, 3), 16);
      const g = parseInt(hex.slice(3, 5), 16);
      const b = parseInt(hex.slice(5, 7), 16);
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    // COLOR_SHADES ascends 50 -> 900, so luminance must descend.
    for (let i = 1; i < COLOR_SHADES.length; i += 1) {
      const previous = luminance(scale[String(COLOR_SHADES[i - 1])]);
      const current = luminance(scale[String(COLOR_SHADES[i])]);
      expect(current, `shade ${COLOR_SHADES[i]} vs ${COLOR_SHADES[i - 1]}`).toBeLessThan(previous);
    }
  });

  it('does not saturate the light or dark end to pure white/black', () => {
    const scale = buildColorScale('#0ea5e9');
    expect(scale['50']).not.toBe('#ffffff');
    expect(scale['900']).not.toBe('#000000');
  });

  it('is deterministic', () => {
    expect(buildColorScale('#22c55e')).toEqual(buildColorScale('#22c55e'));
  });
});

describe('defaultDesignTokens', () => {
  it('builds every section the Rust model requires', () => {
    const tokens = defaultDesignTokens();
    for (const section of [
      'colors',
      'typography',
      'spacing',
      'sizing',
      'borders',
      'shadows',
      'transitions',
      'z_indices',
      'breakpoints',
    ] as const) {
      expect(tokens[section], section).toBeTruthy();
    }
  });

  it('gives every color role a base and a full scale', () => {
    const tokens = defaultDesignTokens();
    for (const role of COLOR_ROLES) {
      expect(tokens.colors[role].base, role).toMatch(/^#[0-9a-f]{6}$/);
      for (const shade of COLOR_SHADES) {
        expect(tokens.colors[role].scale[String(shade)], `${role} ${shade}`).toBeTruthy();
      }
    }
  });

  it('returns a fresh object each call', () => {
    // Store updates mutate tokens; sharing one default would leak edits across
    // themes.
    const a = defaultDesignTokens();
    const b = defaultDesignTokens();
    a.colors.primary.base = '#000000';
    expect(b.colors.primary.base).not.toBe('#000000');
  });
});

describe('newTheme', () => {
  it('starts from the default tokens', () => {
    const theme = newTheme('Brand');
    expect(hasDesignTokens(theme)).toBe(true);
    expect(theme.name).toBe('Brand');
  });

  it('falls back to a name for blank input', () => {
    expect(newTheme('   ').name).toBe('Untitled Theme');
  });

  it('gives each theme its own id and token tree', () => {
    const first = newTheme('A');
    const second = newTheme('B');
    expect(first.id).not.toBe(second.id);
    first.tokens.colors.primary.base = '#000000';
    expect(second.tokens.colors.primary.base).not.toBe('#000000');
  });
});

describe('setColorBase', () => {
  it('regenerates the scale from the new base', () => {
    const tokens = setColorBase(defaultDesignTokens(), 'primary', '#ff00aa');
    expect(tokens.colors.primary.base).toBe('#ff00aa');
    expect(tokens.colors.primary.scale['500']).toBe('#ff00aa');
    expect(tokens.colors.primary.scale['100']).not.toBe(tokens.colors.primary.scale['900']);
  });

  it('keeps the brand roles in step with primary', () => {
    // A rebrand must not leave buttons in the old colour.
    const tokens = setColorBase(defaultDesignTokens(), 'primary', '#ff00aa');
    expect(tokens.colors.action.primary.background).toBe('#ff00aa');
    expect(tokens.colors.text.link).toBe('#ff00aa');
    expect(tokens.colors.border.focus).toBe('#ff00aa');
  });

  it('leaves other roles alone', () => {
    const before = defaultDesignTokens();
    const after = setColorBase(before, 'primary', '#ff00aa');
    expect(after.colors.error.base).toBe(before.colors.error.base);
    expect(after.colors.secondary.base).toBe(before.colors.secondary.base);
  });

  it('does not mutate the tokens it was given', () => {
    const before = defaultDesignTokens();
    const snapshot = JSON.parse(JSON.stringify(before)) as typeof before;
    setColorBase(before, 'primary', '#ff00aa');
    expect(before).toEqual(snapshot);
  });
});

describe('setScaleShade', () => {
  it('changes one shade without moving the base', () => {
    const tokens = setScaleShade(defaultDesignTokens(), 'secondary', 300, '#123456');
    expect(tokens.colors.secondary.scale['300']).toBe('#123456');
    expect(tokens.colors.secondary.base).not.toBe('#123456');
  });
});

describe('typography', () => {
  it('reads back what was written', () => {
    let tokens = defaultDesignTokens();
    tokens = setFontFamily(tokens, 'mono', 'Fira Code');
    tokens = setFontSize(tokens, 'lg', '1.5rem');
    expect(fontFamilyOf(tokens, 'mono')).toBe('Fira Code');
    expect(fontSizeOf(tokens, 'lg')).toBe('1.5rem');
  });

  it('leaves the other slots alone', () => {
    const before = defaultDesignTokens();
    const after = setFontFamily(before, 'sans', 'Inter New');
    expect(fontFamilyOf(after, 'serif')).toBe(fontFamilyOf(before, 'serif'));
  });
});

describe('shadeOf', () => {
  it('returns the stored shade', () => {
    const tokens = defaultDesignTokens();
    expect(shadeOf(tokens, 'primary', 500)).toBe(tokens.colors.primary.scale['500']);
  });

  it('falls back to the base for a missing shade', () => {
    const tokens = defaultDesignTokens();
    expect(shadeOf(tokens, 'primary', 999)).toBe(tokens.colors.primary.base);
  });
});

describe('normalizeTheme', () => {
  it('leaves a complete theme untouched', () => {
    const theme = newTheme('Brand');
    expect(normalizeTheme(theme)).toBe(theme);
  });

  it('repairs a theme saved with no token tree', () => {
    // Older saves stored a partial theme; codegen needs every section.
    const legacy = { id: 'x', name: 'Legacy', tokens: {} } as never;
    expect(hasDesignTokens(normalizeTheme(legacy))).toBe(true);
  });
});