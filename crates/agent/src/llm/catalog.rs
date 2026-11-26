//! The provider catalog: every platform this build can talk to.
//!
//! One table, one adapter. Eighteen of the twenty-one platforms below speak the
//! OpenAI chat-completions dialect, so they need nothing but a base URL, an
//! optional `Authorization` header and — for a few gateways — a couple of
//! branding headers. Only Anthropic and a local Ollama use a different wire
//! format, and both already have their own adapter.
//!
//! This table is the single source of truth. `AgentConfig` reads it to decide
//! which wire format, endpoint and headers a platform needs; the IDE server
//! reads it to decide which stored key rows are usable and what to show in the
//! picker. Adding a provider is therefore one entry here, never an edit in two
//! crates.
//!
//! # Why a base URL is not always `{base}/v1`
//!
//! `OpenAiProvider` appends `/chat/completions` to the base, and adds `/v1`
//! when the base does not already end in a version segment. That rule covers
//! `https://api.groq.com/openai/v1` and `https://open.bigmodel.cn/api/paas/v4`.
//! Three platforms do not fit it — Google's OpenAI shim, GitHub Models and
//! Cloudflare — so their entry carries the *complete* chat endpoint, which the
//! same rule leaves untouched because it already ends in `/chat/completions`.

use crate::config::ProviderKind;
use crate::error::AgentError;

/// How to reach one platform.
pub struct ProviderSpec {
    /// Stable id. This is the value stored in `agent_api_keys.platform` and in
    /// `AGENT_PROVIDER`, so it must not change.
    pub platform: &'static str,
    /// What the picker shows.
    pub label: &'static str,
    /// Wire format to speak.
    pub kind: ProviderKind,
    /// Default endpoint. Empty means the caller must supply one (a custom
    /// gateway, or Cloudflare, whose path carries an account id).
    pub base_url: &'static str,
    /// True when the platform serves requests without any credential.
    pub keyless: bool,
    /// Headers the gateway requires in addition to `Authorization`.
    /// Names are lower-case because that is what `http::HeaderName` stores.
    pub headers: &'static [(&'static str, &'static str)],
    /// True when the credential embeds an account id that belongs in the path
    /// (`account_id:api_token`, as Cloudflare issues it).
    pub account_scoped: bool,
    /// Model used when the request names none.
    pub default_model: &'static str,
    /// Suggestions for the model field. Not exhaustive on purpose: a gateway
    /// ships new models faster than this table can be edited.
    pub models: &'static [&'static str],
    /// Why this platform cannot run as configured, when the entry itself says
    /// so. `None` means the entry is complete.
    pub note: Option<&'static str>,
}

