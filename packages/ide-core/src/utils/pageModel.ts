/**
 * Page builder model helpers.
 *
 * Everything in this module is expressed with the generated Rust types from
 * `types/model.generated` so the IDE and the backend agree on the page
 * contract. Editor-side stores (`stores/projectStore.ts`) keep the permissive
 * `Project['pages']` shape and cast at that single boundary.
 */

import type {
  ActionConfig,
  ActionType,
  BindingSource,
  BindingTarget,
  ComponentType,
  DataBinding,
  Page,
  PageAction,
  PageComponent,
  PageLayout,
  PagePermissions,
  RenderCondition,
} from '../types/model.generated';
import { generateId } from './helpers';

export type PaletteCategory =
  | 'layout'
  | 'data'
  | 'basic'
  | 'input'
  | 'feedback'
  | 'navigation';

export const PALETTE_CATEGORIES: PaletteCategory[] = [
  'layout',
  'data',
  'basic',
  'input',
  'feedback',
  'navigation',
];

export interface PaletteItem {
  type: ComponentType['type'];
  category: PaletteCategory;
  label: string;
  /** Components that need a data source / form / action wired up in the inspector. */
  needsBinding?: boolean;
}

export const PALETTE: PaletteItem[] = [
  // Layout
  { type: 'Container', category: 'layout', label: 'Container' },
  { type: 'Grid', category: 'layout', label: 'Grid' },
  { type: 'Flex', category: 'layout', label: 'Flex' },
  { type: 'Card', category: 'layout', label: 'Card' },
  { type: 'Tabs', category: 'layout', label: 'Tabs' },
  { type: 'Accordion', category: 'layout', label: 'Accordion' },
  { type: 'Divider', category: 'layout', label: 'Divider' },
  // Data
  { type: 'Table', category: 'data', label: 'Table', needsBinding: true },
  { type: 'DetailView', category: 'data', label: 'Detail view', needsBinding: true },
  { type: 'Chart', category: 'data', label: 'Chart', needsBinding: true },
  { type: 'StatCard', category: 'data', label: 'Stat card' },
  { type: 'ProgressBar', category: 'data', label: 'Progress bar' },
  { type: 'Tree', category: 'data', label: 'Tree', needsBinding: true },
  { type: 'Transfer', category: 'data', label: 'Transfer', needsBinding: true },
  { type: 'Cascader', category: 'data', label: 'Cascader', needsBinding: true },
  { type: 'TreeSelect', category: 'data', label: 'Tree select', needsBinding: true },
  // Basic
  { type: 'Html', category: 'basic', label: 'Text / HTML' },
  { type: 'Image', category: 'basic', label: 'Image' },
  { type: 'Avatar', category: 'basic', label: 'Avatar' },
  { type: 'Badge', category: 'basic', label: 'Badge' },
  { type: 'Tag', category: 'basic', label: 'Tag' },
  { type: 'Video', category: 'basic', label: 'Video' },
  { type: 'Audio', category: 'basic', label: 'Audio' },
  { type: 'Iframe', category: 'basic', label: 'Iframe' },
  { type: 'Custom', category: 'basic', label: 'Custom' },
  // Input
  { type: 'Button', category: 'input', label: 'Button', needsBinding: true },
  { type: 'ButtonGroup', category: 'input', label: 'Button group' },
  { type: 'Form', category: 'input', label: 'Form', needsBinding: true },
  { type: 'FormField', category: 'input', label: 'Form field' },
  { type: 'DatePicker', category: 'input', label: 'Date picker' },
  { type: 'TimePicker', category: 'input', label: 'Time picker' },
  { type: 'ColorPicker', category: 'input', label: 'Color picker' },
  { type: 'FileUpload', category: 'input', label: 'File upload', needsBinding: true },
  { type: 'Rate', category: 'input', label: 'Rate' },
  { type: 'Slider', category: 'input', label: 'Slider' },
  { type: 'Mention', category: 'input', label: 'Mention' },
  // Feedback
  { type: 'Alert', category: 'feedback', label: 'Alert' },
  { type: 'Message', category: 'feedback', label: 'Message' },
  { type: 'Notification', category: 'feedback', label: 'Notification' },
  { type: 'Loading', category: 'feedback', label: 'Loading' },
  { type: 'Skeleton', category: 'feedback', label: 'Skeleton' },
  { type: 'Spin', category: 'feedback', label: 'Spin' },
  { type: 'QRCode', category: 'feedback', label: 'QR code' },
  // Navigation
  { type: 'Link', category: 'navigation', label: 'Link' },
  { type: 'Breadcrumb', category: 'navigation', label: 'Breadcrumb' },
  { type: 'Steps', category: 'navigation', label: 'Steps' },
  { type: 'Menu', category: 'navigation', label: 'Menu' },
  { type: 'Anchor', category: 'navigation', label: 'Anchor' },
  { type: 'BackTop', category: 'navigation', label: 'Back to top' },
  { type: 'Pagination', category: 'navigation', label: 'Pagination' },
];

