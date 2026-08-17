use std::env;

use tracing_subscriber::filter::LevelFilter;
use url::Url;

use crate::{AppError, cli::Cli};

const DEFAULT_SEARXNG_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_LOG_LEVEL: &str = "warn";

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) searxng_url: Url,
    pub(crate) log_level: LevelFilter,
    pub(crate) no_color: bool,
}

impl Config {
    pub(crate) fn from_cli(cli: &Cli) -> Result<Self, AppError> {
        Self::resolve(cli, |name| env::var(name).ok())
    }

    fn resolve<F>(cli: &Cli, environment: F) -> Result<Self, AppError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let searxng_url = cli
            .searxng_url
            .as_deref()
            .map(str::to_owned)
            .or_else(|| environment("SEARXNG_URL"))
            .unwrap_or_else(|| DEFAULT_SEARXNG_URL.to_owned());
        let searxng_url = Url::parse(&searxng_url)
            .map_err(|error| AppError::usage(format!("invalid SearXNG URL: {error}")))?;

        if !matches!(searxng_url.scheme(), "http" | "https") || searxng_url.host().is_none() {
            return Err(AppError::usage(
                "invalid SearXNG URL: expected an absolute HTTP or HTTPS URL",
            ));
        }

        let log_level = cli
            .log_level
            .as_deref()
            .map(str::to_owned)
            .or_else(|| environment("XNGMCP_LOG_LEVEL"))
            .unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_owned())
            .parse::<LevelFilter>()
            .map_err(|_| {
                AppError::usage("invalid log level: use error, warn, info, debug, or trace")
            })?;

        Ok(Self {
            searxng_url,
            log_level,
            no_color: cli.no_color
                || environment("NO_COLOR").is_some_and(|value| !value.is_empty()),
        })
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::Command;

    #[test]
    fn application_shell_configuration_flags_override_environment() {
        let cli = Cli::parse_from([
            "xngmcp",
            "--searxng-url",
            "https://flag.example/search",
            "--log-level",
            "debug",
            "--no-color",
            "serve",
        ]);
        let config = Config::resolve(&cli, |name| match name {
            "SEARXNG_URL" => Some("https://environment.example".into()),
            "XNGMCP_LOG_LEVEL" => Some("error".into()),
            "NO_COLOR" => Some(String::new()),
            _ => None,
        })
        .expect("flag settings are valid");

        assert_eq!(config.searxng_url.as_str(), "https://flag.example/search");
        assert_eq!(config.log_level, LevelFilter::DEBUG);
        assert!(config.no_color);
    }

    #[test]
    fn application_shell_configuration_uses_environment_then_defaults() {
        let cli = Cli {
            searxng_url: None,
            log_level: None,
            no_color: false,
            command: Command::Serve,
        };
        let environment = |name: &str| match name {
            "SEARXNG_URL" => Some("https://environment.example".into()),
            "XNGMCP_LOG_LEVEL" => Some("info".into()),
            "NO_COLOR" => Some("1".into()),
            _ => None,
        };
        let config = Config::resolve(&cli, environment).expect("environment settings are valid");

        assert_eq!(config.searxng_url.as_str(), "https://environment.example/");
        assert_eq!(config.log_level, LevelFilter::INFO);
        assert!(config.no_color);

        let defaults = Config::resolve(&cli, |_| None).expect("defaults are valid");
        assert_eq!(
            defaults.searxng_url.as_str(),
            format!("{DEFAULT_SEARXNG_URL}/")
        );
        assert_eq!(defaults.log_level, LevelFilter::WARN);
        assert!(!defaults.no_color);
    }

    #[test]
    fn application_shell_configuration_rejects_invalid_settings() {
        let invalid_url = Cli {
            searxng_url: Some("file:///tmp/searxng".into()),
            log_level: None,
            no_color: false,
            command: Command::Serve,
        };
        let invalid_log_level = Cli {
            searxng_url: None,
            log_level: Some("loud".into()),
            no_color: false,
            command: Command::Serve,
        };

        assert!(Config::resolve(&invalid_url, |_| None).is_err());
        assert!(Config::resolve(&invalid_log_level, |_| None).is_err());
    }
}
