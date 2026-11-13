//! Deployment and integration service for the No-Code IDE.
//!
//! Handles GitHub and Docker Hub connections, credential encryption,
//! generated code deployment, and CI/CD orchestration.
//!
//! # Secret handling
//!
//! A GitHub personal access token is a bearer credential: whoever holds it
//! holds the account's rights. It is encrypted at rest by [`vault`], opened
//! only for the duration of a request, and never returned by any public API.
//! Every error message and log line that could carry one is passed through
//! [`redact`] first. See the module docs in [`git`] for how the token reaches
//! `git` without ever appearing in `argv` or `.git/config`.

pub mod compose;
pub mod deploy;
pub mod git;
pub mod github;
pub mod vault;

pub use compose::{
    patch_workflow_branch, verify_generated, ComposeConfig, RegistryConfig, RegistryConfigResponse,
};
pub use deploy::{
    classify_run, safe_error, DeployConfig, DeployError, DeployOptions, DeployService, Deployment,
    DeploymentStatus,
};
pub use git::{GitError, GitService};
pub use github::{GitHubClient, GitHubConfig, GitHubConfigResponse, GitHubError, WorkflowRun};
pub use vault::{CredentialVault, SealedCredential, VaultError};

/// What a masked secret looks like in logs and errors.
pub const REDACTED: &str = "***REDACTED***";

/// Prefixes that identify a GitHub token or a place one is being supplied.
const SECRET_MARKERS: &[&str] = &[
    "github_pat_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "x-access-token:",
    "Bearer ",
];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '~' | '+' | '=' | '/')
}

/// Mask anything that looks like a GitHub token or an inline credential.
///
/// This is deliberately broad: it is applied to *outbound* strings (errors,
/// log lines, response bodies), so over-masking costs nothing while
/// under-masking leaks a live credential. It is NOT a substitute for never
/// putting a secret in a string in the first place — see [`git`].
pub fn redact(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    loop {
        let mut earliest: Option<(usize, &str)> = None;
        for marker in SECRET_MARKERS {
            if let Some(pos) = rest.find(marker) {
                if earliest.is_none_or(|(best, _)| pos < best) {
                    earliest = Some((pos, marker));
                }
            }
        }

        match earliest {
            None => {
                out.push_str(rest);
                return out;
            }
            Some((pos, marker)) => {
                out.push_str(&rest[..pos]);
                out.push_str(REDACTED);
                let after = &rest[pos + marker.len()..];
                let skip = after
                    .find(|c: char| !is_token_char(c))
                    .unwrap_or(after.len());
                rest = &after[skip..];
            }
        }
    }
}

/// A branch name is safe to hand to `git` only when it cannot be an option
/// and cannot inject a refspec or a shell metacharacter.
pub fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 200
        && !branch.starts_with('-')
        && !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.contains("..")
        && !branch.contains('@')
        && !branch.contains(' ')
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

/// Owner / repository / image names: GitHub- and Docker-safe identifier.
pub fn valid_repo_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A public API base URL is baked into the generated frontend at build time,
/// then interpolated into YAML and Rust templates. It must be an absolute,
/// plain http(s) URL: no userinfo (which could smuggle a credential), no
/// whitespace/control characters, and no shell, template or YAML
/// metacharacters that could change the meaning of the generated file.
pub fn valid_api_base(value: &str) -> bool {
    if value.is_empty() || value.len() > 300 {
        return false;
    }
    let Some(rest) = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
    else {
        return false;
    };
    let forbidden = |c: char| {
        c.is_control()
            || c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '`'
                    | '\\'
                    | '$'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '|'
                    | ';'
                    | '&'
                    | '@'
                    | '#'
                    | '?'
                    | '^'
                    | '!'
                    | '('
                    | ')'
            )
    };
    if rest.chars().any(forbidden) {
        return false;
    }
    let authority = rest.split('/').next().unwrap_or_default();
    !authority.is_empty() && !authority.starts_with(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_classic_and_fine_grained_tokens() {
        assert_eq!(redact("token ghp_abcDEF123xyz"), "token ***REDACTED***");
        assert_eq!(redact("github_pat_11ABCDEFG_longvalue"), "***REDACTED***");
        assert_eq!(
            redact("Authorization: Bearer ghp_secret99 rest"),
            "Authorization: ***REDACTED*** rest"
        );
    }

    #[test]
    fn redacts_token_embedded_in_remote_url() {
        let leaked = "https://x-access-token:ghp_supersecret@github.com/acme/app.git";
        let safe = redact(leaked);
        assert!(safe.contains("github.com/acme/app.git"));
        assert!(!safe.contains("supersecret"));
    }

    #[test]
    fn redact_leaves_ordinary_text_alone() {
        assert_eq!(redact("push to main failed"), "push to main failed");
    }

    #[test]
    fn branch_validation_rejects_injection() {
        assert!(valid_branch("main"));
        assert!(valid_branch("feature/deploy-2"));
        assert!(!valid_branch("-u"));
        assert!(!valid_branch("main;rm -rf /"));
        assert!(!valid_branch("a..b"));
        assert!(!valid_branch("refs/heads/main@{1}"));
        assert!(!valid_branch(""));
    }

    #[test]
    fn repo_component_validation() {
        assert!(valid_repo_component("acme-inc"));
        assert!(valid_repo_component("app.backend"));
        assert!(!valid_repo_component("acme/app"));
        assert!(!valid_repo_component("a b"));
        assert!(!valid_repo_component(""));
    }

    #[test]
    fn api_base_accepts_only_plain_absolute_http_urls() {
        assert!(valid_api_base("http://localhost:8081"));
        assert!(valid_api_base("https://api.example.com"));
        assert!(valid_api_base("https://api.example.com/v1"));
        assert!(!valid_api_base(""));
        assert!(!valid_api_base("api.example.com"));
        assert!(!valid_api_base("javascript:alert(1)"));
        assert!(!valid_api_base("https://user:secret@api.example.com"));
        assert!(!valid_api_base("https://api.example.com/${INJECT}"));
        assert!(!valid_api_base("https://api.example.com #comment"));
        assert!(!valid_api_base("https://api.example.com\nRUN evil"));
        assert!(!valid_api_base("https://"));
    }
}
