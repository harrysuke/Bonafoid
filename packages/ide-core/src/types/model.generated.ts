// AUTO-GENERATED. DO NOT EDIT BY HAND.
// Source: crates/model (schemars) -> src/types/model.schema.json.
// Regenerate: cargo run --manifest-path xtask/Cargo.toml -- types
//             node packages/ide-core/scripts/gen-model-types.mjs
export interface AccordionItem {
  children: Array<ComponentId>;
  disabled: boolean;
  icon?: string | null;
  key: string;
  label: string;
}

export interface ActionColorSet {
  background: string;
  background_active: string;
  background_hover: string;
  border: string;
  disabled_background: string;
  disabled_border: string;
  disabled_text: string;
  text: string;
}

export interface ActionColors {
  danger: ActionColorSet;
  ghost: ActionColorSet;
  primary: ActionColorSet;
  secondary: ActionColorSet;
  tertiary: ActionColorSet;
}

export type ActionConfig = {
  config: {
    path: string;
    replace: boolean;
    state?: unknown;
  };
  type: "Navigate";
} | {
  config: {
    external: boolean;
    url: string;
  };
  type: "Redirect";
} | {
  type: "Reload";
} | {
  type: "Back";
} | {
  type: "Forward";
} | {
  config: {
    delta: number;
  };
  type: "Go";
} | {
  config: {
    modal_id: ComponentId;
    props: Record<string, unknown>;
  };
  type: "OpenModal";
} | {
  config: {
    modal_id: ComponentId;
  };
  type: "CloseModal";
} | {
  config: {
    form_id: ComponentId;
    on_error: Array<ActionId>;
    on_success: Array<ActionId>;
    validate: boolean;
  };
  type: "SubmitForm";
} | {
  config: {
    form_id: ComponentId;
  };
  type: "ResetForm";
} | {
  config: {
    form_id: ComponentId;
    values: Record<string, unknown>;
  };
  type: "SetFormValues";
} | {
  config: {
    field: string;
    form_id: ComponentId;
    value: unknown;
  };
  type: "SetFieldValue";
} | {
  config: {
    form_id: ComponentId;
  };
  type: "ValidateForm";
} | {
  config: {
    data_source_id: string;
    on_error: Array<ActionId>;
    on_success: Array<ActionId>;
    params: Record<string, string>;
  };
  type: "CallApi";
} | {
  config: {
    on_complete: Array<ActionId>;
    on_error: Array<ActionId>;
    variables: Record<string, string>;
    workflow_id: WorkflowId;
  };
  type: "ExecuteWorkflow";
} | {
  config: {
    key: string;
    scope: StateScope;
    value: unknown;
  };
  type: "SetState";
} | {
  config: {
    default?: unknown;
    key: string;
    scope: StateScope;
  };
  type: "GetState";
} | {
  config: {
    key: string;
    scope: StateScope;
  };
  type: "DeleteState";
} | {
  config: {
    scope: StateScope;
  };
  type: "ClearState";
} | {
  config: {
    event_name: string;
    payload: unknown;
    target: EventTarget;
  };
  type: "EmitEvent";
} | {
  config: {
    event_name: string;
    handler: ActionId;
  };
  type: "SubscribeEvent";
} | {
  config: {
    event_name: string;
  };
  type: "UnsubscribeEvent";
} | {
  config: {
    success_message?: string | null;
    text: string;
  };
  type: "CopyToClipboard";
} | {
  config: {
    filename: string;
    url: string;
  };
  type: "Download";
} | {
  type: "Print";
} | {
  config: {
    features: string;
    name: string;
    url: string;
  };
  type: "OpenWindow";
} | {
  type: "CloseWindow";
} | {
  config: {
    selector: string;
  };
  type: "FocusElement";
} | {
  config: {
    behavior: ScrollBehavior;
    block: ScrollAlignment;
    selector: string;
  };
  type: "ScrollTo";
} | {
  config: {
    args: Array<unknown>;
    script: string;
  };
  type: "RunScript";
} | {
  config: {
    expression: string;
    result_var: string;
  };
  type: "EvalExpression";
} | {
  config: {
    ms: number;
  };
  type: "Delay";
} | {
  config: {
    actions: Array<ActionId>;
    parallel: boolean;
  };
  type: "Batch";
} | {
  config: {
    condition: string;
    else_actions: Array<ActionId>;
    then_actions: Array<ActionId>;
  };
  type: "Conditional";
} | {
  config: {
    body: Array<ActionId>;
    collection: string;
    index_var?: string | null;
    item_var: string;
  };
  type: "Loop";
} | {
  config: {
    catch_actions: Array<ActionId>;
    finally_actions: Array<ActionId>;
    try_actions: Array<ActionId>;
  };
  type: "TryCatch";
} | {
  config: {
    level: LogLevel;
    message: string;
    metadata: Record<string, unknown>;
  };
  type: "Log";
} | {
  config: {
    code?: string | null;
    context: Record<string, unknown>;
    message: string;
  };
  type: "ReportError";
};

export type ActionId = string;

export type ActionType = "navigate" | "submit_form" | "reset_form" | "open_modal" | "close_modal" | "call_api" | "execute_workflow" | "set_state" | "emit_event" | "copy_to_clipboard" | "download" | "print" | {
  custom: string;
};

export type AlertType = "success" | "info" | "warning" | "error";

export interface AnchorLink {
  href: string;
  title: string;
}

export interface ApiConfig {
  base_path: string;
  cors: CorsConfig;
  documentation: ApiDocConfig;
  rate_limiting: RateLimitConfig;
  versioning: ApiVersioning;
  webhooks: WebhookConfig;
}

export interface ApiDocConfig {
  contact: ContactInfo;
  description: string;
  enabled: boolean;
  license: LicenseInfo;
  path: string;
  servers: Array<ServerInfo>;
  title: string;
  version: string;
}

export type ApiVersioning = "header" | "query" | "path" | "none";

export interface AttributeMapping {
  display_name: string;
  email: string;
  first_name: string;
  groups: string;
  last_name: string;
  roles: string;
  user_id: string;
}

export interface AuthConfig {
  enabled: boolean;
  mfa: MfaConfig;
  password_policy: PasswordPolicy;
  protected_routes: Array<string>;
  providers: Array<AuthProvider>;
  public_routes: Array<string>;
  session: SessionConfig;
  social: SocialAuthConfig;
  sso: SsoConfig;
}

export interface AuthProvider {
  button_icon?: string | null;
  button_text: string;
  config: Record<string, unknown>;
  enabled: boolean;
  id: string;
  name: string;
  provider_type: AuthProviderType;
}

export type AuthProviderType = "email_password" | "phone_otp" | "magic_link" | "o_auth2" | "o_i_d_c" | "s_a_m_l" | "l_d_a_p" | "custom";

export type AutoGenerate = "uuid" | "uuid_v7" | "nanoid" | "increment" | "timestamp" | {
  custom: string;
};

export interface AxisConfig {
  formatter?: string | null;
  interval?: number | null;
  max?: number | null;
  min?: number | null;
  name?: string | null;
  type?: string | null;
}

export interface AzureBlobConfig {
  account_key: string;
  account_name: string;
  container: string;
  endpoint_suffix: string;
}

export interface BackgroundColors {
  active: string;
  disabled: string;
  hover: string;
  inverse: string;
  modal: string;
  overlay: string;
  popover: string;
  primary: string;
  secondary: string;
  tertiary: string;
  tooltip: string;
}

