mod cli;
mod config;
mod mcp;
mod output;
pub mod web;

use std::{io, process::ExitCode};

use anyhow::Context;

use clap::Parser;
use rmcp::ServiceExt;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{fmt::MakeWriter, prelude::*};

use crate::{
    cli::Command,
    config::Config,
    web::{
        fetch::{FetchError, FetchFormat, FetchRequest, FetchService},
        search::{SearchError, SearchRequest, SearchService, TimeRange},
    },
};

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Runtime(#[from] anyhow::Error),
}

impl AppError {
    fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::from(2),
            Self::Runtime(_) => ExitCode::FAILURE,
        }
    }
}

/// Runs the xngmcp command-line application.
pub async fn run() -> ExitCode {
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(exit_code as u8);
        }
    };

    let config = match Config::from_cli(&cli) {
        Ok(config) => config,
        Err(error) => return report_error(error),
    };

    if let Err(error) = init_tracing(config.log_level) {
        return report_error(error.into());
    }

    tracing::debug!(
        color_enabled = !config.no_color,
        "resolved application settings"
    );

    let cancellation = CancellationToken::new();
    let shutdown_listener = spawn_shutdown_listener(cancellation.clone());
    let result = dispatch(&cli.command, &config, cancellation).await;
    shutdown_listener.abort();
    let _ = shutdown_listener.await;

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_error(error),
    }
}

fn report_error(error: AppError) -> ExitCode {
    eprintln!("xngmcp: {error}");
    error.exit_code()
}

fn init_tracing(level: tracing_subscriber::filter::LevelFilter) -> anyhow::Result<()> {
    tracing::subscriber::set_global_default(make_subscriber(level, io::stderr))
        .context("could not initialize diagnostics")
}

fn make_subscriber<W>(
    level: tracing_subscriber::filter::LevelFilter,
    writer: W,
) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    tracing_subscriber::registry().with(level).with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(writer),
    )
}

fn spawn_shutdown_listener(cancellation: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        match wait_for_shutdown_signal().await {
            Ok(()) => cancellation.cancel(),
            Err(error) => tracing::error!(%error, "could not install shutdown signal listener"),
        }
    })
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() -> io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = interrupt.recv() => Ok(()),
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> io::Result<()> {
    tokio::signal::ctrl_c().await
}

async fn dispatch(
    command: &Command,
    config: &Config,
    cancellation: CancellationToken,
) -> Result<(), AppError> {
    match command {
        Command::Search {
            query,
            limit,
            page,
            language,
            time_range,
            categories,
            engines,
            safe_search,
            include_domains,
            exclude_domains,
            json,
            plain,
        } => {
            let request = SearchRequest {
                query: query.clone(),
                limit: *limit,
                page: *page,
                language: language.clone(),
                time_range: time_range.as_deref().map(|value| match value {
                    "day" => TimeRange::Day,
                    "month" => TimeRange::Month,
                    "year" => TimeRange::Year,
                    _ => unreachable!("Clap validates time ranges"),
                }),
                categories: categories.clone(),
                engines: engines.clone(),
                safe_search: *safe_search,
                include_domains: include_domains.clone(),
                exclude_domains: exclude_domains.clone(),
            };
            search(config, request, *json, *plain, cancellation).await
        }
        Command::Fetch {
            url,
            max_chars,
            format,
            json,
        } => {
            let request = FetchRequest {
                url: url.clone(),
                max_chars: *max_chars,
                format: format.as_deref().map(|value| match value {
                    "markdown" => FetchFormat::Markdown,
                    "text" => FetchFormat::Text,
                    _ => unreachable!("Clap validates fetch formats"),
                }),
            };
            fetch(config, request, *json, cancellation).await
        }
        Command::Serve => serve(config, cancellation).await,
    }
}

async fn search(
    config: &Config,
    request: SearchRequest,
    json: bool,
    plain: bool,
    cancellation: CancellationToken,
) -> Result<(), AppError> {
    let service = SearchService::with_default_timeout(config.searxng_url.clone())
        .map_err(|error| AppError::Runtime(error.into()))?;
    let status = output::StatusLine::start(output::status_enabled(), "Searching…");
    let response = service.search(request, cancellation).await;
    drop(status);
    let response = response.map_err(map_search_error)?;

    let mut stdout = io::stdout().lock();
    if json {
        output::write_json(&mut stdout, &response).context("could not write search result")?;
    } else if plain {
        output::write_plain_search(&mut stdout, &response)
            .context("could not write search result")?;
    } else {
        output::write_human_search(
            &mut stdout,
            &response,
            output::color_enabled(config.no_color),
        )
        .context("could not write search result")?;
    }
    Ok(())
}

