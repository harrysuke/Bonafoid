/**
 * Tests for the page builder model.
 *
 * The component palette is the contract between what a user can build and what
 * codegen emits. The bug this suite guards against: an unsupported type
 * silently becoming a Container, which produced pages with a dangling trigger
 * id instead of an error.
 */

import { describe, expect, it } from 'vitest';
import {
  PALETTE,
  PALETTE_CATEGORIES,
  buildComponentType,
  canAddComponent,
  newPage,
  newPageComponent,
  pagePathFromName,
} from './pageModel';
import type { ComponentType } from '../types/model.generated';

/** Component types the palette deliberately excludes. */
const OVERLAY_TYPES = ['Modal', 'Drawer', 'Tooltip', 'Popover', 'Dropdown'] as const;

describe('palette', () => {
  it('gives every entry a known category', () => {
    for (const item of PALETTE) {
      expect(PALETTE_CATEGORIES, item.type).toContain(item.category);
    }
  });

  it('has no duplicate component types', () => {
    const types = PALETTE.map((item) => item.type);
    expect(new Set(types).size).toBe(types.length);
  });

  it('does not offer overlays or repeating templates', () => {
    for (const type of OVERLAY_TYPES) {
      expect(canAddComponent(type), type).toBe(false);
    }
    expect(canAddComponent('List')).toBe(false);
  });

  it('offers QRCode, which has a builder case', () => {
    expect(canAddComponent('QRCode')).toBe(true);
    expect(buildComponentType('QRCode').type).toBe('QRCode');
  });
});

describe('buildComponentType', () => {
  it('builds every palette entry', () => {
    for (const item of PALETTE) {
      const built = buildComponentType(item.type);
      expect(built.type, item.type).toBe(item.type);
    }
  });

  it('throws for an overlay rather than substituting a Container', () => {
    for (const type of OVERLAY_TYPES) {
      expect(() => buildComponentType(type as ComponentType['type']), type).toThrow(
        /cannot be added from the page builder/,
      );
    }
  });

  it('throws for List, which the list builder owns', () => {
    expect(() => buildComponentType('List')).toThrow(/List builder/);
  });

  it('gives container-ish components an empty child list', () => {
    const container = buildComponentType('Container');
    expect(container.type).toBe('Container');
    expect(container.config).toEqual({ children: [] });
  });
});

describe('newPageComponent', () => {
  it('produces an empty component shell', () => {
    const component = newPageComponent('Card');
    expect(component.component.type).toBe('Card');
    expect(component.props).toEqual({});
    expect(component.bindings).toEqual([]);
    expect(component.events).toEqual([]);
    expect(component.id).toBeTruthy();
  });

  it('gives each component its own id', () => {
    expect(newPageComponent('Card').id).not.toBe(newPageComponent('Card').id);
  });
});

describe('newPage', () => {
  it('starts from a named page with a derived path', () => {
    const page = newPage('Customer Portal', 'customer-portal');
    expect(page.name).toBe('Customer Portal');
    expect(page.path).toBe('customer-portal');
    expect(page.components).toEqual([]);
  });

  it('stamps created and updated timestamps', () => {
    const page = newPage('A', 'a');
    expect(Date.parse(page.created_at)).not.toBeNaN();
    expect(Date.parse(page.updated_at)).not.toBeNaN();
  });
});

describe('pagePathFromName', () => {
  it('produces a route path with a leading slash', () => {
    expect(pagePathFromName('Customer Portal')).toBe('/customer-portal');
    expect(pagePathFromName('  Reports & Analytics ')).toContain('reports');
  });

  it('falls back for a name with no usable characters', () => {
    expect(pagePathFromName('***')).toBeTruthy();
  });
});