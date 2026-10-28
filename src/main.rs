use clap::Parser;
use dygnosis::VERSION;

mod cli_logging;

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
}

fn main() {
    cli_logging::init();
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Could not start the language server runtime");
    runtime.block_on(async {
        if cli.tcp {
            dygnosis::server::run_tcp(&cli.host, cli.port).await;
        } else {
            dygnosis::server::run_stdio().await;
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
    }
}