export type BindingSource = {
  Variable: string;
} | {
  DataSource: {
    path: string;
    source_id: string;
  };
} | {
  Computed: string;
} | {
  Param: string;
} | {
  State: string;
} | {
  Prop: string;
} | {
  Const: unknown;
};

export type BindingTarget = {
  Prop: string;
} | {
  Attr: string;
} | {
  Style: string;
} | {
  Class: string;
} | {
  Event: string;
} | {
  Slot: string;
};

export interface BorderColors {
  divider: string;
  error: string;
  focus: string;
  primary: string;
  secondary: string;
  success: string;
  tertiary: string;
  warning: string;
}

export interface BorderRadii {
  circle: string;
  full: string;
  lg: string;
  md: string;
  none: string;
  sm: string;
  xl: string;
  xs: string;
  xxl: string;
}

export interface BorderStyles {
  dashed: string;
  dotted: string;
  double: string;
  groove: string;
  hidden: string;
  inset: string;
  none: string;
  outset: string;
  ridge: string;
  solid: string;
}

export interface BorderTokens {
  radii: BorderRadii;
  styles: BorderStyles;
  widths: BorderWidths;
}

export interface BorderWidths {
  base: string;
  none: string;
  thick: string;
  thicker: string;
  thin: string;
}

export interface BoundaryEvent {
  attached_to: NodeId;
  cancel_activity: boolean;
  event_type: BoundaryEventType;
  id: BoundaryEventId;
}

export type BoundaryEventId = string;

export type BoundaryEventType = {
  config: {
    time_cycle?: string | null;
    time_duration?: string | null;
  };
  type: "Timer";
} | {
  config: {
    error_ref: string;
  };
  type: "Error";
} | {
  config: {
    escalation_ref: string;
  };
  type: "Escalation";
} | {
  config: {
    signal_ref: string;
  };
  type: "Signal";
} | {
  config: {
    message_ref: string;
  };
  type: "Message";
} | {
  config: {
    activity_ref: string;
  };
  type: "Compensation";
} | {
  config: {
    condition: string;
  };
  type: "Conditional";
};

export interface BreadcrumbItem {
  href?: string | null;
  icon?: string | null;
  label: string;
  separator?: string | null;
}

export interface BreakpointTokens {
  lg: string;
  md: string;
  sm: string;
  xl: string;
  xs: string;
  xxl: string;
}

export interface Breakpoints {
  lg?: string | null;
  md?: string | null;
  sm?: string | null;
  xl?: string | null;
  xs?: string | null;
  xxl?: string | null;
}

export type Bundler = "trunk" | "vite" | "webpack" | "rollup" | "parcel";

export interface ButtonConfig {
  action: string;
  disabled: boolean;
  icon?: string | null;
  label: string;
  size: ButtonSize;
  variant: ButtonVariant;
}

export type ButtonSize = "large" | "middle" | "small";

export type ButtonVariant = "primary" | "secondary" | "success" | "warning" | "danger" | "ghost" | "link" | "text";

export interface CacheConfig {
  enabled: boolean;
  key?: string | null;
  stale_while_revalidate_ms?: number | null;
  ttl_ms: number;
}

export interface CascaderFieldNames {
  children: string;
  label: string;
  value: string;
}

export interface ChartConfig {
  animation: boolean;
  grid?: GridConfig | null;
  legend?: LegendConfig | null;
  responsive: boolean;
  series: Array<SeriesConfig>;
  theme?: string | null;
  tooltip?: TooltipConfig | null;
  x_axis?: AxisConfig | null;
  y_axis?: AxisConfig | null;
}

export type ChartType = "line" | "bar" | "pie" | "area" | "scatter" | "radar" | "funnel" | "gauge" | "heatmap" | "treemap" | "sankey" | "word_cloud" | "liquid_fill";

export interface ColorScale {
  base: string;
  contrast_text?: string | null;
  scale: Record<string, string>;
}

export interface ColorTokens {
  action: ActionColors;
  background: BackgroundColors;
  border: BorderColors;
  custom: Record<string, ColorScale>;
  error: ColorScale;
  info: ColorScale;
  neutral: ColorScale;
  primary: ColorScale;
  secondary: ColorScale;
  success: ColorScale;
  text: TextColors;
  warning: ColorScale;
}

export interface ComponentEvent {
  event: string;
  handler: EventHandler;
  modifiers: Array<EventModifier>;
}

export type ComponentId = string;

export interface ComponentSize {
  class: string;
  props: Record<string, unknown>;
}

export interface ComponentSpacing {
  lg: string;
  md: string;
  sm: string;
  xl: string;
  xs: string;
  xxl: string;
}

export interface ComponentStates {
  active?: string | null;
  disabled?: string | null;
  error?: string | null;
  focus?: string | null;
  focus_visible?: string | null;
  hover?: string | null;
  loading?: string | null;
  read_only?: string | null;
  success?: string | null;
  warning?: string | null;
}

export interface ComponentTheme {
  base: string;
  sizes: Record<string, ComponentSize>;
  slots: Record<string, string>;
  states: ComponentStates;
  variants: Record<string, ComponentVariant>;
}