async fn fetch(
    config: &Config,
    request: FetchRequest,
    json: bool,
    cancellation: CancellationToken,
) -> Result<(), AppError> {
    let service =
        FetchService::with_default_timeout().map_err(|error| AppError::Runtime(error.into()))?;
    let status = output::StatusLine::start(output::status_enabled(), "Fetching…");
    let response = service.fetch(request, cancellation).await;
    drop(status);
    let response = response.map_err(map_fetch_error)?;

    let mut stdout = io::stdout().lock();
    if json {
        output::write_json(&mut stdout, &response).context("could not write fetched content")?;
    } else {
        output::write_human_fetch(
            &mut stdout,
            &response,
            output::color_enabled(config.no_color),
        )
        .context("could not write fetched content")?;
    }
    Ok(())
}

fn map_search_error(error: SearchError) -> AppError {
    match error {
        SearchError::Validation(message) => AppError::usage(message),
        error => AppError::Runtime(error.into()),
    }
}

fn map_fetch_error(error: FetchError) -> AppError {
    match error {
        FetchError::Validation(message) => AppError::usage(message),
        error => AppError::Runtime(error.into()),
    }
}

async fn serve(config: &Config, cancellation: CancellationToken) -> Result<(), AppError> {
    let search = SearchService::with_default_timeout(config.searxng_url.clone())
        .map_err(|error| AppError::Runtime(error.into()))?;
    let fetch =
        FetchService::with_default_timeout().map_err(|error| AppError::Runtime(error.into()))?;
    let server = mcp::McpServer::new(search, fetch, cancellation.clone());
    tracing::debug!(
        searxng_origin = %config.searxng_url.origin().ascii_serialization(),
        "starting stdio MCP server"
    );
    let service = match server
        .serve_with_ct(rmcp::transport::stdio(), cancellation)
        .await
    {
        Ok(service) => service,
        Err(rmcp::service::ServerInitializeError::Cancelled)
        | Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
        Err(error) => return Err(AppError::Runtime(error.into())),
    };
    service
        .waiting()
        .await
        .context("stdio MCP server stopped unexpectedly")
        .map(|_| ())
        .map_err(AppError::Runtime)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        path::Path,
        sync::{Arc, Mutex},
    };

    use tracing_subscriber::fmt::MakeWriter;
    use url::Url;

    use super::*;

    #[derive(Clone)]
    struct BufferWriter(Arc<Mutex<Vec<u8>>>);

    struct LockedBufferWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for LockedBufferWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("test buffer lock")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for BufferWriter {
        type Writer = LockedBufferWriter;

        fn make_writer(&'writer self) -> Self::Writer {
            LockedBufferWriter(self.0.clone())
        }
    }

    #[test]
    fn application_shell_tracing_writes_diagnostics_to_its_writer() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let subscriber = make_subscriber(
            tracing_subscriber::filter::LevelFilter::DEBUG,
            BufferWriter(output.clone()),
        );

        tracing::subscriber::with_default(subscriber, || tracing::debug!("test diagnostic"));

        let output = String::from_utf8(output.lock().expect("test buffer lock").clone())
            .expect("tracing output is UTF-8");
        assert!(output.contains("test diagnostic"));
    }

    #[tokio::test]
    async fn application_shell_cancellation_propagates_to_serve() {
        let config = Config {
            searxng_url: Url::parse("http://127.0.0.1:8080").expect("valid test URL"),
            log_level: tracing_subscriber::filter::LevelFilter::WARN,
            no_color: false,
        };
        let cancellation = CancellationToken::new();
        let cancellation_to_trigger = cancellation.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancellation_to_trigger.cancel();
        });

        assert!(
            dispatch(&Command::Serve, &config, cancellation)
                .await
                .is_ok()
        );
    }

    #[test]
    fn generated_cli_assets_exist_and_describe_the_current_interface() {
        let assets_directory = Path::new(env!("XNGMCP_GENERATED_DIR"));
        let man_directory = assets_directory.join("man");
        let completions_directory = assets_directory.join("completions");
        let root_man_page =
            fs::read_to_string(man_directory.join("xngmcp.1")).expect("root man page is generated");
        let serve_man_page = fs::read_to_string(man_directory.join("xngmcp-serve.1"))
            .expect("serve man page is generated");
        let search_man_page = fs::read_to_string(man_directory.join("xngmcp-search.1"))
            .expect("search man page is generated");
        let fetch_man_page = fs::read_to_string(man_directory.join("xngmcp-fetch.1"))
            .expect("fetch man page is generated");

        assert!(root_man_page.contains("searxng\\-url"));
        assert!(root_man_page.contains("search"));
        assert!(root_man_page.contains("fetch"));
        assert!(serve_man_page.contains("Run the stdio MCP server"));
        assert!(search_man_page.contains("include\\-domain"));
        assert!(fetch_man_page.contains("max\\-chars"));

        for completion in [
            "xngmcp.bash",
            "_xngmcp",
            "xngmcp.fish",
            "xngmcp.elv",
            "_xngmcp.ps1",
        ] {
            let contents = fs::read_to_string(completions_directory.join(completion))
                .unwrap_or_else(|_| panic!("{completion} completion is generated"));
            assert!(contents.contains("serve"));
            assert!(contents.contains("search"));
            assert!(contents.contains("fetch"));
            assert!(contents.contains("searxng-url"));
        }
    }
}
