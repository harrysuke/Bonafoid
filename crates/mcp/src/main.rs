//! `nocode-mcp` binary: stdio MCP server for local desktop clients.

use nocode_mcp::config::Transport;
use nocode_mcp::{transport, Config, McpServer};

fn main() {
    let config = Config::from_env();
    if config.transport == Transport::Http {
        eprintln!(
            "bonafoid-mcp: HTTP transport is mounted in-process by nocode-ide-server at /mcp; \
             run the stdio binary with MCP_TRANSPORT=stdio"
        );
        std::process::exit(2);
    }
    let server = McpServer::new(config);
    if let Err(error) = transport::stdio::run(&server) {
        eprintln!("bonafoid-mcp: stdio transport failed: {error}");
        std::process::exit(1);
    }
}
