use clap::{Parser, Subcommand};
use dygnosis::VERSION;

#[derive(Parser)]
#[command(name = "dygnosis", version = VERSION, about = env!("CARGO_PKG_DESCRIPTION"))]
struct Cli {
    /// Start LSP over TCP (debug only)
    #[arg(long)]
    tcp: bool,
    /// TCP host (debug)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// TCP port (debug)
    #[arg(long, default_value_t = 2087)]
    port: u16,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start MCP over stdio
    Mcp,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Mcp) => dygnosis::mcp::run_stdio().await,
        None if cli.tcp => dygnosis::server::run_tcp(&cli.host, cli.port).await,
        None => dygnosis::server::run_stdio().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_flags_start_the_language_server() {
        let cli = Cli::parse_from(["dygnosis", "--tcp", "--port", "2087"]);
        assert!(cli.tcp);
        assert_eq!(cli.port, 2087);
        assert_eq!(cli.host, "127.0.0.1");
        assert!(cli.command.is_none());
    }

    #[test]
    fn mcp_subcommand_parses() {
        let cli = Cli::parse_from(["dygnosis", "mcp"]);
        match cli.command {
            Some(Commands::Mcp) => {}
            other => panic!("expected mcp, got {other:?}"),
        }
        assert!(!cli.tcp);
    }

    #[test]
    fn retired_user_commands_are_rejected() {
        for args in [
            ["dygnosis", "check", "model.mod"],
            ["dygnosis", "explain", "E001"],
            ["dygnosis", "explain", "--list"],
        ] {
            let err = match Cli::try_parse_from(args) {
                Err(err) => err,
                Ok(_) => panic!("retired command parsed: {args:?}"),
            };
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::InvalidSubcommand,
                "{args:?}"
            );
        }
    }
}