export type ComponentType = {
  config: {
    children: Array<ComponentId>;
  };
  type: "Container";
} | {
  config: {
    children: Array<ComponentId>;
    columns: number;
    gap: string;
  };
  type: "Grid";
} | {
  config: {
    children: Array<ComponentId>;
    direction: string;
    gap: string;
  };
  type: "Flex";
} | {
  config: {
    children: Array<ComponentId>;
    tabs: Array<TabConfig>;
  };
  type: "Tabs";
} | {
  config: {
    children: Array<ComponentId>;
    items: Array<AccordionItem>;
  };
  type: "Accordion";
} | {
  config: {
    children: Array<ComponentId>;
    footer?: string | null;
    header?: string | null;
  };
  type: "Card";
} | {
  config: {
    children: Array<ComponentId>;
    trigger: ComponentId;
  };
  type: "Modal";
} | {
  config: {
    children: Array<ComponentId>;
    position: string;
    trigger: ComponentId;
  };
  type: "Drawer";
} | {
  config: {
    content: string;
    position: string;
    trigger: ComponentId;
  };
  type: "Tooltip";
} | {
  config: {
    content: Array<ComponentId>;
    position: string;
    trigger: ComponentId;
  };
  type: "Popover";
} | {
  config: {
    columns: Array<TableColumn>;
    data_source: string;
    filtering: boolean;
    pagination: PaginationConfig;
    selection: SelectionConfig;
    sorting: boolean;
  };
  type: "Table";
} | {
  config: {
    data_source: string;
    empty_state?: ComponentId | null;
    item_template: ComponentId;
  };
  type: "List";
} | {
  config: {
    data_source: string;
    fields: Array<DetailField>;
  };
  type: "DetailView";
} | {
  config: {
    chart_type: ChartType;
    config: ChartConfig;
    data_source: string;
  };
  type: "Chart";
} | {
  config: {
    icon?: string | null;
    label: string;
    trend?: TrendConfig | null;
    value: string;
  };
  type: "StatCard";
} | {
  config: {
    color?: string | null;
    format: string;
    max: string;
    value: string;
  };
  type: "ProgressBar";
} | {
  config: {
    alt: string;
    shape: string;
    size: string;
    src: string;
  };
  type: "Avatar";
} | {
  config: {
    color: string;
    content: string;
    variant: string;
  };
  type: "Badge";
} | {
  config: {
    closable: boolean;
    color: string;
    content: string;
  };
  type: "Tag";
} | {
  config: {
    dashed: boolean;
    orientation: string;
    text?: string | null;
  };
  type: "Divider";
} | {
  config: {
    form_id: FormId;
    reset_action?: string | null;
    submit_action: string;
  };
  type: "Form";
} | {
  config: {
    field_id: string;
    label: string;
    required: boolean;
  };
  type: "FormField";
} | {
  config: {
    action: string;
    disabled: boolean;
    icon?: string | null;
    label: string;
    loading: boolean;
    size: ButtonSize;
    variant: ButtonVariant;
  };
  type: "Button";
} | {
  config: {
    buttons: Array<ButtonConfig>;
    variant: ButtonVariant;
  };
  type: "ButtonGroup";
} | {
  config: {
    href: string;
    icon?: string | null;
    label: string;
    target: string;
  };
  type: "Link";
} | {
  config: {
    items: Array<BreadcrumbItem>;
  };
  type: "Breadcrumb";
} | {
  config: {
    page_size: number;
    show_quick_jumper: boolean;
    show_size_changer: boolean;
    total: string;
  };
  type: "Pagination";
} | {
  config: {
    current: number;
    direction: string;
    items: Array<StepItem>;
  };
  type: "Steps";
} | {
  config: {
    checkable: boolean;
    data_source: string;
    draggable: boolean;
    field_names: TreeFieldNames;
  };
  type: "Tree";
} | {
  config: {
    closable: boolean;
    message: string;
    show_icon: boolean;
    type: AlertType;
  };
  type: "Alert";
} | {
  config: {
    content: string;
    duration: number;
    type: AlertType;
  };
  type: "Message";
} | {
  config: {
    content: string;
    duration: number;
    title: string;
    type: AlertType;
  };
  type: "Notification";
} | {
  config: {
    delay: number;
    spinning: boolean;
    tip?: string | null;
  };
  type: "Loading";
} | {
  config: {
    active: boolean;
    avatar: boolean;
    paragraph: boolean;
    rows: number;
    title: boolean;
  };
  type: "Skeleton";
} | {
  config: {
    size: string;
    spinning: boolean;
    tip?: string | null;
  };
  type: "Spin";
} | {
  config: {
    items: Array<MenuItem>;
    mode: MenuMode;
    theme: string;
  };
  type: "Menu";
} | {
  config: {
    items: Array<MenuItem>;
    placement: string;
    trigger: ComponentId;
  };
  type: "Dropdown";
} | {
  config: {
    affix: boolean;
    links: Array<AnchorLink>;
  };
  type: "Anchor";
} | {
  config: {
    visibility_height: number;
  };
  type: "BackTop";
} | {
  config: {
    content: string;
    sanitize: boolean;
  };
  type: "Html";
} | {
  config: {
    height: string;
    sandbox: Array<string>;
    src: string;
    width: string;
  };
  type: "Iframe";
} | {
  config: {
    alt: string;
    fallback?: string | null;
    height?: string | null;
    preview: boolean;
    src: string;
    width?: string | null;
  };
  type: "Image";
} | {
  config: {
    autoplay: boolean;
    controls: boolean;
    loop_play: boolean;
    poster: string;
    src: string;
  };
  type: "Video";
} | {
  config: {
    autoplay: boolean;
    controls: boolean;
    loop_play: boolean;
    src: string;
  };
  type: "Audio";
} | {
  config: {
    accept: string;
    action: string;
    directory: boolean;
    drag: boolean;
    list_type: string;
    multiple: boolean;
  };
  type: "FileUpload";
} | {
  config: {
    alpha: boolean;
    format: string;
    preset_colors: Array<string>;
  };
  type: "ColorPicker";
} | {
  config: {
    disabled_date?: string | null;
    format: string;
    range: boolean;
    show_time: boolean;
  };
  type: "DatePicker";
} | {
  config: {
    format: string;
    step: TimeStep;
    use12_hours: boolean;
  };
  type: "TimePicker";
} | {
  config: {
    allow_clear: boolean;
    allow_half: boolean;
    character: string;
    count: number;
  };
  type: "Rate";
} | {
  config: {
    marks: Record<string, string>;
    max: number;
    min: number;
    range: boolean;
    step: number;
    tooltip: boolean;
  };
  type: "Slider";
} | {
  config: {
    data_source: string;
    filterable: boolean;
    operations: Array<string>;
    titles: Array<string>;
  };
  type: "Transfer";
} | {
  config: {
    change_on_select: boolean;
    data_source: string;
    expand_trigger: string;
    field_names: CascaderFieldNames;
  };
  type: "Cascader";
} | {
  config: {
    data_source: string;
    field_names: TreeSelectFieldNames;
    show_search: boolean;
    tree_default_expand_all: boolean;
  };
  type: "TreeSelect";
} | {
  config: {
    prefix: string;
    split: string;
    suggestions: Array<string>;
  };
  type: "Mention";
} | {
  config: {
    icon?: string | null;
    level: string;
    size: number;
    value: string;
  };
  type: "QRCode";
} | {
  config: {
    component_name: string;
    props: Record<string, unknown>;
  };
  type: "Custom";
};

export interface ComponentVariant {
  class: string;
  props: Record<string, unknown>;
}

export interface ConditionalVisibility {
  field_id: FieldId;
  operator: VisibilityOperator;
  value: unknown;
}

export type ConstraintId = string;

export type ConstraintType = "primary_key" | "unique" | "check" | "exclusion" | "foreign_key";

export interface ContactInfo {
  email: string;
  name: string;
  url: string;
}

export type ContainerId = string;

export interface ContainerProps {
  align_items?: string | null;
  background?: string | null;
  border?: string | null;
  border_radius?: string | null;
  bottom?: string | null;
  box_shadow?: string | null;
  css_classes: Array<string>;
  custom_styles: Record<string, string>;
  display?: string | null;
  flex_direction?: string | null;
  gap?: string | null;
  height?: string | null;
  justify_content?: string | null;
  left?: string | null;
  margin?: string | null;
  max_height?: string | null;
  max_width?: string | null;
  min_height?: string | null;
  min_width?: string | null;
  overflow?: string | null;
  padding?: string | null;
  position?: string | null;
  right?: string | null;
  top?: string | null;
  width?: string | null;
  z_index?: number | null;
}

export interface ContainerSizes {
  full: string;
  lg: string;
  md: string;
  sm: string;
  xl: string;
  xxl: string;
}

export interface CorsConfig {
  allow_credentials: boolean;
  allow_headers: Array<string>;
  allow_methods: Array<string>;
  allow_origins: Array<string>;
  enabled: boolean;
  expose_headers: Array<string>;
  max_age_seconds: number;
}

export interface CurrencyConfig {
  code: string;
  display: CurrencyDisplay;
  locale: string;
  symbol: string;
}

export type CurrencyDisplay = "symbol" | "code" | "name" | "narrow_symbol";

export interface CustomValidator {
  expression: string;
  fields: Array<FieldId>;
  message: string;
  name: string;
}

export interface DarkModeConfig {
  enabled: boolean;
  selector: string;
  strategy: DarkModeStrategy;
  tokens?: DesignTokens | null;
}

export type DarkModeStrategy = "class" | "media_query" | "data_attribute" | "manual";

export interface DataBinding {
  source: BindingSource;
  target: BindingTarget;
  transform?: string | null;
  two_way: boolean;
}

export interface DataSource {
  auto_load: boolean;
  config: DataSourceConfig;
  id: string;
  name: string;
  polling_interval?: number | null;
  source_type: DataSourceType;
  transform?: string | null;
}