const EMPTY_SELECTION = {
  enabled: false,
  type: 'checkbox' as const,
  fixed: false,
  column_title: null,
  column_width: null,
};

const EMPTY_PAGINATION = {
  enabled: true,
  page_size: 20,
  show_size_changer: true,
  show_quick_jumper: false,
  show_total: true,
};

const EMPTY_CASCADE = { children: '', label: '', value: '' };

/**
 * Build the `ComponentType` payload for a palette entry. Components that
 * reference another component (Modal/Drawer/Tooltip/Popover/Dropdown) or a
 * repeating template are intentionally absent from the palette: the List
 * builder owns the item template and overlays need a live trigger.
 */
export function buildComponentType(type: ComponentType['type']): ComponentType {
  switch (type) {
    case 'Container':
      return { type, config: { children: [] } };
    case 'Grid':
      return { type, config: { children: [], columns: 12, gap: '16px' } };
    case 'Flex':
      return { type, config: { children: [], direction: 'row', gap: '16px' } };
    case 'Tabs':
      return { type, config: { children: [], tabs: [] } };
    case 'Accordion':
      return { type, config: { children: [], items: [] } };
    case 'Card':
      return { type, config: { children: [], header: null, footer: null } };
    case 'Table':
      return {
        type,
        config: {
          columns: [],
          data_source: '',
          filtering: false,
          pagination: { ...EMPTY_PAGINATION },
          selection: { ...EMPTY_SELECTION },
          sorting: true,
        },
      };
    case 'DetailView':
      return { type, config: { data_source: '', fields: [] } };
    case 'Chart':
      return {
        type,
        config: {
          chart_type: 'bar',
          config: { animation: true, responsive: true, series: [] },
          data_source: '',
        },
      };
    case 'StatCard':
      return { type, config: { label: 'Stat', value: '0', icon: null, trend: null } };
    case 'ProgressBar':
      return { type, config: { value: '0', max: '100', format: 'percent', color: null } };
    case 'Avatar':
      return { type, config: { src: '', alt: '', shape: 'circle', size: 'medium' } };
    case 'Badge':
      return { type, config: { content: 'Badge', color: 'blue', variant: 'solid' } };
    case 'Tag':
      return { type, config: { content: 'Tag', color: 'blue', closable: false } };
    case 'Divider':
      return { type, config: { orientation: 'horizontal', dashed: false, text: null } };
    case 'Form':
      return { type, config: { form_id: '', submit_action: '', reset_action: null } };
    case 'FormField':
      return { type, config: { field_id: '', label: 'Field', required: false } };
    case 'Button':
      return {
        type,
        config: {
          label: 'Button',
          action: '',
          variant: 'primary',
          size: 'middle',
          disabled: false,
          loading: false,
          icon: null,
        },
      };
    case 'ButtonGroup':
      return { type, config: { variant: 'primary', buttons: [] } };
    case 'Link':
      return { type, config: { label: 'Link', href: '/', target: '_self', icon: null } };
    case 'Breadcrumb':
      return { type, config: { items: [] } };
    case 'Pagination':
      return {
        type,
        config: {
          page_size: 20,
          show_size_changer: true,
          show_quick_jumper: false,
          total: '0',
        },
      };
    case 'Steps':
      return { type, config: { current: 0, direction: 'horizontal', items: [] } };
    case 'Tree':
      return {
        type,
        config: {
          data_source: '',
          checkable: false,
          draggable: false,
          field_names: { children: 'children', key: 'id', title: 'name' },
        },
      };
    case 'Alert':
      return { type, config: { type: 'info', message: '', closable: false, show_icon: true } };
    case 'Message':
      return { type, config: { type: 'info', content: '', duration: 3 } };
    case 'Notification':
      return { type, config: { type: 'info', title: '', content: '', duration: 3 } };
    case 'Loading':
      return { type, config: { spinning: true, delay: 0, tip: null } };
    case 'Skeleton':
      return {
        type,
        config: { active: true, avatar: false, title: true, paragraph: true, rows: 3 },
      };
    case 'Spin':
      return { type, config: { spinning: true, size: 'large', tip: null } };
    case 'Menu':
      return { type, config: { items: [], mode: 'inline', theme: 'light' } };
    case 'Anchor':
      return { type, config: { links: [], affix: true } };
    case 'BackTop':
      return { type, config: { visibility_height: 200 } };
    case 'Html':
      return { type, config: { content: '<p>Text</p>', sanitize: true } };
    case 'Iframe':
      return { type, config: { src: '', width: '100%', height: '400px', sandbox: [] } };
    case 'Image':
      return {
        type,
        config: { src: '', alt: '', preview: false, width: null, height: null, fallback: null },
      };
    case 'Video':
      return {
        type,
        config: { src: '', poster: '', controls: true, autoplay: false, loop_play: false },
      };
    case 'Audio':
      return { type, config: { src: '', controls: true, autoplay: false, loop_play: false } };
    case 'FileUpload':
      return {
        type,
        config: {
          accept: '',
          action: '',
          directory: false,
          drag: true,
          list_type: 'text',
          multiple: false,
        },
      };
    case 'ColorPicker':
      return { type, config: { format: 'hex', alpha: false, preset_colors: [] } };
    case 'DatePicker':
      return {
        type,
        config: { format: 'YYYY-MM-DD', show_time: false, range: false, disabled_date: null },
      };
    case 'TimePicker':
      return {
        type,
        config: {
          format: 'HH:mm',
          use12_hours: false,
          step: { hour: 1, minute: 1, second: 1 },
        },
      };
    case 'Rate':
      return { type, config: { count: 5, allow_half: false, allow_clear: true, character: '*' } };
    case 'Slider':
      return {
        type,
        config: {
          min: 0,
          max: 100,
          step: 1,
          range: false,
          marks: {},
          tooltip: true,
        },
      };
    case 'Transfer':
      return {
        type,
        config: { data_source: '', titles: ['Options', 'Selected'], operations: [], filterable: true },
      };
    case 'Cascader':
      return {
        type,
        config: {
          data_source: '',
          expand_trigger: 'click',
          change_on_select: false,
          field_names: { ...EMPTY_CASCADE },
        },
      };
    case 'TreeSelect':
      return {
        type,
        config: {
          data_source: '',
          show_search: true,
          tree_default_expand_all: true,
          field_names: {
            children: 'children',
            key: 'id',
            title: 'name',
          },
        },
      };
    case 'Mention':
      return { type, config: { prefix: '@', split: ' ', suggestions: [] } };
    case 'QRCode':
      return { type, config: { value: '', size: 160, level: 'M', icon: null } };
    case 'Custom':
      return { type, config: { component_name: '', props: {} } };
    default:
      // Overlays (Modal/Drawer/Tooltip/Popover/Dropdown) and repeating
      // templates (List) are deliberately not palette-droppable: they need a
      // live trigger component or a record template the builder does not have
      // yet. Returning a silent Container here would let a hand-edited config
      // produce a page with a dangling trigger id, so this throws instead.
      throw new Error(
        `${String(type)} cannot be added from the page builder: it needs a live ` +
          'trigger component or a record template. Use the List builder for List.',
      );
  }
}

