//! Environment allowlist for preview child processes.
//!
//! Previews execute code generated from a project model, so a child must never
//! inherit the IDE server's environment wholesale: `DATABASE_URL`,
//! `JWT_KEYS_DIR` and any operator-provided secret would be readable by (and
//! exfiltrable from) that generated code. Children are started from a cleared
//! environment that receives only the variables below, plus whatever the
//! caller sets explicitly.
//!
//! The build children (`cargo`, `trunk`) run the generated crate's build
//! scripts, so they get the same treatment: a build script is arbitrary code
//! too.

use tokio::process::Command;

/// Variables a preview child inherits from the server, unless the caller sets
/// its own value.
///
/// Kept to what a compiled binary and the Rust toolchain need. Notably absent:
/// `DATABASE_URL`, every JWT/secret variable, and `RUST_LOG`.
const ALLOWED: &[&str] = &[
    // Process basics. Windows needs SYSTEMROOT/WINDIR/COMSPEC for most of the
    // C runtime to work at all; PATHEXT resolves bare program names.
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    // Home + temp, used by cargo for its config and by the linker for objects.
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TMP",
    "TEMP",
    "TMPDIR",
    // Locale.
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    // Rust toolchain: the registry cache lives in CARGO_HOME, the toolchains
    // themselves in RUSTUP_HOME. Dropping these makes `cargo` re-download.
    "CARGO_HOME",
    "RUSTUP_HOME",
    "CARGO_NET_OFFLINE",
    "RUSTC_WRAPPER",
    // Outbound HTTP(S), needed by trunk's wasm-bindgen download and by any
    // crate fetched at build time.
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// True when `name` is on the allowlist.
///
/// Windows environment variables are case-insensitive, so the comparison is too
/// (a server started with `Path` must not end up with an empty `PATH`).
pub fn is_allowed(name: &str) -> bool {
    ALLOWED
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(name))
}

/// The allowlisted variables currently set in the server's own environment.
///
/// Reads through `std::env::vars`, which on Windows exposes each variable once
/// under its canonical casing.
pub fn inherited() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(key, _)| is_allowed(key))
        .collect()
}

/// Start `command` from an empty environment plus the allowlist.
///
/// Callers add the variables the child actually needs (for example `PORT` or
/// `CARGO_TARGET_DIR`) after this call.
pub fn isolate(command: &mut Command) -> &mut Command {
    command.env_clear();
    for (key, value) in inherited() {
        command.env(key, value);
    }
    command
}

/// Variables the child creates for itself, so a listing of the child's
/// environment will show them even though the parent never had them.
///
/// `cmd.exe` defines `PROMPT` internally (`$P$G`), which is why it shows up in
/// `cmd /C set` on Windows.
#[cfg(test)]
const SYNTHESIZED_BY_CHILD: &[&str] = &["PROMPT"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_and_secret_variables_are_never_inherited() {
        for name in [
            "DATABASE_URL",
            "JWT_KEYS_DIR",
            "JWT_SECRET",
            "NOCODE_DB_PASSWORD",
            "AWS_SECRET_ACCESS_KEY",
            "RUST_LOG",
        ] {
            assert!(!is_allowed(name), "{name} must not be inherited");
        }
    }

    #[test]
    fn toolchain_and_platform_variables_are_inherited() {
        for name in [
            "PATH",
            "HOME",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "TMPDIR",
            "HTTPS_PROXY",
            "SSL_CERT_FILE",
            "SYSTEMROOT",
            "COMSPEC",
        ] {
            assert!(is_allowed(name), "{name} should be inherited");
        }
    }

    #[test]
    fn matching_is_case_insensitive_on_every_platform() {
        assert!(is_allowed("path"));
        assert!(is_allowed("Cargo_Home"));
        assert!(!is_allowed("database_url"));
    }

    #[test]
    fn inherited_contains_no_secret_shaped_variables() {
        // The real server environment may well contain a DATABASE_URL; the
        // filter is what keeps it away from children.
        let vars = inherited();
        assert!(
            vars.iter().all(|(key, _)| is_allowed(key)),
            "inherited() leaked a non-allowlisted variable: {vars:?}"
        );
    }

    /// Spawn a real child through `isolate` and read back its environment.
    ///
    /// The unit test above only checks the filter; this proves `env_clear`
    /// actually reaches the child, which is the property that keeps the IDE
    /// server's credentials out of generated code.
    #[tokio::test]
    async fn a_spawned_child_sees_only_the_allowlist() {
        // Values only the parent knows; if they reach the child, isolation
        // failed. The second one is deliberately not on the allowlist and
        // stands in for DATABASE_URL, JWT_KEYS_DIR and friends.
        let explicit_marker = "nocode_preview_explicit_probe";
        let parent_secret = "nocode_preview_parent_secret_probe";
        std::env::set_var("NOCODE_PREVIEW_PARENT_SECRET_PROBE", parent_secret);

        let mut command = match std::env::consts::OS {
            "windows" => {
                let mut command = Command::new("cmd");
                command.args(["/C", "set"]);
                command
            }
            _ => {
                let mut command = Command::new("sh");
                command.args(["-c", "env"]);
                command
            }
        };
        isolate(&mut command);
        command.env("NOCODE_PREVIEW_EXPLICIT_PROBE", explicit_marker);

        let output = command
            .output()
            .await
            .expect("spawn an environment-listing child");
        let listing = String::from_utf8_lossy(&output.stdout).to_string();
        std::env::remove_var("NOCODE_PREVIEW_PARENT_SECRET_PROBE");

        // The explicit variable survives: `isolate` clears, it does not block.
        assert!(
            listing.contains(explicit_marker),
            "explicit variables must still be passed: {listing}"
        );
        // The parent's non-allowlisted variable does not.
        assert!(
            !listing.contains(parent_secret),
            "parent environment leaked into the child: {listing}"
        );
        let offending: Vec<String> = listing
            .lines()
            .filter_map(|line| {
                let key = line.split('=').next().unwrap_or_default().trim();
                if key.is_empty()
                    || is_allowed(key)
                    || key == "NOCODE_PREVIEW_EXPLICIT_PROBE"
                    || SYNTHESIZED_BY_CHILD.contains(&key)
                {
                    None
                } else {
                    Some(key.to_string())
                }
            })
            .collect();
        assert!(
            offending.is_empty(),
            "child saw variables that are neither allowlisted nor explicit: {offending:?}"
        );
    }
}