/// Every platform this build knows how to reach.
pub const CATALOG: &[ProviderSpec] = &[
    ProviderSpec {
        platform: "openai",
        label: "OpenAI",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.openai.com/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "gpt-4o-mini",
        models: &["gpt-4o-mini", "gpt-4o", "gpt-4.1", "o4-mini"],
        note: None,
    },
    ProviderSpec {
        platform: "anthropic",
        label: "Anthropic",
        kind: ProviderKind::Anthropic,
        base_url: "https://api.anthropic.com",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "claude-sonnet-4-5",
        models: &["claude-sonnet-4-5", "claude-opus-4-1", "claude-haiku-4-5"],
        note: None,
    },
    ProviderSpec {
        platform: "google",
        label: "Google Gemini",
        kind: ProviderKind::OpenAi,
        // Gemini's OpenAI compatibility layer. The full endpoint, because the
        // path has a version (`v1beta`) *before* the compatibility segment, so
        // the `{base}/v1/chat/completions` rule would double it.
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "gemini-3.5-flash",
        models: &[
            "gemini-3.5-flash",
            "gemini-3-flash-preview",
            "gemini-3.1-flash-lite-preview",
        ],
        note: None,
    },
    ProviderSpec {
        platform: "groq",
        label: "Groq",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.groq.com/openai/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "llama-3.3-70b-versatile",
        models: &[
            "llama-3.3-70b-versatile",
            "openai/gpt-oss-120b",
            "meta-llama/llama-4-scout-17b-16e-instruct",
        ],
        note: None,
    },
    ProviderSpec {
        platform: "cerebras",
        label: "Cerebras",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.cerebras.ai/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "gpt-oss-120b",
        models: &["gpt-oss-120b", "zai-glm-4.7", "llama-3.3-70b"],
        note: None,
    },
    ProviderSpec {
        platform: "sambanova",
        label: "SambaNova",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.sambanova.ai/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "DeepSeek-V3.2",
        models: &["DeepSeek-V3.2", "gpt-oss-120b", "Meta-Llama-3.3-70B-Instruct"],
        note: None,
    },
    ProviderSpec {
        platform: "nvidia",
        label: "NVIDIA NIM",
        kind: ProviderKind::OpenAi,
        base_url: "https://integrate.api.nvidia.com/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "qwen/qwen3-coder-480b-a35b-instruct",
        models: &[
            "qwen/qwen3-coder-480b-a35b-instruct",
            "deepseek-ai/deepseek-v4-pro",
            "minimaxai/minimax-m2.7",
        ],
        note: None,
    },
    ProviderSpec {
        platform: "mistral",
        label: "Mistral",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.mistral.ai/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "mistral-large-latest",
        models: &[
            "mistral-large-latest",
            "mistral-medium-latest",
            "mistral-small-latest",
            "codestral-latest",
        ],
        note: None,
    },
    ProviderSpec {
        platform: "openrouter",
        label: "OpenRouter",
        kind: ProviderKind::OpenAi,
        base_url: "https://openrouter.ai/api/v1",
        keyless: false,
        // OpenRouter attributes traffic from these two headers. They are
        // optional, but a request without them lands in the "unknown app"
        // bucket and is deprioritised under load.
        headers: &[
            ("http-referer", "https://github.com/bonafoid"),
            ("x-title", "Bonafoid"),
        ],
        account_scoped: false,
        default_model: "openai/gpt-4o-mini",
        models: &[
            "openai/gpt-4o-mini",
            "qwen/qwen3-coder:free",
            "openai/gpt-oss-120b:free",
            "anthropic/claude-sonnet-4",
        ],
        note: None,
    },
    ProviderSpec {
        platform: "github",
        label: "GitHub Models",
        kind: ProviderKind::OpenAi,
        // A complete endpoint: GitHub's path is `/inference/chat/completions`
        // with no version segment, so the `{base}/v1/...` rule does not fit.
        base_url: "https://models.github.ai/inference/chat/completions",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "openai/gpt-4.1",
        models: &["openai/gpt-4.1", "openai/gpt-4o", "openai/gpt-4o-mini"],
        note: None,
    },
    ProviderSpec {
        platform: "cohere",
        label: "Cohere",
        kind: ProviderKind::OpenAi,
        // Cohere's own chat API is not OpenAI-shaped, but this compatibility
        // route is, and it is the documented migration path for OpenAI clients.
        base_url: "https://api.cohere.ai/compatibility/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "command-a-reasoning-08-2025",
        models: &[
            "command-a-reasoning-08-2025",
            "command-a-03-2025",
            "command-r-08-2024",
        ],
        note: None,
    },
    ProviderSpec {
        platform: "cloudflare",
        label: "Cloudflare Workers AI",
        kind: ProviderKind::OpenAi,
        // The account id sits between the collection and the version, so the
        // path is built in `resolve` from an `account_id:api_token` credential.
        base_url: "https://api.cloudflare.com/client/v4/accounts",
        keyless: false,
        headers: &[],
        account_scoped: true,
        default_model: "@cf/moonshotai/kimi-k2.6",
        models: &[
            "@cf/moonshotai/kimi-k2.6",
            "@cf/openai/gpt-oss-120b",
            "@cf/qwen/qwen3-30b-a3b-fp8",
        ],
        note: None,
    },
    ProviderSpec {
        platform: "zhipu",
        label: "Zhipu (GLM)",
        kind: ProviderKind::OpenAi,
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "glm-4.7-flash",
        models: &["glm-4.7-flash", "glm-4.5-flash", "glm-4-plus"],
        note: None,
    },
    ProviderSpec {
        platform: "ollama",
        label: "Ollama (local)",
        kind: ProviderKind::Ollama,
        base_url: "http://127.0.0.1:11434",
        keyless: true,
        headers: &[],
        account_scoped: false,
        default_model: "llama3.1",
        models: &["llama3.1", "qwen2.5-coder", "mistral"],
        note: None,
    },
    ProviderSpec {
        platform: "ollama-cloud",
        label: "Ollama Cloud",
        kind: ProviderKind::OpenAi,
        base_url: "https://ollama.com/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "qwen3-coder:480b",
        models: &["qwen3-coder:480b", "qwen3-coder-next", "glm-4.7"],
        note: None,
    },
    ProviderSpec {
        platform: "kilo",
        label: "Kilo Gateway",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.kilo.ai/api/gateway/v1",
        keyless: true,
        headers: &[],
        account_scoped: false,
        default_model: "nvidia/nemotron-3-super-120b-a12b:free",
        models: &["nvidia/nemotron-3-super-120b-a12b:free"],
        note: None,
    },
    ProviderSpec {
        platform: "pollinations",
        label: "Pollinations",
        kind: ProviderKind::OpenAi,
        base_url: "https://text.pollinations.ai/openai/v1",
        keyless: true,
        headers: &[],
        account_scoped: false,
        default_model: "openai",
        models: &["openai", "mistral"],
        note: None,
    },
    ProviderSpec {
        platform: "llm7",
        label: "LLM7",
        kind: ProviderKind::OpenAi,
        base_url: "https://api.llm7.io/v1",
        keyless: true,
        headers: &[],
        account_scoped: false,
        default_model: "codestral-latest",
        models: &["codestral-latest", "gpt-4o-mini"],
        note: None,
    },
    ProviderSpec {
        platform: "huggingface",
        label: "Hugging Face",
        kind: ProviderKind::OpenAi,
        base_url: "https://router.huggingface.co/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "Qwen/Qwen3-Coder-Next",
        models: &[
            "Qwen/Qwen3-Coder-Next",
            "moonshotai/Kimi-K2.6",
            "deepseek-ai/DeepSeek-V4-Flash",
        ],
        note: None,
    },
    ProviderSpec {
        platform: "opencode",
        label: "OpenCode Zen",
        kind: ProviderKind::OpenAi,
        base_url: "https://opencode.ai/zen/v1",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "deepseek-v4-flash-free",
        models: &["deepseek-v4-flash-free", "nemotron-3-super-free"],
        note: None,
    },
    ProviderSpec {
        platform: "custom",
        label: "Custom (OpenAI-compatible)",
        kind: ProviderKind::OpenAi,
        // No default: the operator's own gateway, so the row must carry it.
        base_url: "",
        keyless: false,
        headers: &[],
        account_scoped: false,
        default_model: "",
        models: &[],
        note: Some("needs a base URL on its key row (any OpenAI-compatible gateway)"),
    },
];