export interface DataSourceConfig {
  cache: CacheConfig;
  debounce_ms?: number | null;
  retry: RetryConfig3;
  timeout_ms: number;
  transform_error?: string | null;
  transform_response?: string | null;
}

export type DataSourceType = {
  config: {
    body?: string | null;
    endpoint: string;
    headers: Record<string, string>;
    method: HttpMethod;
    params: Record<string, string>;
  };
  type: "Api";
} | {
  config: {
    endpoint: string;
    query: string;
    variables: Record<string, string>;
  };
  type: "GraphQL";
} | {
  config: {
    message_handler: string;
    protocols: Array<string>;
    url: string;
  };
  type: "WebSocket";
} | {
  config: {
    entity_id: EntityId;
    query: DatabaseQuery;
  };
  type: "Database";
} | {
  config: {
    data: unknown;
  };
  type: "Static";
} | {
  config: {
    dependencies: Array<string>;
    expression: string;
  };
  type: "Computed";
} | {
  config: {
    keys: Array<string>;
    store_id: string;
  };
  type: "Store";
} | {
  config: {
    param: string;
    transform?: string | null;
  };
  type: "UrlParam";
} | {
  config: {
    default?: unknown;
    key: string;
  };
  type: "LocalStorage";
} | {
  config: {
    default?: unknown;
    key: string;
  };
  type: "SessionStorage";
} | {
  config: {
    default?: unknown;
    name: string;
  };
  type: "Cookie";
};

export interface DatabaseConfig {
  connect_timeout_seconds: number;
  driver: DatabaseDriver;
  idle_timeout_seconds: number;
  max_connections: number;
  max_lifetime_seconds: number;
  migrations: MigrationConfig;
  min_connections: number;
  pool_options: PoolOptions;
  schema?: string | null;
  ssl_mode: SslMode;
  url: string;
}

export type DatabaseDriver = "postgres" | "mysql" | "sqlite" | "sql_server" | "oracle";

export interface DatabaseQuery {
  group_by: Array<string>;
  having: Array<WhereClause>;
  limit?: number | null;
  offset?: number | null;
  order_by: Array<OrderByClause>;
  select: Array<string>;
  where: Array<WhereClause>;
}

export interface DatabaseQueueConfig {
  failed_table: string;
  table: string;
}

export interface DesignTokens {
  borders: BorderTokens;
  breakpoints: BreakpointTokens;
  colors: ColorTokens;
  custom: Record<string, unknown>;
  shadows: ShadowTokens;
  sizing: SizingTokens;
  spacing: SpacingTokens;
  transitions: TransitionTokens;
  typography: TypographyTokens;
  z_indices: ZIndexTokens;
}

export interface DetailField {
  label: string;
  label_col?: number | null;
  span?: number | null;
  value: string;
  value_col?: number | null;
}

export type EdgeId = string;

export interface EdgeStyle {
  animated: boolean;
  marker_end?: string | null;
  stroke_color?: string | null;
  stroke_dasharray?: string | null;
  stroke_width?: number | null;
}

export interface EmailAddress {
  address: string;
  name: string;
}

export interface EmailConfig {
  driver: EmailDriver;
  from: EmailAddress;
  mailgun: MailgunConfig;
  queue: EmailQueueConfig;
  reply_to?: EmailAddress | null;
  sendgrid: SendGridConfig;
  ses: SesConfig;
  smtp: SmtpConfig;
  templates: Record<string, EmailTemplate>;
}

export type EmailDriver = "smtp" | "send_grid" | "mailgun" | "s_e_s" | "console" | "null" | "custom";

export interface EmailQueueConfig {
  backoff_seconds: number;
  concurrency: number;
  database: DatabaseQueueConfig;
  driver: QueueDriver;
  enabled: boolean;
  max_attempts: number;
  redis: RedisConfig;
}

export interface EmailTemplate {
  html: string;
  subject: string;
  text?: string | null;
  variables: Array<string>;
}

export type EndEventType = "none" | "terminate" | "multiple" | {
  message: {
    message_ref: string;
  };
} | {
  error: {
    error_ref: string;
  };
} | {
  escalation: {
    escalation_ref: string;
  };
} | {
  compensation: {
    activity_ref: string;
  };
} | {
  signal: {
    signal_ref: string;
  };
} | {
  link: {
    target_ref: string;
  };
};

export interface Entity {
  constraints: Array<EntityConstraint>;
  created_at: string;
  description?: string | null;
  fields: Array<EntityField>;
  id: EntityId;
  indexes: Array<EntityIndex>;
  name: string;
  options: EntityOptions;
  plural_name: string;
  relations: Array<EntityRelation>;
  updated_at: string;
}

export interface EntityConstraint {
  constraint_type: ConstraintType;
  expression?: string | null;
  fields: Array<FieldId2>;
  id: ConstraintId;
  name: string;
}

export interface EntityField {
  auto_generate?: AutoGenerate | null;
  comment?: string | null;
  default?: unknown;
  field_type: EntityFieldType;
  id: FieldId2;
  name: string;
  nullable: boolean;
  order: number;
  unique: boolean;
  validation?: string | null;
}

export type EntityFieldType = {
  config: {
    max_length?: number | null;
  };
  type: "String";
} | {
  type: "Text";
} | {
  type: "Integer";
} | {
  type: "BigInteger";
} | {
  type: "Float";
} | {
  config: {
    precision: number;
    scale: number;
  };
  type: "Decimal";
} | {
  type: "Boolean";
} | {
  type: "Date";
} | {
  type: "DateTime";
} | {
  type: "Time";
} | {
  type: "Timestamp";
} | {
  type: "Json";
} | {
  type: "Jsonb";
} | {
  type: "Uuid";
} | {
  config: {
    values: Array<string>;
  };
  type: "Enum";
} | {
  type: "Bytes";
} | {
  config: RelationType;
  type: "Relation";
};

export type EntityId = string;

export interface EntityIndex {
  condition?: string | null;
  fields: Array<FieldId2>;
  id: IndexId;
  index_type: IndexType;
  name: string;
  unique: boolean;
}

export interface EntityOptions {
  audit_log: boolean;
  comment?: string | null;
  enable_rls: boolean;
  rls_policies: Array<RlsPolicy>;
  schema?: string | null;
  soft_delete: boolean;
  table_name?: string | null;
  timestamps: boolean;
}

export interface EntityRelation {
  from_field: FieldId2;
  id: RelationId;
  name: string;
  on_delete: OnDelete;
  on_update: OnUpdate;
  relation_type: RelationType;
  to_entity: EntityId;
  to_field: FieldId2;
}

export interface EnvironmentConfig {
  database?: DatabaseConfig | null;
  description?: string | null;
  display_name: string;
  domain?: string | null;
  email?: EmailConfig | null;
  features: FeatureFlags;
  name: string;
  replicas: number;
  resources: ResourceLimits;
  secrets: Array<string>;
  ssl: boolean;
  storage?: StorageConfig | null;
  variables: Record<string, string>;
}

export interface EventHandler {
  action_id: string;
  action_type: ActionType;
  payload_mapping: Record<string, string>;
}

export type EventModifier = "prevent" | "stop" | "once" | "passive" | "capture" | "self_";

export type EventTarget = "self_" | "parent" | "window" | "document" | "broadcast" | {
  component: ComponentId;
};

export type ExecutionEvent = "start" | "end" | "take" | "error" | "timeout";

export interface ExecutionListener {
  event: ExecutionEvent;
  implementation: ListenerImplementation;
}

