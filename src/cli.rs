use clap::{Parser, Subcommand};

/// Command-line interface shared by the application and build script.
#[derive(Debug, Parser)]
#[command(
    name = "xngmcp",
    version,
    about = "Search and fetch the public web through a local SearXNG instance.",
    arg_required_else_help = true,
    infer_subcommands = false,
    after_help = "Examples:\n  xngmcp --searxng-url http://127.0.0.1:8080 serve\n  xngmcp serve"
)]
pub struct Cli {
    /// URL of the local SearXNG backend.
    #[arg(long, value_name = "URL", global = true)]
    pub searxng_url: Option<String>,

    /// Diagnostic verbosity: error, warn, info, debug, or trace.
    #[arg(long, value_name = "LEVEL", global = true)]
    pub log_level: Option<String>,

    /// Disable colored terminal output.
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the stdio MCP server.
    Serve,
}
