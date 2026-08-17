mod cli;
mod config;
pub mod web;

use std::{io, process::ExitCode};

use anyhow::Context;

use clap::Parser;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{fmt::MakeWriter, prelude::*};

use crate::{cli::Command, config::Config};

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
        Command::Serve => serve(config, cancellation).await,
    }
}

async fn serve(config: &Config, cancellation: CancellationToken) -> Result<(), AppError> {
    tracing::debug!(
        searxng_origin = %config.searxng_url.origin().ascii_serialization(),
        "waiting for stdio MCP transport"
    );
    cancellation.cancelled().await;
    Ok(())
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

        assert!(root_man_page.contains("searxng\\-url"));
        assert!(root_man_page.contains("serve"));
        assert!(serve_man_page.contains("Run the stdio MCP server"));

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
            assert!(contents.contains("searxng-url"));
        }
    }
}
