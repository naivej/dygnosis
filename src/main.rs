use clap::{Parser, Subcommand};
use dygnosis::{check_file, format_check_lines, maybe_run_and_reconcile, Severity, VERSION};
use std::path::Path;

#[derive(Parser)]
#[command(name = "dygnosis", version = VERSION, about = "Thin Dynare analysis (LSP / MCP)")]
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
    /// Run diagnostics on a .mod file and exit (no server)
    Check {
        /// Path to a .mod / .inc file
        file: String,
    },
    /// Print markdown documentation for a diagnostic code
    Explain {
        /// Diagnostic code (for example E010)
        code: Option<String>,
        /// List documented diagnostic codes
        #[arg(long)]
        list: bool,
    },
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
        Some(Commands::Check { file }) => run_check(&file),
        Some(Commands::Explain { code, list }) => run_explain(code, list),
        Some(Commands::Mcp) => dygnosis::mcp::run_stdio().await,
        None if cli.tcp => dygnosis::server::run_tcp(&cli.host, cli.port).await,
        None => dygnosis::server::run_stdio().await,
    }
}

fn run_check(file: &str) {
    let text = match std::fs::read(file) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("Error: File not found: {file}");
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("Error: Cannot read {file}: {err}");
            std::process::exit(1);
        }
    };
    let abs_path = abs_path_for_workspace(file);
    let own = check_file(&text, &abs_path);
    let diags = maybe_run_and_reconcile(own, &text, Path::new(&abs_path).parent(), None);
    print!("{}", format_check_lines(file, &diags, &text));
    if diags.iter().any(|d| d.severity == Severity::Error) {
        std::process::exit(1);
    }
}

fn abs_path_for_workspace(path: &str) -> String {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        p.to_string_lossy().into_owned()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p).to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string())
    }
}

fn run_explain(code: Option<String>, list: bool) {
    if list || code.is_none() {
        let codes = dygnosis::explain::known_codes();
        println!("Documented diagnostic codes:");
        for c in &codes {
            let title = dygnosis::explain::explain(c).map(|e| e.title).unwrap_or("");
            println!("  {c:<6}  {title}");
        }
        println!(
            "\n{} codes. Run `dygnosis explain <CODE>` for details.",
            codes.len()
        );
        return;
    }
    let code = code.unwrap();
    match dygnosis::explain::render_markdown(&code) {
        Some(rendered) => println!("{rendered}"),
        None => {
            eprintln!(
                "No documentation found for diagnostic code '{code}'. Run `dygnosis explain --list` to see known codes."
            );
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_flags_do_not_take_check_subcommand() {
        let cli = Cli::parse_from(["dygnosis", "--tcp", "--port", "2087"]);
        assert!(cli.tcp);
        assert_eq!(cli.port, 2087);
        assert_eq!(cli.host, "127.0.0.1");
        assert!(cli.command.is_none());
    }

    #[test]
    fn check_subcommand_still_parses() {
        let cli = Cli::parse_from(["dygnosis", "check", "model.mod"]);
        match cli.command {
            Some(Commands::Check { file }) => assert_eq!(file, "model.mod"),
            other => panic!("expected check, got {other:?}"),
        }
        assert!(!cli.tcp);
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
}
