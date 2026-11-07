//! `tuldmi-mcp [--data-dir <dir>]`: serves tuldMi's history to an MCP client over stdio until
//! stdin closes. See docs/mcp.md.

use std::io::{BufReader, stdin, stdout};
use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    // stdout carries the protocol; logs go to stderr, which MCP clients show or discard.
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false).with_target(false).init();

    let mut args = std::env::args().skip(1);
    let mut data_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "--version" => {
                println!("tuldmi-mcp {}", env!("CARGO_PKG_VERSION"));
                return std::process::ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                println!("tuldmi-mcp: tuldMi's transcripts for AI assistants (Model Context Protocol over stdio).\n\nAn MCP client starts this program; it is not meant to be run by hand.\nAccess is off until turned on in tuldMi: Settings > Privacy & data > AI assistants.\n\nOptions:\n  --data-dir <dir>  read this tuldMi data folder instead of the app's\n  --version");
                return std::process::ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("tuldmi-mcp: unknown argument (see --help)");
                return std::process::ExitCode::from(2);
            }
        }
    }
    let Some(data_dir) = data_dir.or_else(tuldmi_mcp::default_data_dir) else {
        eprintln!("tuldmi-mcp: could not find tuldMi's data folder; pass --data-dir");
        return std::process::ExitCode::from(2);
    };
    let server = tuldmi_mcp::McpServer::new(data_dir, tuldmi_mcp::sibling_app(), env!("CARGO_PKG_VERSION"));
    match tuldmi_mcp::serve(&server, BufReader::new(stdin().lock()), stdout().lock()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}
