use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Theme {
    pub id: ThemeId,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub tokens: DesignTokens,
    pub components: IndexMap<String, ComponentTheme>,
    pub global_styles: String,
    pub dark_mode: DarkModeConfig,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Theme {
    /// Create a new theme.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: ThemeId::new(),
            name: name.into(),
            description: None,
            version: "1.0.0".to_string(),
            tokens: DesignTokens::default(),
            components: IndexMap::new(),
            global_styles: String::new(),
            dark_mode: DarkModeConfig::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct ThemeId(pub Uuid);

impl ThemeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ThemeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ThemeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct DesignTokens {
    pub colors: ColorTokens,
    pub typography: TypographyTokens,
    pub spacing: SpacingTokens,
    pub sizing: SizingTokens,
    pub borders: BorderTokens,
    pub shadows: ShadowTokens,
    pub transitions: TransitionTokens,
    pub z_indices: ZIndexTokens,
    pub breakpoints: BreakpointTokens,
    pub custom: IndexMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ColorTokens {
    pub primary: ColorScale,
    pub secondary: ColorScale,
    pub success: ColorScale,
    pub warning: ColorScale,
    pub error: ColorScale,
    pub info: ColorScale,
    pub neutral: ColorScale,
    pub background: BackgroundColors,
    pub text: TextColors,
    pub border: BorderColors,
    pub action: ActionColors,
    pub custom: IndexMap<String, ColorScale>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ColorScale {
    pub base: String,
    pub scale: IndexMap<u16, String>,
    pub contrast_text: Option<String>,
}

impl ColorScale {
    pub fn get(&self, shade: u16) -> Option<&String> {
        self.scale.get(&shade).or(Some(&self.base))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct BackgroundColors {
    pub primary: String,
    pub secondary: String,
    pub tertiary: String,
    pub inverse: String,
    pub hover: String,
    pub active: String,
    pub disabled: String,
    pub overlay: String,
    pub modal: String,
    pub popover: String,
    pub tooltip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TextColors {
    pub primary: String,
    pub secondary: String,
    pub tertiary: String,
    pub inverse: String,
    pub disabled: String,
    pub link: String,
    pub link_hover: String,
    pub placeholder: String,
    pub on_primary: String,
    pub on_secondary: String,
    pub on_error: String,
    pub on_success: String,
    pub on_warning: String,
    pub on_info: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct BorderColors {
    pub primary: String,
    pub secondary: String,
    pub tertiary: String,
    pub focus: String,
    pub error: String,
    pub success: String,
    pub warning: String,
    pub divider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ActionColors {
    pub primary: ActionColorSet,
    pub secondary: ActionColorSet,
    pub tertiary: ActionColorSet,
    pub ghost: ActionColorSet,
    pub danger: ActionColorSet,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ActionColorSet {
    pub background: String,
    pub background_hover: String,
    pub background_active: String,
    pub text: String,
    pub border: String,
    pub disabled_background: String,
    pub disabled_text: String,
    pub disabled_border: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TypographyTokens {
    pub font_families: FontFamilies,
    pub font_sizes: FontSizes,
    pub font_weights: FontWeights,
    pub line_heights: LineHeights,
    pub letter_spacings: LetterSpacings,
    pub text_styles: IndexMap<String, TextStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FontFamilies {
    pub sans: String,
    pub serif: String,
    pub mono: String,
    pub display: Option<String>,
    pub custom: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FontSizes {
    pub xs: String,
    pub sm: String,
    pub base: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
    pub xxxl: String,
    pub display_xs: String,
    pub display_sm: String,
    pub display_base: String,
    pub display_lg: String,
    pub display_xl: String,
    pub display_xxl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FontWeights {
    pub thin: u16,
    pub extra_light: u16,
    pub light: u16,
    pub normal: u16,
    pub medium: u16,
    pub semi_bold: u16,
    pub bold: u16,
    pub extra_bold: u16,
    pub black: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct LineHeights {
    pub none: String,
    pub tight: String,
    pub snug: String,
    pub normal: String,
    pub relaxed: String,
    pub loose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct LetterSpacings {
    pub tighter: String,
    pub tight: String,
    pub normal: String,
    pub wide: String,
    pub wider: String,
    pub widest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TextStyle {
    pub font_family: Option<String>,
    pub font_size: Option<String>,
    pub font_weight: Option<u16>,
    pub line_height: Option<String>,
    pub letter_spacing: Option<String>,
    pub color: Option<String>,
    pub text_transform: Option<String>,
    pub text_decoration: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SpacingTokens {
    pub base: String,
    pub scale: IndexMap<u8, String>,
    pub component: ComponentSpacing,
    pub layout: LayoutSpacing,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ComponentSpacing {
    pub xs: String,
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct LayoutSpacing {
    pub container_padding: String,
    pub section_gap: String,
    pub component_gap: String,
    pub inline_gap: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SizingTokens {
    pub widths: WidthScale,
    pub heights: HeightScale,
    pub max_widths: MaxWidthScale,
    pub container: ContainerSizes,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct WidthScale {
    pub auto: String,
    pub px: String,
    pub full: String,
    pub screen: String,
    pub min: String,
    pub max: String,
    pub scale: IndexMap<u8, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct HeightScale {
    pub auto: String,
    pub px: String,
    pub full: String,
    pub screen: String,
    pub min: String,
    pub max: String,
    pub scale: IndexMap<u8, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct MaxWidthScale {
    pub none: String,
    pub xs: String,
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
    pub full: String,
    pub screen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ContainerSizes {
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
    pub full: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct BorderTokens {
    pub widths: BorderWidths,
    pub radii: BorderRadii,
    pub styles: BorderStyles,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct BorderWidths {
    pub none: String,
    pub thin: String,
    pub base: String,
    pub thick: String,
    pub thicker: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct BorderRadii {
    pub none: String,
    pub xs: String,
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
    pub full: String,
    pub circle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct BorderStyles {
    pub solid: String,
    pub dashed: String,
    pub dotted: String,
    pub double: String,
    pub groove: String,
    pub ridge: String,
    pub inset: String,
    pub outset: String,
    pub none: String,
    pub hidden: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ShadowTokens {
    pub none: String,
    pub xs: String,
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
    pub inner: String,
    pub outline: String,
    pub focus: String,
    pub elevated: IndexMap<u8, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TransitionTokens {
    pub durations: TransitionDurations,
    pub easings: TransitionEasings,
    pub properties: TransitionProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TransitionDurations {
    pub instant: String,
    pub fast: String,
    pub normal: String,
    pub slow: String,
    pub slower: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TransitionEasings {
    pub linear: String,
    pub ease_in: String,
    pub ease_out: String,
    pub ease_in_out: String,
    pub ease_out_back: String,
    pub ease_in_back: String,
    pub ease_out_circ: String,
    pub ease_in_circ: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct TransitionProperties {
    pub all: String,
    pub colors: String,
    pub opacity: String,
    pub transform: String,
    pub box_shadow: String,
    pub border: String,
    pub background: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ZIndexTokens {
    pub auto: String,
    pub base: String,
    pub dropdown: String,
    pub sticky: String,
    pub fixed: String,
    pub modal_backdrop: String,
    pub modal: String,
    pub popover: String,
    pub tooltip: String,
    pub toast: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct BreakpointTokens {
    pub xs: String,
    pub sm: String,
    pub md: String,
    pub lg: String,
    pub xl: String,
    pub xxl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ComponentTheme {
    pub base: String,
    pub variants: IndexMap<String, ComponentVariant>,
    pub sizes: IndexMap<String, ComponentSize>,
    pub states: ComponentStates,
    pub slots: IndexMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ComponentVariant {
    pub class: String,
    pub props: IndexMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ComponentSize {
    pub class: String,
    pub props: IndexMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ComponentStates {
    pub hover: Option<String>,
    pub active: Option<String>,
    pub focus: Option<String>,
    pub focus_visible: Option<String>,
    pub disabled: Option<String>,
    pub loading: Option<String>,
    pub error: Option<String>,
    pub success: Option<String>,
    pub warning: Option<String>,
    pub read_only: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct DarkModeConfig {
    pub enabled: bool,
    pub strategy: DarkModeStrategy,
    pub selector: String,
    pub tokens: Option<DesignTokens>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DarkModeStrategy {
    #[default]
    Class,
    MediaQuery,
    DataAttribute,
    Manual,
}