export function newPageComponent(type: ComponentType['type']): PageComponent {
  return {
    id: generateId(),
    component: buildComponentType(type),
    props: {},
    slots: {},
    bindings: [],
    conditions: [],
    events: [],
    style_overrides: {},
  };
}

/** True when the page builder can create this component type. */
export function canAddComponent(type: ComponentType['type']): boolean {
  return PALETTE.some((item) => item.type === type);
}

export function newPageComponentWithId(type: ComponentType['type'], id: string): PageComponent {
  return { ...newPageComponent(type), id };
}

export function emptyLayout(): PageLayout {
  return { type: 'fluid', breakpoints: {}, containers: [], global_styles: {} };
}

export function emptyPermissions(): PagePermissions {
  return { public_access: true, view_roles: [], edit_roles: [], admin_roles: [] };
}

export function newPage(name: string, path: string): Page {
  const now = new Date().toISOString();
  return {
    id: generateId(),
    name,
    path,
    title: name,
    description: null,
    layout: emptyLayout(),
    permissions: emptyPermissions(),
    seo: { keywords: [] },
    components: [],
    data_sources: [],
    actions: [],
    created_at: now,
    updated_at: now,
  };
}

export function newPageAction(
  name: string,
  actionType: ActionType,
  config: ActionConfig,
): PageAction {
  return { id: generateId(), name, action_type: actionType, config, permissions: [] };
}

export function newDataBinding(
  source: BindingSource,
  target: BindingTarget,
  twoWay = false,
  transform: string | null = null,
): DataBinding {
  return { source, target, two_way: twoWay, transform };
}

export function newDataSourceBinding(sourceId: string, path: string): BindingSource {
  return { DataSource: { source_id: sourceId, path } };
}

export function newPropTarget(prop: string): BindingTarget {
  return { Prop: prop };
}

export function componentLabel(component: PageComponent): string {
  return `${component.component.type}`;
}

/** Editable page metadata (everything else is structural). */
export interface PageMetaPatch {
  name?: string;
  path?: string;
  title?: string;
  description?: string | null;
}

/** Patch applied to one page component by the inspector. */
export interface PageComponentPatch {
  component?: ComponentType;
  props?: Record<string, unknown>;
  style_overrides?: Record<string, string>;
  bindings?: DataBinding[];
  conditions?: RenderCondition[];
}

export function pagePathFromName(name: string): string {
  const slug = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  return `/${slug || 'page'}`;
}