export interface FeatureFlags {
  ai_assist: boolean;
  api: boolean;
  audit_log: boolean;
  auth: boolean;
  email: boolean;
  files: boolean;
  localization: boolean;
  marketplace: boolean;
  multi_tenant: boolean;
  plugins: boolean;
  realtime: boolean;
  scheduler: boolean;
  soft_delete: boolean;
  theming: boolean;
  webhooks: boolean;
  workflows: boolean;
}

export type FieldId = string;

export type FieldId2 = string;

export interface FieldLayout {
  column_span: number;
  column_start: number;
  row_span?: number | null;
  row_start?: number | null;
}

export interface FieldPermissions {
  read_roles: Array<string>;
  write_roles: Array<string>;
}

export type FieldType = {
  config: {
    max_length?: number | null;
    placeholder?: string | null;
  };
  type: "Text";
} | {
  config: {
    max_length?: number | null;
    rows?: number | null;
  };
  type: "TextArea";
} | {
  config: {
    max?: number | null;
    min?: number | null;
    step?: number | null;
  };
  type: "Number";
} | {
  config: {
    max?: number | null;
    min?: number | null;
  };
  type: "Integer";
} | {
  type: "Boolean";
} | {
  config: {
    multiple: boolean;
    options: Array<SelectOption>;
    searchable: boolean;
  };
  type: "Select";
} | {
  config: {
    options: Array<SelectOption>;
  };
  type: "Radio";
} | {
  config: {
    options: Array<SelectOption>;
  };
  type: "Checkbox";
} | {
  type: "Date";
} | {
  type: "DateTime";
} | {
  type: "Time";
} | {
  type: "Email";
} | {
  type: "Url";
} | {
  type: "Phone";
} | {
  config: {
    accept?: string | null;
    max_size_mb?: number | null;
    multiple: boolean;
  };
  type: "File";
} | {
  config: {
    max_height?: number | null;
    max_width?: number | null;
  };
  type: "Image";
} | {
  type: "Signature";
} | {
  type: "RichText";
} | {
  type: "Json";
} | {
  config: {
    display_field: string;
    entity_id: EntityId;
    multiple: boolean;
  };
  type: "Relation";
} | {
  config: {
    expression: string;
    return_type: FieldType;
  };
  type: "Computed";
} | {
  config: {
    fields: Array<FormField>;
  };
  type: "Section";
} | {
  config: {
    fields: Array<FormField>;
    max?: number | null;
    min: number;
  };
  type: "Repeater";
};

export interface FieldUiProps {
  css_classes: Array<string>;
  custom_attrs: Record<string, string>;
  disabled: boolean;
  help_text?: string | null;
  hidden: boolean;
  label_position: LabelPosition;
  placeholder?: string | null;
  prefix?: string | null;
  readonly: boolean;
  suffix?: string | null;
  width?: string | null;
}

export interface FontFamilies {
  custom: Record<string, string>;
  display?: string | null;
  mono: string;
  sans: string;
  serif: string;
}

export interface FontSizes {
  base: string;
  display_base: string;
  display_lg: string;
  display_sm: string;
  display_xl: string;
  display_xs: string;
  display_xxl: string;
  lg: string;
  sm: string;
  xl: string;
  xs: string;
  xxl: string;
  xxxl: string;
}

export interface FontWeights {
  black: number;
  bold: number;
  extra_bold: number;
  extra_light: number;
  light: number;
  medium: number;
  normal: number;
  semi_bold: number;
  thin: number;
}

export interface Form {
  created_at: string;
  description?: string | null;
  fields: Array<FormField>;
  id: FormId;
  layout: FormLayout;
  name: string;
  permissions: FormPermissions;
  updated_at: string;
  validation: FormValidation;
}

export interface FormField {
  conditional_visibility?: ConditionalVisibility | null;
  default_value?: unknown;
  field_type: FieldType;
  id: FieldId;
  label: string;
  name: string;
  order: number;
  required: boolean;
  ui_props: FieldUiProps;
  validation_rules: Array<ValidationRule>;
}

export type FormId = string;

export interface FormLayout {
  column_gap: string;
  columns: number;
  fields: Record<string, FieldLayout>;
  row_gap: string;
}

export interface FormPermissions {
  create_roles: Array<string>;
  delete_roles: Array<string>;
  field_permissions: Record<string, FieldPermissions>;
  read_roles: Array<string>;
  update_roles: Array<string>;
}

export interface FormValidation {
  custom_validators: Array<CustomValidator>;
  validate_on_blur: boolean;
  validate_on_change: boolean;
  validate_on_submit: boolean;
}

export interface FrontendConfig {
  bundler: Bundler;
  framework: FrontendFramework;
}

export type FrontendFramework = "leptos";

export interface GcsConfig {
  bucket: string;
  credentials_json: string;
  location: string;
  project_id: string;
}

export interface GridConfig {
  bottom?: string | null;
  contain_label: boolean;
  left?: string | null;
  right?: string | null;
  show: boolean;
  top?: string | null;
}

export interface HeightScale {
  auto: string;
  full: string;
  max: string;
  min: string;
  px: string;
  scale: Record<string, string>;
  screen: string;
}

export type HistoryLevel = "none" | "activity" | "audit" | "full";

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";

export type IndexId = string;

export type IndexType = "b_tree" | "hash" | "gin" | "gist" | "sp_gist" | "brin";

export interface LabelConfig {
  color?: string | null;
  font_size?: number | null;
  formatter?: string | null;
  position?: string | null;
  show: boolean;
}

export type LabelPosition = "top" | "left" | "right" | "inline" | "floating";

export interface LayoutContainer {
  children: Array<LayoutContainer>;
  component_id?: ComponentId | null;
  id: ContainerId;
  name: string;
  props: ContainerProps;
}

export interface LayoutSpacing {
  component_gap: string;
  container_padding: string;
  inline_gap: string;
  section_gap: string;
}

export type LayoutType = "fixed" | "fluid" | "grid" | "flex";

export interface LegendConfig {
  bottom?: string | null;
  left?: string | null;
  orient?: string | null;
  right?: string | null;
  show: boolean;
  top?: string | null;
}

export interface LetterSpacings {
  normal: string;
  tight: string;
  tighter: string;
  wide: string;
  wider: string;
  widest: string;
}

export interface LicenseInfo {
  name: string;
  url: string;
}

export interface LineHeights {
  loose: string;
  none: string;
  normal: string;
  relaxed: string;
  snug: string;
  tight: string;
}

export type ListenerImplementation = {
  config: {
    script: string;
  };
  type: "Script";
} | {
  config: {
    expression: string;
  };
  type: "Expression";
} | {
  config: {
    class_name: string;
  };
  type: "Delegate";
};

export interface LocalStorageConfig {
  root_path: string;
  serve_via_app: boolean;
  url_prefix: string;
}

export type LogLevel = "trace" | "debug" | "info" | "warn" | "error";

export type LogicalOperator = "and" | "or";

export interface MailgunConfig {
  api_key: string;
  base_url: string;
  domain: string;
}

export interface MaxWidthScale {
  full: string;
  lg: string;
  md: string;
  none: string;
  screen: string;
  sm: string;
  xl: string;
  xs: string;
  xxl: string;
}

export interface MenuItem {
  children: Array<MenuItem>;
  danger: boolean;
  disabled: boolean;
  icon?: string | null;
  key: string;
  label: string;
  type: MenuItemType;
}

export type MenuItemType = "item" | "group" | "sub_menu" | "divider";

