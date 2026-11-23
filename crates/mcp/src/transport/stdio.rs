//! Newline-delimited JSON-RPC over stdin/stdout. All diagnostics go to
//! stderr so stdout stays a clean MCP channel.

use std::io::{self, BufRead, BufWriter, Write};

use crate::{CallerContext, McpServer};

/// Serve one stdio client until stdin closes.
pub fn run(server: &McpServer) -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    let caller = CallerContext::local();

    eprintln!("bonafoid-mcp: stdio transport ready");

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(&caller, &line) {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    eprintln!("bonafoid-mcp: stdin closed");
    Ok(())
}