/// The entry for a platform id, case-insensitively.
pub fn spec(platform: &str) -> Option<&'static ProviderSpec> {
    CATALOG
        .iter()
        .find(|spec| spec.platform.eq_ignore_ascii_case(platform))
}

/// A resolved endpoint: what to POST to, with which credential and headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub base_url: String,
    /// `None` for a keyless platform, or for a platform whose credential is
    /// optional and was not supplied.
    pub api_key: Option<String>,
    pub headers: Vec<(String, String)>,
}

/// Turn a spec plus a stored credential into a concrete endpoint.
///
/// `base_url_override` is what an operator typed on the key row; it wins over
/// the catalog default, which is what makes `custom` and a self-hosted gateway
/// work without a code change.
pub fn resolve(
    spec: &ProviderSpec,
    api_key: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<Endpoint, AgentError> {
    let override_url = base_url_override.map(str::trim).filter(|url| !url.is_empty());
    let base = match override_url {
        Some(url) => url.to_string(),
        None if spec.base_url.is_empty() => {
            return Err(AgentError::MissingBaseUrl(spec.platform.to_string()))
        }
        None => spec.base_url.to_string(),
    };
    // A trailing slash would produce `//v1/chat/completions`, which some
    // gateways answer with a 404 rather than a redirect.
    let base = base.trim_end_matches('/').to_string();

    let key = api_key.map(str::trim).filter(|key| !key.is_empty());

    // Cloudflare issues `account_id:api_token` because the account id is part
    // of the path, not of the header.
    if spec.account_scoped {
        let credential = key.ok_or_else(|| AgentError::MissingKey(spec.platform.to_string()))?;
        let (account, token) = credential.split_once(':').ok_or_else(|| {
            AgentError::BadCredential(format!(
                "{} expects `account_id:api_token`, not a bare token",
                spec.platform
            ))
        })?;
        if account.trim().is_empty() || token.trim().is_empty() {
            return Err(AgentError::BadCredential(format!(
                "{} expects `account_id:api_token` with both parts filled in",
                spec.platform
            )));
        }
        return Ok(Endpoint {
            base_url: format!("{base}/{}/ai/v1", account.trim()),
            api_key: Some(token.trim().to_string()),
            headers: static_headers(spec),
        });
    }

    Ok(Endpoint {
        // A keyless platform ignores a supplied key rather than sending a
        // credential nobody asked for.
        api_key: if spec.keyless { None } else { key.map(str::to_string) },
        base_url: base,
        headers: static_headers(spec),
    })
}

fn static_headers(spec: &ProviderSpec) -> Vec<(String, String)> {
    spec.headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// Whether a credential is required before the platform can be used.
pub fn requires_key(platform: &str) -> bool {
    spec(platform).map(|spec| !spec.keyless).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_unique_and_has_a_label() {
        let mut seen: Vec<&str> = Vec::new();
        for spec in CATALOG {
            assert!(
                !seen.contains(&spec.platform),
                "duplicate platform {}",
                spec.platform
            );
            seen.push(spec.platform);
            assert!(!spec.label.is_empty(), "{} has no label", spec.platform);
            // Only a platform that requires a caller-supplied base URL may have
            // no default endpoint, and only `custom` has no default model.
            if spec.base_url.is_empty() {
                assert!(
                    spec.note.is_some(),
                    "{} has no base URL and no note",
                    spec.platform
                );
            } else {
                assert!(
                    !spec.default_model.is_empty(),
                    "{} has no default model",
                    spec.platform
                );
            }
        }
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert_eq!(spec("GROQ").map(|s| s.platform), Some("groq"));
        assert!(spec("nope").is_none());
    }

    #[test]
    fn a_keyless_platform_sends_no_credential() {
        let spec = spec("kilo").unwrap();
        let endpoint = resolve(spec, Some("sk-should-be-ignored"), None).unwrap();
        assert_eq!(endpoint.api_key, None);
        assert_eq!(endpoint.base_url, "https://api.kilo.ai/api/gateway/v1");
    }

    #[test]
    fn a_custom_platform_needs_a_base_url() {
        let spec = spec("custom").unwrap();
        assert!(matches!(
            resolve(spec, Some("sk"), None),
            Err(AgentError::MissingBaseUrl(_))
        ));
        let endpoint = resolve(spec, Some("sk"), Some("https://acme.test/v1/")).unwrap();
        assert_eq!(endpoint.base_url, "https://acme.test/v1");
        assert_eq!(endpoint.api_key.as_deref(), Some("sk"));
    }

    #[test]
    fn cloudflare_splits_the_account_id_into_the_path() {
        let spec = spec("cloudflare").unwrap();
        let endpoint = resolve(spec, Some("abc123:token-xyz"), None).unwrap();
        assert_eq!(
            endpoint.base_url,
            "https://api.cloudflare.com/client/v4/accounts/abc123/ai/v1"
        );
        assert_eq!(endpoint.api_key.as_deref(), Some("token-xyz"));
        assert!(matches!(
            resolve(spec, Some("bare-token"), None),
            Err(AgentError::BadCredential(_))
        ));
    }

    #[test]
    fn openrouter_carries_its_attribution_headers() {
        let spec = spec("openrouter").unwrap();
        let endpoint = resolve(spec, Some("sk-or"), None).unwrap();
        let names: Vec<&str> = endpoint.headers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["http-referer", "x-title"]);
    }

    #[test]
    fn an_override_wins_over_the_catalog_default() {
        let spec = spec("groq").unwrap();
        let endpoint = resolve(spec, Some("sk"), Some("https://proxy.test/v1")).unwrap();
        assert_eq!(endpoint.base_url, "https://proxy.test/v1");
    }
}
