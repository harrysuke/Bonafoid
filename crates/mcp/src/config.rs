//! Environment configuration for the MCP server.
//!
//! Every value has a safe default so the stdio binary can start with no
//! configuration. Project saves are the only operation that requires a secret
//! (`MCP_API_TOKEN`), and that requirement is enforced at call time.

use std::path::PathBuf;
use std::str::FromStr;

/// Transport selected by the standalone binary. The HTTP transport is
/// normally mounted in-process by `nocode-ide-server` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stdio,
    Http,
}

impl FromStr for Transport {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "stdio" => Ok(Self::Stdio),
            "http" => Ok(Self::Http),
            other => Err(format!(
                "unknown MCP_TRANSPORT `{other}` (expected stdio or http)"
            )),
        }
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio => f.write_str("stdio"),
            Self::Http => f.write_str("http"),
        }
    }
}

/// Runtime limits and integration settings.
#[derive(Debug, Clone)]
pub struct Config {
    pub transport: Transport,
    pub bind_addr: String,
    pub api_base_url: String,
    pub api_token: Option<String>,
    pub max_nodes: usize,
    pub undo_depth: usize,
    pub audit_log: Option<PathBuf>,
    /// Files written by `save_workflow(target="file")` must live below this
    /// directory. This is the path-traversal guard from plan §7.4.
    pub file_root: PathBuf,
}

impl Config {
    /// Read configuration from the environment. Invalid numeric values fall
    /// back to the documented defaults rather than preventing startup.
    pub fn from_env() -> Self {
        let transport = std::env::var("MCP_TRANSPORT")
            .ok()
            .and_then(|value| Transport::from_str(&value).ok())
            .unwrap_or(Transport::Stdio);
        Self {
            transport,
            bind_addr: std::env::var("MCP_BIND_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:8090".to_string()),
            api_base_url: std::env::var("MCP_API_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8081".to_string()),
            api_token: std::env::var("MCP_API_TOKEN")
                .ok()
                .filter(|v| !v.is_empty()),
            max_nodes: env_usize("MCP_MAX_NODES", 200),
            undo_depth: env_usize("MCP_UNDO_DEPTH", 20),
            audit_log: std::env::var("MCP_AUDIT_LOG")
                .ok()
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            file_root: std::env::var("MCP_FILE_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::from_env()
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_transport() {
        assert_eq!("stdio".parse::<Transport>().unwrap(), Transport::Stdio);
        assert_eq!("HTTP".parse::<Transport>().unwrap(), Transport::Http);
        assert!("socket".parse::<Transport>().is_err());
    }

    #[test]
    fn numeric_limits_fall_back_safely() {
        assert_eq!(env_usize("MCP_TEST_UNSET_LIMIT", 200), 200);
        assert_eq!(env_usize("MCP_TEST_UNSET_DEPTH", 20), 20);
        assert_eq!(env_usize("MCP_TEST_UNSET_LIMIT", 7), 7);
    }
}
