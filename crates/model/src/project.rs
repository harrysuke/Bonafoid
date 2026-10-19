use crate::{Entity, Form, Page, Theme, Workflow};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub version: String,
    pub icon: Option<String>,
    pub settings: ProjectSettings,
    pub forms: IndexMap<crate::form::FormId, Form>,
    pub entities: IndexMap<crate::entity::EntityId, Entity>,
    pub workflows: IndexMap<crate::workflow::WorkflowId, Workflow>,
    pub pages: IndexMap<crate::page::PageId, Page>,
    pub themes: IndexMap<crate::theme::ThemeId, Theme>,
    pub active_theme_id: Option<crate::theme::ThemeId>,
    pub plugins: Vec<PluginConfig>,
    pub environments: IndexMap<String, EnvironmentConfig>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct ProjectId(pub Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ProjectSettings {
    pub default_locale: String,
    pub supported_locales: Vec<String>,
    pub timezone: String,
    pub date_format: String,
    pub time_format: String,
    pub number_format: NumberFormat,
    pub currency: CurrencyConfig,
    pub authentication: AuthConfig,
    pub api: ApiConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub frontend: FrontendConfig,
    pub storage: StorageConfig,
    pub email: EmailConfig,
    pub features: FeatureFlags,
    pub custom: IndexMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct NumberFormat {
    pub locale: String,
    pub decimal_separator: String,
    pub grouping_separator: String,
    pub min_fraction_digits: u8,
    pub max_fraction_digits: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct CurrencyConfig {
    pub code: String,
    pub symbol: String,
    pub display: CurrencyDisplay,
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum CurrencyDisplay {
    #[default]
    Symbol,
    Code,
    Name,
    NarrowSymbol,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct AuthConfig {
    pub enabled: bool,
    pub providers: Vec<AuthProvider>,
    pub session: SessionConfig,
    pub password_policy: PasswordPolicy,
    pub mfa: MfaConfig,
    pub social: SocialAuthConfig,
    pub sso: SsoConfig,
    pub public_routes: Vec<String>,
    pub protected_routes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct AuthProvider {
    pub id: String,
    pub name: String,
    pub provider_type: AuthProviderType,
    pub config: IndexMap<String, serde_json::Value>,
    pub enabled: bool,
    pub button_text: String,
    pub button_icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthProviderType {
    EmailPassword,
    PhoneOtp,
    MagicLink,
    #[serde(rename = "oauth2", alias = "o_auth2")]
    OAuth2,
    #[serde(rename = "oidc", alias = "o_i_d_c")]
    OIDC,
    #[serde(rename = "saml", alias = "s_a_m_l")]
    SAML,
    #[serde(rename = "ldap", alias = "l_d_a_p")]
    LDAP,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SessionConfig {
    pub timeout_minutes: u32,
    pub rolling: bool,
    pub secure_cookies: bool,
    pub same_site: SameSitePolicy,
    pub domain: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SameSitePolicy {
    Strict,
    #[default]
    Lax,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct PasswordPolicy {
    pub min_length: u8,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_numbers: bool,
    pub require_symbols: bool,
    pub prevent_reuse: u8,
    pub max_age_days: Option<u32>,
    pub lockout_threshold: u8,
    pub lockout_duration_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct MfaConfig {
    pub enabled: bool,
    pub required: bool,
    pub methods: Vec<MfaMethod>,
    pub backup_codes: bool,
    pub grace_period_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MfaMethod {
    Totp,
    Sms,
    Email,
    WebAuthn,
    BackupCodes,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SocialAuthConfig {
    pub google: Option<OAuth2Config>,
    pub github: Option<OAuth2Config>,
    pub microsoft: Option<OAuth2Config>,
    pub facebook: Option<OAuth2Config>,
    pub twitter: Option<OAuth2Config>,
    pub linkedin: Option<OAuth2Config>,
    pub apple: Option<OAuth2Config>,
    pub custom: IndexMap<String, OAuth2Config>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct OAuth2Config {
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub user_info_url: String,
    pub scope: Vec<String>,
    pub redirect_url: String,
    pub pkce: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SsoConfig {
    pub enabled: bool,
    pub provider: SsoProvider,
    pub config: IndexMap<String, serde_json::Value>,
    pub attribute_mapping: AttributeMapping,
    pub just_in_time_provisioning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SsoProvider {
    #[default]
    #[serde(rename = "saml", alias = "s_a_m_l")]
    SAML,
    #[serde(rename = "oidc", alias = "o_i_d_c")]
    OIDC,
    #[serde(rename = "ldap", alias = "l_d_a_p")]
    LDAP,
    ActiveDirectory,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct AttributeMapping {
    pub user_id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
    pub groups: String,
    pub roles: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ApiConfig {
    pub base_path: String,
    pub versioning: ApiVersioning,
    pub cors: CorsConfig,
    pub rate_limiting: RateLimitConfig,
    pub documentation: ApiDocConfig,
    pub webhooks: WebhookConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ApiVersioning {
    Header,
    Query,
    #[default]
    Path,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct CorsConfig {
    pub enabled: bool,
    pub allow_origins: Vec<String>,
    pub allow_methods: Vec<String>,
    pub allow_headers: Vec<String>,
    pub expose_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub burst: u32,
    pub key_generator: RateLimitKeyGenerator,
    pub skip_successful: bool,
    pub skip_failed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum RateLimitKeyGenerator {
    #[default]
    Ip,
    UserId,
    ApiKey,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ApiDocConfig {
    pub enabled: bool,
    pub path: String,
    pub title: String,
    pub description: String,
    pub version: String,
    pub contact: ContactInfo,
    pub license: LicenseInfo,
    pub servers: Vec<ServerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ContactInfo {
    pub name: String,
    pub email: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct LicenseInfo {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ServerInfo {
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct WebhookConfig {
    pub enabled: bool,
    pub secret_header: String,
    pub timeout_seconds: u32,
    pub retry: RetryConfig,
    pub allowed_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_interval_ms: u64,
    pub max_interval_ms: u64,
    pub multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct DatabaseConfig {
    pub driver: DatabaseDriver,
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_seconds: u64,
    pub idle_timeout_seconds: u64,
    pub max_lifetime_seconds: u64,
    pub schema: Option<String>,
    pub migrations: MigrationConfig,
    pub ssl_mode: SslMode,
    pub pool_options: PoolOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DatabaseDriver {
    #[default]
    Postgres,
    #[serde(rename = "mysql", alias = "my_s_q_l")]
    MySQL,
    #[serde(rename = "sqlite", alias = "s_q_lite")]
    SQLite,
    SqlServer,
    Oracle,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SslMode {
    Disable,
    Allow,
    #[default]
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct MigrationConfig {
    pub enabled: bool,
    pub table: String,
    pub auto_apply: bool,
    pub lock_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct PoolOptions {
    pub test_before_acquire: bool,
    pub acquire_timeout_seconds: u64,
    pub acquire_retry_attempts: u32,
    pub acquire_retry_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FrontendConfig {
    pub framework: FrontendFramework,
    pub bundler: Bundler,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FrontendFramework {
    #[default]
    Leptos,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Bundler {
    #[default]
    Trunk,
    Vite,
    Webpack,
    Rollup,
    Parcel,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct StorageConfig {
    pub driver: StorageDriver,
    pub local: LocalStorageConfig,
    pub s3: S3Config,
    pub gcs: GcsConfig,
    pub azure: AzureBlobConfig,
    pub cdn_url: Option<String>,
    pub max_file_size_mb: u64,
    pub allowed_mime_types: Vec<String>,
    pub quarantine_suspicious: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum StorageDriver {
    #[default]
    Local,
    S3,
    #[serde(rename = "gcs", alias = "g_c_s")]
    GCS,
    AzureBlob,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct LocalStorageConfig {
    pub root_path: String,
    pub serve_via_app: bool,
    pub url_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub force_path_style: bool,
    pub acl: String,
    pub server_side_encryption: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct GcsConfig {
    pub bucket: String,
    pub project_id: String,
    pub credentials_json: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct AzureBlobConfig {
    pub account_name: String,
    pub account_key: String,
    pub container: String,
    pub endpoint_suffix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EmailConfig {
    pub driver: EmailDriver,
    pub from: EmailAddress,
    pub reply_to: Option<EmailAddress>,
    pub smtp: SmtpConfig,
    pub sendgrid: SendGridConfig,
    pub mailgun: MailgunConfig,
    pub ses: SesConfig,
    pub templates: IndexMap<String, EmailTemplate>,
    pub queue: EmailQueueConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum EmailDriver {
    Smtp,
    SendGrid,
    Mailgun,
    #[serde(rename = "ses", alias = "s_e_s")]
    SES,
    #[default]
    Console,
    Null,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EmailAddress {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub encryption: SmtpEncryption,
    pub auth_type: SmtpAuthType,
    pub timeout_seconds: u32,
    pub pool_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SmtpEncryption {
    None,
    #[default]
    #[serde(rename = "start_tls", alias = "start_t_l_s")]
    StartTls,
    #[serde(rename = "ssl_tls", alias = "s_s_l__t_l_s")]
    SslTls,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SmtpAuthType {
    None,
    #[default]
    Plain,
    Login,
    #[serde(rename = "cram_md5", alias = "cram_m_d5")]
    CramMD5,
    #[serde(rename = "xoauth2", alias = "x_o_auth2")]
    XOAuth2,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SendGridConfig {
    pub api_key: String,
    pub sandbox_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct MailgunConfig {
    pub api_key: String,
    pub domain: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct SesConfig {
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub configuration_set: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EmailTemplate {
    pub subject: String,
    pub html: String,
    pub text: Option<String>,
    pub variables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EmailQueueConfig {
    pub enabled: bool,
    pub driver: QueueDriver,
    pub redis: RedisConfig,
    pub database: DatabaseQueueConfig,
    pub concurrency: u32,
    pub max_attempts: u32,
    pub backoff_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum QueueDriver {
    Redis,
    Database,
    #[default]
    Sync,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct RedisConfig {
    pub url: String,
    pub key_prefix: String,
    pub db: u8,
    pub pool_size: u32,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct DatabaseQueueConfig {
    pub table: String,
    pub failed_table: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct FeatureFlags {
    pub auth: bool,
    pub api: bool,
    pub workflows: bool,
    pub realtime: bool,
    pub files: bool,
    pub email: bool,
    pub webhooks: bool,
    pub scheduler: bool,
    pub audit_log: bool,
    pub soft_delete: bool,
    pub multi_tenant: bool,
    pub localization: bool,
    pub theming: bool,
    pub plugins: bool,
    pub marketplace: bool,
    pub ai_assist: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct EnvironmentConfig {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub variables: IndexMap<String, String>,
    pub secrets: Vec<String>,
    pub database: Option<DatabaseConfig>,
    pub storage: Option<StorageConfig>,
    pub email: Option<EmailConfig>,
    pub features: FeatureFlags,
    pub domain: Option<String>,
    pub ssl: bool,
    pub replicas: u32,
    pub resources: ResourceLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct ResourceLimits {
    pub cpu_cores: Option<f32>,
    pub memory_mb: Option<u32>,
    pub storage_gb: Option<u32>,
    pub bandwidth_gb: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct PluginConfig {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub license: Option<String>,
    pub enabled: bool,
    pub config: IndexMap<String, serde_json::Value>,
    pub permissions: Vec<PluginPermission>,
    pub entry_points: Vec<PluginEntryPoint>,
    pub dependencies: Vec<PluginDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    ReadForms,
    WriteForms,
    ReadEntities,
    WriteEntities,
    ReadWorkflows,
    WriteWorkflows,
    ReadPages,
    WritePages,
    ReadThemes,
    WriteThemes,
    ExecuteCode,
    AccessDatabase,
    AccessFileSystem,
    AccessNetwork,
    AccessAuth,
    AccessEmail,
    AccessScheduler,
    ManagePlugins,
    ManageUsers,
    ViewAuditLog,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PluginEntryPoint {
    pub name: String,
    pub r#type: PluginEntryPointType,
    pub path: String,
    pub exports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginEntryPointType {
    Component,
    Action,
    DataSource,
    FieldType,
    WorkflowNode,
    AuthProvider,
    StorageDriver,
    EmailDriver,
    Hook,
    Middleware,
    Command,
    Page,
    Layout,
    Util,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PluginDependency {
    pub id: String,
    pub version: String,
    pub optional: bool,
}

impl Project {
    pub fn new(name: &str) -> Self {
        let slug = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        let now = Utc::now();
        Self {
            id: ProjectId::new(),
            name: name.to_string(),
            slug,
            description: None,
            version: "0.1.0".to_string(),
            icon: None,
            settings: ProjectSettings::default(),
            forms: IndexMap::new(),
            entities: IndexMap::new(),
            workflows: IndexMap::new(),
            pages: IndexMap::new(),
            themes: IndexMap::new(),
            active_theme_id: None,
            plugins: Vec::new(),
            environments: IndexMap::new(),
            created_at: now,
            updated_at: now,
            published_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// Wire values of every enum whose Rust variants end in an acronym.
    ///
    /// `#[serde(rename_all = "snake_case")` splits a trailing acronym into one
    /// snake_case letter per capital: `SAML` becomes `s_a_m_l`, `OIDC` becomes
    /// `o_i_d_c`. Those strings reached the database and the TypeScript types
    /// before this test existed, so the renames are pinned here.
    #[test]
    fn acronym_enums_serialize_readable_values() {
        let cases: Vec<(Value, Value, &str)> = vec![
            (json!(SsoProvider::SAML), json!("saml"), "SsoProvider::SAML"),
            (json!(SsoProvider::OIDC), json!("oidc"), "SsoProvider::OIDC"),
            (json!(SsoProvider::LDAP), json!("ldap"), "SsoProvider::LDAP"),
            (
                json!(AuthProviderType::OAuth2),
                json!("oauth2"),
                "AuthProviderType::OAuth2",
            ),
            (
                json!(AuthProviderType::OIDC),
                json!("oidc"),
                "AuthProviderType::OIDC",
            ),
            (
                json!(AuthProviderType::SAML),
                json!("saml"),
                "AuthProviderType::SAML",
            ),
            (
                json!(AuthProviderType::LDAP),
                json!("ldap"),
                "AuthProviderType::LDAP",
            ),
            (
                json!(StorageDriver::GCS),
                json!("gcs"),
                "StorageDriver::GCS",
            ),
            (json!(EmailDriver::SES), json!("ses"), "EmailDriver::SES"),
            (
                json!(SmtpAuthType::CramMD5),
                json!("cram_md5"),
                "SmtpAuthType::CramMD5",
            ),
            (
                json!(SmtpAuthType::XOAuth2),
                json!("xoauth2"),
                "SmtpAuthType::XOAuth2",
            ),
            (
                json!(SmtpEncryption::StartTls),
                json!("start_tls"),
                "SmtpEncryption::StartTls",
            ),
            (
                json!(SmtpEncryption::SslTls),
                json!("ssl_tls"),
                "SmtpEncryption::SslTls",
            ),
            (
                json!(DatabaseDriver::SQLite),
                json!("sqlite"),
                "DatabaseDriver::SQLite",
            ),
        ];

        let mut wrong: Vec<String> = Vec::new();
        for (value, expected, label) in cases {
            let actual = serde_json::to_value(&value).expect("serialize");
            if actual != expected {
                wrong.push(format!("{label}: got {actual}, want {expected}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// Projects saved before the renames must still load.
    ///
    /// Every rename is paired with an `alias`, so a project.json written by an
    /// older build still opens instead of failing to deserialize.
    #[test]
    fn legacy_mangled_values_still_deserialize() {
        /// Assert that the named type still accepts each legacy string.
        macro_rules! accepts {
            ($ty:ty, $name:literal, $($value:literal),+ $(,)?) => {
                for value in [$($value),+] {
                    assert!(
                        serde_json::from_value::<$ty>(Value::String(value.to_string())).is_ok(),
                        "{} must still accept {}",
                        $name,
                        value
                    );
                }
            };
        }

        accepts!(SsoProvider, "SsoProvider", "s_a_m_l", "o_i_d_c", "l_d_a_p");
        accepts!(
            AuthProviderType,
            "AuthProviderType",
            "o_auth2",
            "o_i_d_c",
            "s_a_m_l",
            "l_d_a_p"
        );
        accepts!(StorageDriver, "StorageDriver", "g_c_s");
        accepts!(EmailDriver, "EmailDriver", "s_e_s");
        accepts!(SmtpAuthType, "SmtpAuthType", "x_o_auth2", "cram_m_d5");
        accepts!(DatabaseDriver, "DatabaseDriver", "s_q_lite", "my_s_q_l");
        accepts!(
            SmtpEncryption,
            "SmtpEncryption",
            "start_t_l_s",
            "s_s_l__t_l_s"
        );
    }
}