export type MenuMode = "inline" | "horizontal" | "vertical";

export interface MfaConfig {
  backup_codes: boolean;
  enabled: boolean;
  grace_period_days: number;
  methods: Array<MfaMethod>;
  required: boolean;
}

export type MfaMethod = "totp" | "sms" | "email" | "web_authn" | "backup_codes";

export interface MigrationConfig {
  auto_apply: boolean;
  enabled: boolean;
  lock_timeout_seconds: number;
  table: string;
}

export interface MultiInstanceConfig {
  cardinality?: number | null;
  collection_expression: string;
  completion_condition?: string | null;
  element_variable: string;
  mode: MultiInstanceMode;
}

export type MultiInstanceMode = "sequential" | "parallel";

export interface NodeConfig {
  boundary_events: Array<BoundaryEvent>;
  execution_listeners: Array<ExecutionListener>;
  inputs: Record<string, NodeInput>;
  multi_instance?: MultiInstanceConfig | null;
  outputs: Record<string, NodeOutput>;
  properties: Record<string, unknown>;
}

export type NodeId = string;

export interface NodeInput {
  default?: unknown;
  expression?: string | null;
  input_type: string;
  name: string;
  required: boolean;
}

export interface NodeOutput {
  expression?: string | null;
  name: string;
  output_type: string;
}

export interface NodePosition {
  x: number;
  y: number;
}

export interface NumberFormat {
  decimal_separator: string;
  grouping_separator: string;
  locale: string;
  max_fraction_digits: number;
  min_fraction_digits: number;
}

export interface OAuth2Config {
  auth_url: string;
  client_id: string;
  client_secret: string;
  pkce: boolean;
  redirect_url: string;
  scope: Array<string>;
  token_url: string;
  user_info_url: string;
}

export type OnDelete = "cascade" | "set_null" | "restrict" | "no_action" | "set_default";

export type OnUpdate = "cascade" | "set_null" | "restrict" | "no_action" | "set_default";

export interface OrderByClause {
  direction: OrderDirection;
  field: string;
}

export type OrderDirection = "asc" | "desc";

export interface Page {
  actions: Array<PageAction>;
  components: Array<PageComponent>;
  created_at: string;
  data_sources: Array<DataSource>;
  description?: string | null;
  id: PageId;
  layout: PageLayout;
  name: string;
  path: string;
  permissions: PagePermissions;
  seo: SeoConfig;
  title: string;
  updated_at: string;
}

export interface PageAction {
  action_type: ActionType;
  config: ActionConfig;
  id: ActionId;
  name: string;
  permissions: Array<string>;
}

export interface PageComponent {
  bindings: Array<DataBinding>;
  component: ComponentType;
  conditions: Array<RenderCondition>;
  events: Array<ComponentEvent>;
  id: ComponentId;
  props: Record<string, unknown>;
  slots: Record<string, Array<ComponentId>>;
  style_overrides: Record<string, string>;
}

export type PageId = string;

export interface PageLayout {
  breakpoints: Breakpoints;
  containers: Array<LayoutContainer>;
  global_styles: Record<string, string>;
  type: LayoutType;
}

export interface PagePermissions {
  admin_roles: Array<string>;
  edit_roles: Array<string>;
  public_access: boolean;
  view_roles: Array<string>;
}

export interface PaginationConfig {
  enabled: boolean;
  page_size: number;
  show_quick_jumper: boolean;
  show_size_changer: boolean;
  show_total: boolean;
}

export interface PasswordPolicy {
  lockout_duration_minutes: number;
  lockout_threshold: number;
  max_age_days?: number | null;
  min_length: number;
  prevent_reuse: number;
  require_lowercase: boolean;
  require_numbers: boolean;
  require_symbols: boolean;
  require_uppercase: boolean;
}

export interface PluginConfig {
  author?: string | null;
  config: Record<string, unknown>;
  dependencies: Array<PluginDependency>;
  description?: string | null;
  enabled: boolean;
  entry_points: Array<PluginEntryPoint>;
  homepage?: string | null;
  id: string;
  license?: string | null;
  name: string;
  permissions: Array<PluginPermission>;
  repository?: string | null;
  version: string;
}

export interface PluginDependency {
  id: string;
  optional: boolean;
  version: string;
}

export interface PluginEntryPoint {
  exports: Array<string>;
  name: string;
  path: string;
  type: PluginEntryPointType;
}

export type PluginEntryPointType = "component" | "action" | "data_source" | "field_type" | "workflow_node" | "auth_provider" | "storage_driver" | "email_driver" | "hook" | "middleware" | "command" | "page" | "layout" | "util";

export type PluginPermission = "read_forms" | "write_forms" | "read_entities" | "write_entities" | "read_workflows" | "write_workflows" | "read_pages" | "write_pages" | "read_themes" | "write_themes" | "execute_code" | "access_database" | "access_file_system" | "access_network" | "access_auth" | "access_email" | "access_scheduler" | "manage_plugins" | "manage_users" | "view_audit_log" | {
  custom: string;
};

export interface PoolOptions {
  acquire_retry_attempts: number;
  acquire_retry_interval_ms: number;
  acquire_timeout_seconds: number;
  test_before_acquire: boolean;
}

export type ProjectId = string;

export interface ProjectSettings {
  api: ApiConfig;
  authentication: AuthConfig;
  currency: CurrencyConfig;
  custom: Record<string, unknown>;
  database: DatabaseConfig;
  date_format: string;
  default_locale: string;
  email: EmailConfig;
  features: FeatureFlags;
  frontend?: unknown;
  number_format: NumberFormat;
  storage: StorageConfig;
  supported_locales: Array<string>;
  time_format: string;
  timezone: string;
}

export type QueueDriver = "redis" | "database" | "sync" | "custom";

export interface RateLimitConfig {
  burst: number;
  enabled: boolean;
  key_generator: RateLimitKeyGenerator;
  requests_per_minute: number;
  skip_failed: boolean;
  skip_successful: boolean;
}

export type RateLimitKeyGenerator = "ip" | "user_id" | "api_key" | {
  custom: string;
};

export interface RedisConfig {
  db: number;
  key_prefix: string;
  pool_size: number;
  timeout_ms: number;
  url: string;
}

export type RelationId = string;

export type RelationType = {
  config: {
    entity_id: EntityId;
    field: string;
    on_delete: OnDelete;
    on_update: OnUpdate;
  };
  type: "BelongsTo";
} | {
  config: {
    entity_id: EntityId;
    field: string;
  };
  type: "HasOne";
} | {
  config: {
    entity_id: EntityId;
    field: string;
  };
  type: "HasMany";
} | {
  config: {
    entity_id: EntityId;
    field: string;
    related_field: string;
    through?: string | null;
  };
  type: "ManyToMany";
};

export interface RenderCondition {
  else_render?: ComponentId | null;
  expression: string;
}

export interface ResourceLimits {
  bandwidth_gb?: number | null;
  cpu_cores?: number | null;
  memory_mb?: number | null;
  storage_gb?: number | null;
}

export interface RetryConfig {
  initial_interval_ms: number;
  max_attempts: number;
  max_interval_ms: number;
  multiplier: number;
}

export interface RetryConfig2 {
  initial_interval_ms: number;
  max_attempts: number;
  max_interval_ms: number;
  multiplier: number;
  retryable_status_codes: Array<number>;
}

export interface RetryConfig3 {
  initial_interval_ms: number;
  max_attempts: number;
  max_interval_ms: number;
  multiplier: number;
  retryable_codes: Array<number>;
}

