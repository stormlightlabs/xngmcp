use clap::{ArgAction, Parser, Subcommand, builder::NonEmptyStringValueParser};

/// Command-line interface shared by the application and build script.
#[derive(Debug, Parser)]
#[command(
    name = "xngmcp",
    version,
    about = "Search and fetch the public web through a local SearXNG instance.",
    arg_required_else_help = true,
    infer_subcommands = false,
    color = clap::ColorChoice::Never,
    after_help = "Examples:\n  xngmcp search 'Rust MCP SDK'\n  xngmcp search --json --limit 5 'Rust MCP SDK'\n  xngmcp fetch --max-chars 5000 https://example.com/article\n\nExit codes: 0 on success, 1 for backend or network failures, 2 for usage errors."
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
    /// Search the public web through SearXNG.
    #[command(
        after_help = "Examples:\n  xngmcp search 'Rust MCP SDK'\n  xngmcp search --category general --engine brave --json 'Rust MCP SDK'\n  xngmcp search --include-domain github.com --plain 'Rust MCP SDK'"
    )]
    Search {
        /// Search terms. SearXNG search syntax is supported.
        #[arg(value_parser = NonEmptyStringValueParser::new())]
        query: String,

        /// Maximum results to return (1-20).
        #[arg(long)]
        limit: Option<u8>,

        /// Search-results page (1-10).
        #[arg(long)]
        page: Option<u8>,

        /// SearXNG language code, or all.
        #[arg(long)]
        language: Option<String>,

        /// Limit results to day, month, or year.
        #[arg(long, value_parser = ["day", "month", "year"])]
        time_range: Option<String>,

        /// Search category. May be repeated.
        #[arg(long = "category", action = ArgAction::Append)]
        categories: Vec<String>,

        /// Search engine. May be repeated.
        #[arg(long = "engine", action = ArgAction::Append)]
        engines: Vec<String>,

        /// Safe-search level: 0, 1, or 2.
        #[arg(long)]
        safe_search: Option<u8>,

        /// Restrict results to this hostname or its subdomains. May be repeated.
        #[arg(long = "include-domain", action = ArgAction::Append)]
        include_domains: Vec<String>,

        /// Exclude this hostname and its subdomains. May be repeated.
        #[arg(long = "exclude-domain", action = ArgAction::Append)]
        exclude_domains: Vec<String>,

        /// Emit the shared structured JSON response.
        #[arg(long, conflicts_with = "plain")]
        json: bool,

        /// Emit one tab-separated title, URL, and snippet per result.
        #[arg(long, conflicts_with = "json")]
        plain: bool,
    },

    /// Fetch readable content from a public HTTP(S) URL.
    #[command(
        after_help = "Examples:\n  xngmcp fetch https://example.com/article\n  xngmcp fetch --format text --max-chars 5000 --json https://example.com/article"
    )]
    Fetch {
        /// Public HTTP or HTTPS URL to fetch.
        #[arg(value_parser = NonEmptyStringValueParser::new())]
        url: String,

        /// Maximum readable characters to return (1,000-100,000).
        #[arg(long)]
        max_chars: Option<usize>,

        /// Readable output format.
        #[arg(long, value_parser = ["markdown", "text"])]
        format: Option<String>,

        /// Emit the shared structured JSON response.
        #[arg(long)]
        json: bool,
    },

    /// Run the stdio MCP server.
    Serve,
}
