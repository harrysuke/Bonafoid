use crate::form::FormId;
use crate::retry::RetryConfig;
use crate::workflow::{HttpMethod, WorkflowId};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Page {
    pub id: PageId,
    pub name: String,
    pub path: String,
    pub title: String,
    pub description: Option<String>,
    pub layout: PageLayout,
    pub components: Vec<PageComponent>,
    pub data_sources: Vec<DataSource>,
    pub actions: Vec<PageAction>,
    pub seo: SeoConfig,
    pub permissions: PagePermissions,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Page {
    /// Create a new page with a minimal fluid layout.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: PageId::new(),
            name: name.into(),
            path: "/".to_string(),
            title: String::new(),
            description: None,
            layout: PageLayout {
                r#type: LayoutType::Fluid,
                containers: Vec::new(),
                breakpoints: Breakpoints::default(),
                global_styles: IndexMap::new(),
            },
            components: Vec::new(),
            data_sources: Vec::new(),
            actions: Vec::new(),
            seo: SeoConfig::default(),
            permissions: PagePermissions::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct PageId(pub Uuid);

impl PageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for PageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PageLayout {
    pub r#type: LayoutType,
    pub containers: Vec<LayoutContainer>,
    pub breakpoints: Breakpoints,
    pub global_styles: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutType {
    Fixed,
    Fluid,
    Grid,
    Flex,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LayoutContainer {
    pub id: ContainerId,
    pub name: String,
    pub component_id: Option<ComponentId>,
    pub children: Vec<LayoutContainer>,
    pub props: ContainerProps,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct ContainerId(pub Uuid);

impl ContainerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ContainerId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ContainerProps {
    pub display: Option<String>,
    pub flex_direction: Option<String>,
    pub justify_content: Option<String>,
    pub align_items: Option<String>,
    pub gap: Option<String>,
    pub padding: Option<String>,
    pub margin: Option<String>,
    pub width: Option<String>,
    pub height: Option<String>,
    pub min_width: Option<String>,
    pub max_width: Option<String>,
    pub min_height: Option<String>,
    pub max_height: Option<String>,
    pub background: Option<String>,
    pub border: Option<String>,
    pub border_radius: Option<String>,
    pub box_shadow: Option<String>,
    pub overflow: Option<String>,
    pub position: Option<String>,
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
    pub z_index: Option<i32>,
    pub css_classes: Vec<String>,
    pub custom_styles: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct Breakpoints {
    pub xs: Option<String>,
    pub sm: Option<String>,
    pub md: Option<String>,
    pub lg: Option<String>,
    pub xl: Option<String>,
    pub xxl: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PageComponent {
    pub id: ComponentId,
    pub component: ComponentType,
    pub props: IndexMap<String, serde_json::Value>,
    pub bindings: Vec<DataBinding>,
    pub events: Vec<ComponentEvent>,
    pub conditions: Vec<RenderCondition>,
    pub slots: IndexMap<String, Vec<ComponentId>>,
    pub style_overrides: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct ComponentId(pub Uuid);

impl ComponentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ComponentId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum ComponentType {
    // Layout
    Container {
        children: Vec<ComponentId>,
    },
    Grid {
        columns: u8,
        gap: String,
        children: Vec<ComponentId>,
    },
    Flex {
        direction: String,
        gap: String,
        children: Vec<ComponentId>,
    },
    Tabs {
        tabs: Vec<TabConfig>,
        children: Vec<ComponentId>,
    },
    Accordion {
        items: Vec<AccordionItem>,
        children: Vec<ComponentId>,
    },
    Card {
        header: Option<String>,
        footer: Option<String>,
        children: Vec<ComponentId>,
    },
    Modal {
        trigger: ComponentId,
        children: Vec<ComponentId>,
    },
    Drawer {
        trigger: ComponentId,
        position: String,
        children: Vec<ComponentId>,
    },
    Tooltip {
        trigger: ComponentId,
        content: String,
        position: String,
    },
    Popover {
        trigger: ComponentId,
        content: Vec<ComponentId>,
        position: String,
    },

    // Data Display
    Table {
        columns: Vec<TableColumn>,
        data_source: String,
        pagination: PaginationConfig,
        sorting: bool,
        filtering: bool,
        selection: SelectionConfig,
    },
    List {
        item_template: ComponentId,
        data_source: String,
        empty_state: Option<ComponentId>,
    },
    DetailView {
        fields: Vec<DetailField>,
        data_source: String,
    },
    Chart {
        chart_type: ChartType,
        data_source: String,
        /// Boxed: `ChartConfig` is 632 bytes and made this enum 664 bytes
        /// all on its own. `Box` is transparent to serde and schemars, so
        /// the wire format is unchanged.
        config: Box<ChartConfig>,
    },
    StatCard {
        value: String,
        label: String,
        trend: Option<TrendConfig>,
        icon: Option<String>,
    },
    ProgressBar {
        value: String,
        max: String,
        format: String,
        color: Option<String>,
    },
    Avatar {
        src: String,
        alt: String,
        size: String,
        shape: String,
    },
    Badge {
        content: String,
        color: String,
        variant: String,
    },
    Tag {
        content: String,
        color: String,
        closable: bool,
    },
    Divider {
        orientation: String,
        dashed: bool,
        text: Option<String>,
    },

    // Data Input
    Form {
        form_id: FormId,
        submit_action: String,
        reset_action: Option<String>,
    },
    FormField {
        field_id: String,
        label: String,
        required: bool,
    },
    Button {
        label: String,
        variant: ButtonVariant,
        size: ButtonSize,
        icon: Option<String>,
        loading: bool,
        disabled: bool,
        action: String,
    },
    ButtonGroup {
        buttons: Vec<ButtonConfig>,
        variant: ButtonVariant,
    },
    Link {
        label: String,
        href: String,
        target: String,
        icon: Option<String>,
    },
    Breadcrumb {
        items: Vec<BreadcrumbItem>,
    },
    Pagination {
        total: String,
        page_size: u32,
        show_size_changer: bool,
        show_quick_jumper: bool,
    },
    Steps {
        items: Vec<StepItem>,
        current: u32,
        direction: String,
    },
    Tree {
        data_source: String,
        field_names: TreeFieldNames,
        checkable: bool,
        draggable: bool,
    },

    // Feedback
    Alert {
        message: String,
        r#type: AlertType,
        closable: bool,
        show_icon: bool,
    },
    Message {
        content: String,
        r#type: AlertType,
        duration: u32,
    },
    Notification {
        title: String,
        content: String,
        r#type: AlertType,
        duration: u32,
    },
    Loading {
        spinning: bool,
        tip: Option<String>,
        delay: u32,
    },
    Skeleton {
        active: bool,
        avatar: bool,
        title: bool,
        paragraph: bool,
        rows: u32,
    },
    Spin {
        spinning: bool,
        size: String,
        tip: Option<String>,
    },

    // Navigation
    Menu {
        items: Vec<MenuItem>,
        mode: MenuMode,
        theme: String,
    },
    Dropdown {
        trigger: ComponentId,
        items: Vec<MenuItem>,
        placement: String,
    },
    Anchor {
        links: Vec<AnchorLink>,
        affix: bool,
    },
    BackTop {
        visibility_height: u32,
    },

    // Other
    Html {
        content: String,
        sanitize: bool,
    },
    Iframe {
        src: String,
        width: String,
        height: String,
        sandbox: Vec<String>,
    },
    Image {
        src: String,
        alt: String,
        width: Option<String>,
        height: Option<String>,
        fallback: Option<String>,
        preview: bool,
    },
    Video {
        src: String,
        poster: String,
        controls: bool,
        autoplay: bool,
        loop_play: bool,
    },
    Audio {
        src: String,
        controls: bool,
        autoplay: bool,
        loop_play: bool,
    },
    FileUpload {
        action: String,
        accept: String,
        multiple: bool,
        directory: bool,
        drag: bool,
        list_type: String,
    },
    ColorPicker {
        format: String,
        alpha: bool,
        preset_colors: Vec<String>,
    },
    DatePicker {
        format: String,
        show_time: bool,
        range: bool,
        disabled_date: Option<String>,
    },
    TimePicker {
        format: String,
        use12_hours: bool,
        step: TimeStep,
    },
    Rate {
        count: u32,
        allow_half: bool,
        allow_clear: bool,
        character: String,
    },
    Slider {
        min: f64,
        max: f64,
        step: f64,
        range: bool,
        marks: IndexMap<String, String>,
        tooltip: bool,
    },
    Transfer {
        data_source: String,
        titles: Vec<String>,
        operations: Vec<String>,
        filterable: bool,
    },
    Cascader {
        data_source: String,
        field_names: CascaderFieldNames,
        expand_trigger: String,
        change_on_select: bool,
    },
    TreeSelect {
        data_source: String,
        field_names: TreeSelectFieldNames,
        tree_default_expand_all: bool,
        show_search: bool,
    },
    Mention {
        suggestions: Vec<String>,
        prefix: String,
        split: String,
    },
    QRCode {
        value: String,
        size: u32,
        level: String,
        icon: Option<String>,
    },
    Custom {
        component_name: String,
        props: IndexMap<String, serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TabConfig {
    pub key: String,
    pub label: String,
    pub icon: Option<String>,
    pub disabled: bool,
    pub closable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AccordionItem {
    pub key: String,
    pub label: String,
    pub icon: Option<String>,
    pub disabled: bool,
    pub children: Vec<ComponentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TableColumn {
    pub key: String,
    pub title: String,
    pub data_index: String,
    pub width: Option<String>,
    pub min_width: Option<String>,
    pub max_width: Option<String>,
    pub fixed: Option<String>,
    pub align: Option<String>,
    pub ellipsis: bool,
    pub sortable: bool,
    pub filterable: bool,
    pub render: Option<String>,
    pub children: Vec<TableColumn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct PaginationConfig {
    pub enabled: bool,
    pub page_size: u32,
    pub show_size_changer: bool,
    pub show_quick_jumper: bool,
    pub show_total: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SelectionConfig {
    pub enabled: bool,
    pub r#type: SelectionType,
    pub column_width: Option<String>,
    pub column_title: Option<String>,
    pub fixed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionType {
    #[default]
    Checkbox,
    Radio,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct DetailField {
    pub label: String,
    pub value: String,
    pub span: Option<u8>,
    pub label_col: Option<u8>,
    pub value_col: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    Line,
    Bar,
    Pie,
    Area,
    Scatter,
    Radar,
    Funnel,
    Gauge,
    Heatmap,
    Treemap,
    Sankey,
    WordCloud,
    LiquidFill,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ChartConfig {
    pub x_axis: Option<AxisConfig>,
    pub y_axis: Option<AxisConfig>,
    pub series: Vec<SeriesConfig>,
    pub legend: Option<LegendConfig>,
    pub tooltip: Option<TooltipConfig>,
    pub grid: Option<GridConfig>,
    pub theme: Option<String>,
    pub animation: bool,
    pub responsive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct AxisConfig {
    pub r#type: Option<String>,
    pub name: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub interval: Option<f64>,
    pub formatter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SeriesConfig {
    pub name: String,
    pub r#type: ChartType,
    pub data: String,
    pub smooth: bool,
    pub stack: Option<String>,
    pub area_style: Option<IndexMap<String, String>>,
    pub item_style: Option<IndexMap<String, String>>,
    pub label: Option<LabelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct LegendConfig {
    pub show: bool,
    pub orient: Option<String>,
    pub left: Option<String>,
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TooltipConfig {
    pub show: bool,
    pub trigger: Option<String>,
    pub formatter: Option<String>,
    pub background_color: Option<String>,
    pub border_color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct GridConfig {
    pub show: bool,
    pub left: Option<String>,
    pub right: Option<String>,
    pub top: Option<String>,
    pub bottom: Option<String>,
    pub contain_label: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct LabelConfig {
    pub show: bool,
    pub position: Option<String>,
    pub formatter: Option<String>,
    pub color: Option<String>,
    pub font_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TrendConfig {
    pub value: f64,
    pub is_positive: bool,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Success,
    Warning,
    Danger,
    Ghost,
    Link,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ButtonSize {
    Large,
    Middle,
    Small,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ButtonConfig {
    pub label: String,
    pub variant: ButtonVariant,
    pub size: ButtonSize,
    pub icon: Option<String>,
    pub action: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct BreadcrumbItem {
    pub label: String,
    pub href: Option<String>,
    pub icon: Option<String>,
    pub separator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct StepItem {
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Wait,
    Process,
    Finish,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TreeFieldNames {
    pub title: String,
    pub key: String,
    pub children: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AlertType {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MenuItem {
    pub key: String,
    pub label: String,
    pub icon: Option<String>,
    pub disabled: bool,
    pub danger: bool,
    pub children: Vec<MenuItem>,
    pub r#type: MenuItemType,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MenuItemType {
    Item,
    Group,
    SubMenu,
    Divider,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MenuMode {
    Inline,
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AnchorLink {
    pub href: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TimeStep {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CascaderFieldNames {
    pub label: String,
    pub value: String,
    pub children: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TreeSelectFieldNames {
    pub title: String,
    pub key: String,
    pub children: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct DataBinding {
    pub target: BindingTarget,
    pub source: BindingSource,
    pub transform: Option<String>,
    pub two_way: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub enum BindingTarget {
    Prop(String),
    Attr(String),
    Style(String),
    Class(String),
    Event(String),
    Slot(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub enum BindingSource {
    Variable(String),
    DataSource { source_id: String, path: String },
    Computed(String),
    Param(String),
    State(String),
    Prop(String),
    Const(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ComponentEvent {
    pub event: String,
    pub handler: EventHandler,
    pub modifiers: Vec<EventModifier>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EventHandler {
    pub action_type: ActionType,
    pub action_id: String,
    pub payload_mapping: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Navigate,
    SubmitForm,
    ResetForm,
    OpenModal,
    CloseModal,
    CallApi,
    ExecuteWorkflow,
    SetState,
    EmitEvent,
    CopyToClipboard,
    Download,
    Print,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventModifier {
    Prevent,
    Stop,
    Once,
    Passive,
    Capture,
    Self_,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RenderCondition {
    pub expression: String,
    pub else_render: Option<ComponentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct DataSource {
    pub id: String,
    pub name: String,
    pub source_type: DataSourceType,
    pub config: DataSourceConfig,
    pub auto_load: bool,
    pub polling_interval: Option<u32>,
    pub transform: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum DataSourceType {
    Api {
        endpoint: String,
        method: HttpMethod,
        headers: IndexMap<String, String>,
        params: IndexMap<String, String>,
        body: Option<String>,
    },
    GraphQL {
        endpoint: String,
        query: String,
        variables: IndexMap<String, String>,
    },
    WebSocket {
        url: String,
        protocols: Vec<String>,
        message_handler: String,
    },
    Database {
        entity_id: crate::entity::EntityId,
        query: DatabaseQuery,
    },
    Static {
        data: serde_json::Value,
    },
    Computed {
        expression: String,
        dependencies: Vec<String>,
    },
    Store {
        store_id: String,
        keys: Vec<String>,
    },
    UrlParam {
        param: String,
        transform: Option<String>,
    },
    LocalStorage {
        key: String,
        default: Option<serde_json::Value>,
    },
    SessionStorage {
        key: String,
        default: Option<serde_json::Value>,
    },
    Cookie {
        name: String,
        default: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct DatabaseQuery {
    pub select: Vec<String>,
    pub r#where: Vec<WhereClause>,
    pub order_by: Vec<OrderByClause>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub group_by: Vec<String>,
    pub having: Vec<WhereClause>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct WhereClause {
    pub field: String,
    pub operator: WhereOperator,
    pub value: serde_json::Value,
    pub logical: LogicalOperator,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WhereOperator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    ILike,
    In,
    NotIn,
    Between,
    NotBetween,
    IsNull,
    IsNotNull,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogicalOperator {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct OrderByClause {
    pub field: String,
    pub direction: OrderDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct DataSourceConfig {
    pub timeout_ms: u64,
    pub retry: RetryConfig,
    pub cache: CacheConfig,
    pub debounce_ms: Option<u32>,
    pub transform_response: Option<String>,
    pub transform_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct CacheConfig {
    pub enabled: bool,
    pub ttl_ms: u64,
    pub stale_while_revalidate_ms: Option<u64>,
    pub key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PageAction {
    pub id: ActionId,
    pub name: String,
    pub action_type: ActionType,
    pub config: ActionConfig,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct ActionId(pub Uuid);

impl ActionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ActionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum ActionConfig {
    Navigate {
        path: String,
        replace: bool,
        state: Option<serde_json::Value>,
    },
    Redirect {
        url: String,
        external: bool,
    },
    Reload,
    Back,
    Forward,
    Go {
        delta: i32,
    },
    OpenModal {
        modal_id: ComponentId,
        props: IndexMap<String, serde_json::Value>,
    },
    CloseModal {
        modal_id: ComponentId,
    },
    SubmitForm {
        form_id: ComponentId,
        validate: bool,
        on_success: Vec<ActionId>,
        on_error: Vec<ActionId>,
    },
    ResetForm {
        form_id: ComponentId,
    },
    SetFormValues {
        form_id: ComponentId,
        values: IndexMap<String, serde_json::Value>,
    },
    SetFieldValue {
        form_id: ComponentId,
        field: String,
        value: serde_json::Value,
    },
    ValidateForm {
        form_id: ComponentId,
    },
    CallApi {
        data_source_id: String,
        params: IndexMap<String, String>,
        on_success: Vec<ActionId>,
        on_error: Vec<ActionId>,
    },
    ExecuteWorkflow {
        workflow_id: WorkflowId,
        variables: IndexMap<String, String>,
        on_complete: Vec<ActionId>,
        on_error: Vec<ActionId>,
    },
    SetState {
        key: String,
        value: serde_json::Value,
        scope: StateScope,
    },
    GetState {
        key: String,
        scope: StateScope,
        default: Option<serde_json::Value>,
    },
    DeleteState {
        key: String,
        scope: StateScope,
    },
    ClearState {
        scope: StateScope,
    },
    EmitEvent {
        event_name: String,
        payload: serde_json::Value,
        target: EventTarget,
    },
    SubscribeEvent {
        event_name: String,
        handler: ActionId,
    },
    UnsubscribeEvent {
        event_name: String,
    },
    CopyToClipboard {
        text: String,
        success_message: Option<String>,
    },
    Download {
        url: String,
        filename: String,
    },
    Print,
    OpenWindow {
        url: String,
        name: String,
        features: String,
    },
    CloseWindow,
    FocusElement {
        selector: String,
    },
    ScrollTo {
        selector: String,
        behavior: ScrollBehavior,
        block: ScrollAlignment,
    },
    RunScript {
        script: String,
        args: Vec<serde_json::Value>,
    },
    EvalExpression {
        expression: String,
        result_var: String,
    },
    Delay {
        ms: u32,
    },
    Batch {
        actions: Vec<ActionId>,
        parallel: bool,
    },
    Conditional {
        condition: String,
        then_actions: Vec<ActionId>,
        else_actions: Vec<ActionId>,
    },
    Loop {
        collection: String,
        item_var: String,
        index_var: Option<String>,
        body: Vec<ActionId>,
    },
    TryCatch {
        try_actions: Vec<ActionId>,
        catch_actions: Vec<ActionId>,
        finally_actions: Vec<ActionId>,
    },
    Log {
        level: LogLevel,
        message: String,
        metadata: IndexMap<String, serde_json::Value>,
    },
    ReportError {
        message: String,
        code: Option<String>,
        context: IndexMap<String, serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StateScope {
    Global,
    Page,
    Component,
    Session,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventTarget {
    Self_,
    Parent,
    Window,
    Document,
    Component(ComponentId),
    Broadcast,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollBehavior {
    Auto,
    Smooth,
    Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollAlignment {
    Start,
    Center,
    End,
    Nearest,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SeoConfig {
    pub title: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub og_title: Option<String>,
    pub og_description: Option<String>,
    pub og_image: Option<String>,
    pub og_type: Option<String>,
    pub twitter_card: Option<String>,
    pub twitter_title: Option<String>,
    pub twitter_description: Option<String>,
    pub twitter_image: Option<String>,
    pub canonical_url: Option<String>,
    pub robots: Option<String>,
    pub structured_data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct PagePermissions {
    pub view_roles: Vec<String>,
    pub edit_roles: Vec<String>,
    pub admin_roles: Vec<String>,
    pub public_access: bool,
}