export type RlsCommand = "all" | "select" | "insert" | "update" | "delete";

export interface RlsPolicy {
  command: RlsCommand;
  name: string;
  roles: Array<string>;
  using_expression: string;
  with_check_expression?: string | null;
}

export interface S3Config {
  access_key_id: string;
  acl: string;
  bucket: string;
  endpoint?: string | null;
  force_path_style: boolean;
  region: string;
  secret_access_key: string;
  server_side_encryption: string;
  session_token?: string | null;
}

export type SameSitePolicy = "strict" | "lax" | "none";

export type ScrollAlignment = "start" | "center" | "end" | "nearest";

export type ScrollBehavior = "auto" | "smooth" | "instant";

export interface SelectOption {
  disabled: boolean;
  label: string;
  value: unknown;
}

export interface SelectionConfig {
  column_title?: string | null;
  column_width?: string | null;
  enabled: boolean;
  fixed: boolean;
  type: SelectionType;
}

export type SelectionType = "checkbox" | "radio";

export interface SendGridConfig {
  api_key: string;
  sandbox_mode: boolean;
}

export interface SeoConfig {
  canonical_url?: string | null;
  description?: string | null;
  keywords: Array<string>;
  og_description?: string | null;
  og_image?: string | null;
  og_title?: string | null;
  og_type?: string | null;
  robots?: string | null;
  structured_data?: unknown;
  title?: string | null;
  twitter_card?: string | null;
  twitter_description?: string | null;
  twitter_image?: string | null;
  twitter_title?: string | null;
}

export type SerializationFormat = "json" | "protobuf" | "avro";

export interface SeriesConfig {
  area_style?: Record<string, string> | null;
  data: string;
  item_style?: Record<string, string> | null;
  label?: LabelConfig | null;
  name: string;
  smooth: boolean;
  stack?: string | null;
  type: ChartType;
}

export interface ServerInfo {
  description: string;
  url: string;
}

export type ServiceTaskImplementation = {
  config: {
    body_template?: string | null;
    headers: Record<string, string>;
    method: HttpMethod;
    retry: RetryConfig2;
    timeout_ms: number;
    url: string;
  };
  type: "Http";
} | {
  config: {
    function_name: string;
    input_mapping: Record<string, string>;
    output_mapping: Record<string, string>;
  };
  type: "Function";
} | {
  config: {
    config: unknown;
    plugin_id: string;
  };
  type: "Plugin";
} | {
  config: {
    config: unknown;
    connector_id: string;
    operation: string;
  };
  type: "External";
};

export interface SesConfig {
  access_key_id: string;
  configuration_set?: string | null;
  region: string;
  secret_access_key: string;
}

export interface SessionConfig {
  domain?: string | null;
  path: string;
  rolling: boolean;
  same_site: SameSitePolicy;
  secure_cookies: boolean;
  timeout_minutes: number;
}

export interface ShadowTokens {
  elevated: Record<string, string>;
  focus: string;
  inner: string;
  lg: string;
  md: string;
  none: string;
  outline: string;
  sm: string;
  xl: string;
  xs: string;
  xxl: string;
}

export interface SignalDefinition {
  id: SignalId;
  name: string;
  payload_type?: VariableType | null;
  scope: SignalScope;
}

export type SignalId = string;

export type SignalScope = "global" | "process";

export interface SizingTokens {
  container: ContainerSizes;
  heights: HeightScale;
  max_widths: MaxWidthScale;
  widths: WidthScale;
}

export type SmtpAuthType = "none" | "plain" | "login" | "cram_m_d5" | "x_o_auth2";

export interface SmtpConfig {
  auth_type: SmtpAuthType;
  encryption: SmtpEncryption;
  host: string;
  password: string;
  pool_size: number;
  port: number;
  timeout_seconds: number;
  username: string;
}

export type SmtpEncryption = "none" | "start_t_l_s" | "s_s_l__t_l_s";

export interface SocialAuthConfig {
  apple?: OAuth2Config | null;
  custom: Record<string, OAuth2Config>;
  facebook?: OAuth2Config | null;
  github?: OAuth2Config | null;
  google?: OAuth2Config | null;
  linkedin?: OAuth2Config | null;
  microsoft?: OAuth2Config | null;
  twitter?: OAuth2Config | null;
}

export interface SpacingTokens {
  base: string;
  component: ComponentSpacing;
  layout: LayoutSpacing;
  scale: Record<string, string>;
}

export type SslMode = "disable" | "allow" | "prefer" | "require" | "verify_ca" | "verify_full";

export interface SsoConfig {
  attribute_mapping: AttributeMapping;
  config: Record<string, unknown>;
  enabled: boolean;
  just_in_time_provisioning: boolean;
  provider: SsoProvider;
}

export type SsoProvider = "s_a_m_l" | "o_i_d_c" | "l_d_a_p" | "active_directory" | "custom";

export type StartEventType = "none" | {
  message: {
    message_ref: string;
  };
} | {
  timer: {
    time_cycle?: string | null;
    time_duration?: string | null;
  };
} | {
  signal: {
    signal_ref: string;
  };
} | {
  error: {
    error_ref: string;
  };
} | {
  escalation: {
    escalation_ref: string;
  };
} | {
  compensation: {
    activity_ref: string;
  };
} | {
  conditional: {
    condition: string;
  };
};

export type StateScope = "global" | "page" | "component" | "session" | "local";

export interface StepItem {
  description?: string | null;
  icon?: string | null;
  status: StepStatus;
  title: string;
}

export type StepStatus = "wait" | "process" | "finish" | "error";

export interface StorageConfig {
  allowed_mime_types: Array<string>;
  azure: AzureBlobConfig;
  cdn_url?: string | null;
  driver: StorageDriver;
  gcs: GcsConfig;
  local: LocalStorageConfig;
  max_file_size_mb: number;
  quarantine_suspicious: boolean;
  s3: S3Config;
}

export type StorageDriver = "local" | "s3" | "g_c_s" | "azure_blob" | "custom";

export interface TabConfig {
  closable: boolean;
  disabled: boolean;
  icon?: string | null;
  key: string;
  label: string;
}

export interface TableColumn {
  align?: string | null;
  children: Array<TableColumn>;
  data_index: string;
  ellipsis: boolean;
  filterable: boolean;
  fixed?: string | null;
  key: string;
  max_width?: string | null;
  min_width?: string | null;
  render?: string | null;
  sortable: boolean;
  title: string;
  width?: string | null;
}

export type TaskType = "user" | "service" | "script" | "business_rule" | "send" | "receive" | "manual";

export interface TextColors {
  disabled: string;
  inverse: string;
  link: string;
  link_hover: string;
  on_error: string;
  on_info: string;
  on_primary: string;
  on_secondary: string;
  on_success: string;
  on_warning: string;
  placeholder: string;
  primary: string;
  secondary: string;
  tertiary: string;
}

export interface TextStyle {
  color?: string | null;
  font_family?: string | null;
  font_size?: string | null;
  font_weight?: number | null;
  letter_spacing?: string | null;
  line_height?: string | null;
  text_decoration?: string | null;
  text_transform?: string | null;
}

export interface Theme {
  components: Record<string, ComponentTheme>;
  created_at: string;
  dark_mode: DarkModeConfig;
  description?: string | null;
  global_styles: string;
  id: ThemeId;
  name: string;
  tokens: DesignTokens;
  updated_at: string;
  version: string;
}

export type ThemeId = string;

export interface TimeStep {
  hour: number;
  minute: number;
  second: number;
}

