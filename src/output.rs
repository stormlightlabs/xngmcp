use std::{
    ffi::OsStr,
    io::{self, IsTerminal, Write},
};

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::web::{fetch::FetchResponse, search::SearchResponse};

pub(crate) fn color_enabled(no_color: bool) -> bool {
    can_color(
        no_color,
        io::stdout().is_terminal(),
        std::env::var_os("TERM").as_deref(),
    )
}

pub(crate) fn status_enabled() -> bool {
    can_show_status(io::stderr().is_terminal(), std::env::var_os("TERM").as_deref())
}

fn can_color(no_color: bool, stdout_is_terminal: bool, term: Option<&OsStr>) -> bool {
    !no_color && stdout_is_terminal && term != Some(OsStr::new("dumb"))
}

fn can_show_status(stderr_is_terminal: bool, term: Option<&OsStr>) -> bool {
    stderr_is_terminal && term != Some(OsStr::new("dumb"))
}

pub(crate) fn write_json<W, T>(writer: &mut W, value: &T) -> io::Result<()>
where
    W: Write,
    T: Serialize,
{
    serde_json::to_writer(&mut *writer, value).map_err(io::Error::other)?;
    writer.write_all(b"\n")
}

pub(crate) fn write_plain_search<W>(writer: &mut W, response: &SearchResponse) -> io::Result<()>
where
    W: Write,
{
    for result in &response.results {
        writeln!(
            writer,
            "{}\t{}\t{}",
            collapse_whitespace(&result.title),
            collapse_whitespace(&result.url),
            collapse_whitespace(&result.snippet),
        )?;
    }
    Ok(())
}

pub(crate) fn write_human_search<W>(writer: &mut W, response: &SearchResponse, color: bool) -> io::Result<()>
where
    W: Write,
{
    if response.results.is_empty() {
        return writeln!(writer, "No results found.");
    }

    for (index, result) in response.results.iter().enumerate() {
        if index > 0 {
            writeln!(writer)?;
        }
        if color {
            writeln!(writer, "{}", result.title.bold().cyan())?;
            writeln!(writer, "{}", result.url.dimmed())?;
        } else {
            writeln!(writer, "{}", result.title)?;
            writeln!(writer, "{}", result.url)?;
        }
        if !result.snippet.trim().is_empty() {
            writeln!(writer, "{}", result.snippet)?;
        }
    }
    Ok(())
}

pub(crate) fn write_human_fetch<W>(writer: &mut W, response: &FetchResponse, color: bool) -> io::Result<()>
where
    W: Write,
{
    if let Some(title) = &response.title {
        if color {
            writeln!(writer, "{}", title.bold().cyan())?;
            writeln!(writer, "{}\n", response.url.dimmed())?;
        } else {
            writeln!(writer, "{title}")?;
            writeln!(writer, "{}\n", response.url)?;
        }
    }
    writeln!(writer, "{}", response.content)
}

pub(crate) struct StatusLine {
    active: bool,
}

impl StatusLine {
    pub(crate) fn start(enabled: bool, message: &str) -> Self {
        if enabled {
            eprint!("{message}");
            let _ = io::stderr().flush();
        }
        Self { active: enabled }
    }
}

impl Drop for StatusLine {
    fn drop(&mut self) {
        if self.active {
            eprint!("\r\x1b[2K");
            let _ = io::stderr().flush();
        }
    }
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::search::SearchResult;

    fn response() -> SearchResponse {
        SearchResponse {
            query: "rust".into(),
            results: vec![SearchResult {
                title: "A\ttitle".into(),
                url: "https://example.com/article".into(),
                snippet: "First\nsecond".into(),
                score: 1.0,
                published_at: None,
                engines: Vec::new(),
            }],
            suggestions: None,
            unresponsive_engines: None,
        }
    }

    #[test]
    fn cli_plain_search_collapses_embedded_whitespace() {
        let mut output = Vec::new();
        write_plain_search(&mut output, &response()).expect("plain output writes");
        assert_eq!(
            String::from_utf8(output).expect("output is UTF-8"),
            "A title\thttps://example.com/article\tFirst second\n"
        );
    }

    #[test]
    fn cli_json_output_is_undecorated_and_valid() {
        let mut output = Vec::new();
        write_json(&mut output, &response()).expect("JSON output writes");
        let output = String::from_utf8(output).expect("output is UTF-8");
        assert!(serde_json::from_str::<serde_json::Value>(&output).is_ok());
        assert!(!output.contains("\x1b["));
    }

    #[test]
    fn cli_human_output_only_adds_ansi_when_requested() {
        let mut plain = Vec::new();
        write_human_search(&mut plain, &response(), false).expect("human output writes");
        assert!(!plain.contains(&b'\x1b'));

        let mut styled = Vec::new();
        write_human_search(&mut styled, &response(), true).expect("styled output writes");
        assert!(styled.contains(&b'\x1b'));
    }

    #[test]
    fn cli_color_requires_a_terminal_and_permits_standard_opt_outs() {
        assert!(can_color(false, true, Some(OsStr::new("xterm-256color"))));
        assert!(!can_color(true, true, Some(OsStr::new("xterm-256color"))));
        assert!(!can_color(false, false, Some(OsStr::new("xterm-256color"))));
        assert!(!can_color(false, true, Some(OsStr::new("dumb"))));
        assert!(can_show_status(true, Some(OsStr::new("xterm-256color"))));
        assert!(!can_show_status(false, Some(OsStr::new("xterm-256color"))));
        assert!(!can_show_status(true, Some(OsStr::new("dumb"))));
    }
}
