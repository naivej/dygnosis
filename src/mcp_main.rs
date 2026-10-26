//! MCP-only process. LSP never installs this allocator.

mod cli_logging;

use clap::{Parser, Subcommand};
use dygnosis::VERSION;

#[global_allocator]
static ALLOCATOR: dygnosis::retained_memory::TrackingAllocator =
    dygnosis::retained_memory::TrackingAllocator;

#[derive(Parser)]
#[command(name = "dygnosis-mcp", version = VERSION, about = "Dygnosis MCP server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start MCP over stdio (also the default)
    Mcp,
}

fn main() {
    cli_logging::init();
    Cli::parse();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Could not start the MCP server runtime")
        .block_on(dygnosis::mcp::run_stdio());
}