export interface TimerDefinition {
  attached_to?: NodeId | null;
  id: TimerId;
  name: string;
  time_cycle?: string | null;
  time_date?: string | null;
  time_duration?: string | null;
  timer_type: TimerType;
}

export type TimerId = string;

export type TimerType = "duration" | "cycle" | "date";

export interface TooltipConfig {
  background_color?: string | null;
  border_color?: string | null;
  formatter?: string | null;
  show: boolean;
  trigger?: string | null;
}

export interface TransitionDurations {
  fast: string;
  instant: string;
  normal: string;
  slow: string;
  slower: string;
}

export interface TransitionEasings {
  ease_in: string;
  ease_in_back: string;
  ease_in_circ: string;
  ease_in_out: string;
  ease_out: string;
  ease_out_back: string;
  ease_out_circ: string;
  linear: string;
}

export interface TransitionProperties {
  all: string;
  background: string;
  border: string;
  box_shadow: string;
  colors: string;
  opacity: string;
  transform: string;
}

export interface TransitionTokens {
  durations: TransitionDurations;
  easings: TransitionEasings;
  properties: TransitionProperties;
}

export interface TreeFieldNames {
  children: string;
  key: string;
  title: string;
}

export interface TreeSelectFieldNames {
  children: string;
  key: string;
  title: string;
}

export interface TrendConfig {
  is_positive: boolean;
  label?: string | null;
  value: number;
}

export interface TypographyTokens {
  font_families: FontFamilies;
  font_sizes: FontSizes;
  font_weights: FontWeights;
  letter_spacings: LetterSpacings;
  line_heights: LineHeights;
  text_styles: Record<string, TextStyle>;
}

export type ValidationRule = {
  Required: {
    message: string;
  };
} | {
  MinLength: {
    message: string;
    min: number;
  };
} | {
  MaxLength: {
    max: number;
    message: string;
  };
} | {
  Pattern: {
    message: string;
    regex: string;
  };
} | {
  Min: {
    message: string;
    value: number;
  };
} | {
  Max: {
    message: string;
    value: number;
  };
} | {
  Email: {
    message: string;
  };
} | {
  Url: {
    message: string;
  };
} | {
  Custom: {
    expression: string;
    message: string;
  };
};

export type VariableId = string;

export type VariableScope = "global" | "process" | "local" | "task";

export type VariableType = {
  type: "String";
} | {
  type: "Integer";
} | {
  type: "Float";
} | {
  type: "Boolean";
} | {
  type: "Date";
} | {
  type: "DateTime";
} | {
  type: "Json";
} | {
  config: {
    properties: Record<string, VariableType>;
  };
  type: "Object";
} | {
  config: {
    items: VariableType;
  };
  type: "Array";
} | {
  type: "File";
} | {
  type: "Binary";
};

export type VisibilityOperator = "equals" | "not_equals" | "contains" | "not_contains" | "greater_than" | "less_than" | "in" | "not_in" | "is_empty" | "is_not_empty";

export interface WebhookConfig {
  allowed_events: Array<string>;
  enabled: boolean;
  retry: RetryConfig;
  secret_header: string;
  timeout_seconds: number;
}

export interface WhereClause {
  field: string;
  logical: LogicalOperator;
  operator: WhereOperator;
  value: unknown;
}

export type WhereOperator = "eq" | "ne" | "gt" | "gte" | "lt" | "lte" | "like" | "i_like" | "in" | "not_in" | "between" | "not_between" | "is_null" | "is_not_null" | "contains" | "not_contains" | "starts_with" | "ends_with";

export interface WidthScale {
  auto: string;
  full: string;
  max: string;
  min: string;
  px: string;
  scale: Record<string, string>;
  screen: string;
}

export interface Workflow {
  created_at: string;
  description?: string | null;
  edges: Array<WorkflowEdge>;
  id: WorkflowId;
  name: string;
  nodes: Array<WorkflowNode>;
  settings: WorkflowSettings;
  signals: Array<SignalDefinition>;
  timers: Array<TimerDefinition>;
  updated_at: string;
  variables: Array<WorkflowVariable>;
  version: string;
}

export interface WorkflowEdge {
  condition?: string | null;
  id: EdgeId;
  label?: string | null;
  source: NodeId;
  source_handle?: string | null;
  style: EdgeStyle;
  target: NodeId;
  target_handle?: string | null;
}

export type WorkflowId = string;

export interface WorkflowNode {
  config: NodeConfig;
  documentation?: string | null;
  id: NodeId;
  name: string;
  node_type: WorkflowNodeType;
  position: NodePosition;
}

export type WorkflowNodeType = {
  config: {
    event_type: StartEventType;
  };
  type: "StartEvent";
} | {
  config: {
    event_type: EndEventType;
  };
  type: "EndEvent";
} | {
  config: {
    assignee?: string | null;
    candidate_groups: Array<string>;
    due_date?: string | null;
    form_id?: FormId | null;
  };
  type: "UserTask";
} | {
  config: {
    async: boolean;
    exclusive: boolean;
    implementation: ServiceTaskImplementation;
  };
  type: "ServiceTask";
} | {
  config: {
    script: string;
    script_format: string;
  };
  type: "ScriptTask";
} | {
  config: {
    decision_ref: string;
    decision_table?: string | null;
  };
  type: "BusinessRuleTask";
} | {
  config: {
    message_ref: string;
  };
  type: "SendTask";
} | {
  config: {
    message_ref: string;
  };
  type: "ReceiveTask";
} | {
  type: "ManualTask";
} | {
  config: {
    default_flow?: EdgeId | null;
  };
  type: "ExclusiveGateway";
} | {
  config: {
    default_flow?: EdgeId | null;
  };
  type: "InclusiveGateway";
} | {
  type: "ParallelGateway";
} | {
  type: "EventBasedGateway";
} | {
  config: {
    is_event_subprocess: boolean;
    is_transaction: boolean;
    workflow_id?: WorkflowId | null;
  };
  type: "SubProcess";
} | {
  config: {
    called_workflow_id: WorkflowId;
    input_mapping: Record<string, string>;
    output_mapping: Record<string, string>;
  };
  type: "CallActivity";
} | {
  config: {
    task_type: TaskType;
  };
  type: "Task";
};

export interface WorkflowSettings {
  async_continuations: boolean;
  exclusive_gateway_default: boolean;
  history_level: HistoryLevel;
  is_executable: boolean;
  retry_config?: RetryConfig2 | null;
  timeout?: string | null;
  variables_serialization: SerializationFormat;
}

export interface WorkflowVariable {
  default_value?: unknown;
  description?: string | null;
  id: VariableId;
  name: string;
  scope: VariableScope;
  variable_type: VariableType;
}

export interface ZIndexTokens {
  auto: string;
  base: string;
  dropdown: string;
  fixed: string;
  modal: string;
  modal_backdrop: string;
  popover: string;
  sticky: string;
  toast: string;
  tooltip: string;
}

export interface Project {
  active_theme_id?: ThemeId | null;
  created_at: string;
  description?: string | null;
  entities: Record<string, Entity>;
  environments: Record<string, EnvironmentConfig>;
  forms: Record<string, Form>;
  icon?: string | null;
  id: ProjectId;
  name: string;
  pages: Record<string, Page>;
  plugins: Array<PluginConfig>;
  published_at?: string | null;
  settings: ProjectSettings;
  slug: string;
  themes: Record<string, Theme>;
  updated_at: string;
  version: string;
  workflows: Record<string, Workflow>;
}
