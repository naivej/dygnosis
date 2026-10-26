use clap::{Parser, Subcommand};
use dygnosis::VERSION;

mod cli_logging;
mod mcp_launcher;

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

fn main() {
    cli_logging::init();
    let cli = Cli::parse();
    if matches!(cli.command, Some(Commands::Mcp)) {
        match mcp_launcher::run() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Could not start the language server runtime");
    runtime.block_on(async {
        match cli.command {
            Some(Commands::Mcp) => {
                unreachable!("MCP delegates before starting a language server runtime")
            }
            None if cli.tcp => dygnosis::server::run_tcp(&cli.host, cli.port).await,
            None => dygnosis::server::run_stdio().await,
        }
    });
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
        assert!(matches!(cli.command, Some(Commands::Mcp)));
        assert!(!cli.tcp);
    }

    #[test]
    fn retired_mcp_profile_arguments_are_rejected() {
        // Guards silently accepting an obsolete launch configuration.
        let error = Cli::try_parse_from(["dygnosis", "mcp", "--profile", "legacy-v2"])
            .err()
            .expect("retired profile must fail");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
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
