use std::{collections::HashSet, time::Duration};

use reqwest::{Client, StatusCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

/// The whole-request deadline for searches sent to SearXNG.
pub const DEFAULT_SEARCH_TIMEOUT: Duration = Duration::from_secs(15);

const DEFAULT_PAGE: u8 = 1;
const DEFAULT_LIMIT: u8 = 8;
const DEFAULT_LANGUAGE: &str = "all";
const DEFAULT_CATEGORY: &str = "general";
const MAX_QUERY_LENGTH: usize = 1_000;
const MAX_LIST_VALUES: usize = 10;
const MAX_DOMAINS: usize = 20;

/// Input accepted by the normalized web-search service.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SearchRequest {
    /// Search terms. Must not be blank; SearXNG search syntax is supported.
    #[schemars(length(min = 1, max = 1_000))]
    pub query: String,
    /// Maximum results to return. Defaults to 8.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 20))]
    pub limit: Option<u8>,
    /// Search-results page. Defaults to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 10))]
    pub page: Option<u8>,
    /// Nonblank SearXNG language code, or `all`. Defaults to `all`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub language: Option<String>,
    /// Limit results to `day`, `month`, or `year`. Omit for all time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<TimeRange>,
    /// Nonblank search categories. Defaults to `["general"]`; at most 10 values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 10), inner(length(min = 1)))]
    pub categories: Vec<String>,
    /// Nonblank search engines. Omit to use SearXNG's configured engines; at most 10 values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 10), inner(length(min = 1)))]
    pub engines: Vec<String>,
    /// Safe-search level: 0, 1, or 2. Defaults to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(max = 2))]
    pub safe_search: Option<u8>,
    /// Restrict results to normalized hostnames or their subdomains; at most 20 values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 20), inner(length(min = 1)))]
    pub include_domains: Vec<String>,
    /// Exclude normalized hostnames and their subdomains; at most 20 values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 20), inner(length(min = 1)))]
    pub exclude_domains: Vec<String>,
}

impl SearchRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: None,
            page: None,
            language: None,
            time_range: None,
            categories: Vec::new(),
            engines: Vec::new(),
            safe_search: None,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
        }
    }

    fn validate(self) -> Result<ValidatedSearchRequest, SearchError> {
        if self.query.trim().is_empty() {
            return Err(SearchError::validation("query must not be blank"));
        }
        if self.query.chars().count() > MAX_QUERY_LENGTH {
            return Err(SearchError::validation(format!(
                "query must be at most {MAX_QUERY_LENGTH} characters"
            )));
        }

        let limit = self.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=20).contains(&limit) {
            return Err(SearchError::validation("limit must be between 1 and 20"));
        }

        let page = self.page.unwrap_or(DEFAULT_PAGE);
        if !(1..=10).contains(&page) {
            return Err(SearchError::validation("page must be between 1 and 10"));
        }

        let language = validate_language(self.language.as_deref().unwrap_or(DEFAULT_LANGUAGE))?;
        let categories = validate_values(
            "categories",
            self.categories,
            MAX_LIST_VALUES,
            &[DEFAULT_CATEGORY],
        )?;
        let engines = validate_values("engines", self.engines, MAX_LIST_VALUES, &[])?;

        let safe_search = self.safe_search.unwrap_or(1);
        if safe_search > 2 {
            return Err(SearchError::validation("safe_search must be 0, 1, or 2"));
        }

        let include_domains = validate_domains("include_domains", self.include_domains)?;
        let exclude_domains = validate_domains("exclude_domains", self.exclude_domains)?;

        Ok(ValidatedSearchRequest {
            query: self.query,
            limit,
            page,
            language,
            time_range: self.time_range,
            categories,
            engines,
            safe_search,
            include_domains,
            exclude_domains,
        })
    }
}

/// SearXNG's supported recency filters.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeRange {
    Day,
    Month,
    Year,
}

impl TimeRange {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}

/// A normalized search response suitable for JSON output and MCP structured content.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchResult>,
    #[serde(skip_serializing_if = "option_vec_is_empty")]
    pub suggestions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "option_vec_is_empty")]
    pub unresponsive_engines: Option<Vec<UnavailableEngine>>,
}

/// A normalized result returned by SearXNG.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub score: f64,
    #[serde(skip_serializing_if = "option_string_is_blank")]
    pub published_at: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub engines: Vec<String>,
}

/// An engine that did not contribute to a completed search.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnavailableEngine {
    pub name: String,
    #[serde(skip_serializing_if = "option_string_is_blank")]
    pub reason: Option<String>,
}

/// Failures from input validation or the SearXNG backend.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SearchError {
    #[error("invalid search request: {0}")]
    Validation(String),
    #[error("search was cancelled")]
    Cancelled,
    #[error("SearXNG did not respond before the search deadline")]
    Timeout,
    #[error("SearXNG returned HTTP {0}; check that the backend is healthy")]
    Backend(StatusCode),
    #[error("SearXNG returned an invalid JSON search response")]
    MalformedResponse,
    #[error("could not reach SearXNG; check the configured backend URL")]
    Transport,
}

impl SearchError {
    fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

/// Reusable asynchronous client for the configured SearXNG JSON endpoint.
#[derive(Clone, Debug)]
pub struct SearchService {
    client: Client,
    endpoint: Url,
}

impl SearchService {
    pub fn with_default_timeout(searxng_url: Url) -> Result<Self, SearchError> {
        Self::new(searxng_url, DEFAULT_SEARCH_TIMEOUT)
    }

    pub fn new(searxng_url: Url, timeout: Duration) -> Result<Self, SearchError> {
        let endpoint = search_endpoint(searxng_url)?;
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| SearchError::Transport)?;

        Ok(Self { client, endpoint })
    }

    pub async fn search(
        &self,
        request: SearchRequest,
        cancellation: CancellationToken,
    ) -> Result<SearchResponse, SearchError> {
        let request = request.validate()?;
        let parameters = request.parameters();
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(SearchError::Cancelled),
            result = self.client.get(self.endpoint.clone()).query(&parameters).send() => {
                result.map_err(map_reqwest_error)?
            }
        };

        if !response.status().is_success() {
            return Err(SearchError::Backend(response.status()));
        }

        let body = tokio::select! {
            _ = cancellation.cancelled() => return Err(SearchError::Cancelled),
            result = response.text() => result.map_err(map_reqwest_error)?
        };
        let upstream: UpstreamResponse =
            serde_json::from_str(&body).map_err(|_| SearchError::MalformedResponse)?;

        Ok(request.normalize(upstream))
    }
}

#[derive(Clone, Debug)]
struct ValidatedSearchRequest {
    query: String,
    limit: u8,
    page: u8,
    language: String,
    time_range: Option<TimeRange>,
    categories: Vec<String>,
    engines: Vec<String>,
    safe_search: u8,
    include_domains: Vec<String>,
    exclude_domains: Vec<String>,
}

impl ValidatedSearchRequest {
    fn parameters(&self) -> Vec<(&str, String)> {
        let mut parameters = vec![
            ("q", self.query.clone()),
            ("format", "json".to_owned()),
            ("pageno", self.page.to_string()),
            ("language", self.language.clone()),
            ("categories", self.categories.join(",")),
            ("safesearch", self.safe_search.to_string()),
        ];

        if let Some(time_range) = self.time_range {
            parameters.push(("time_range", time_range.as_str().to_owned()));
        }
        if !self.engines.is_empty() {
            parameters.push(("engines", self.engines.join(",")));
        }

        parameters
    }

    fn normalize(self, upstream: UpstreamResponse) -> SearchResponse {
        let mut seen_urls = HashSet::new();
        let results = upstream
            .results
            .into_iter()
            .filter_map(|result| normalize_result(result, &self))
            .filter(|result| seen_urls.insert(result.url.clone()))
            .take(usize::from(self.limit))
            .collect();

        SearchResponse {
            query: self.query,
            results,
            suggestions: useful_strings(upstream.suggestions),
            unresponsive_engines: useful_engines(upstream.unresponsive_engines),
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpstreamResponse {
    results: Vec<UpstreamResult>,
    #[serde(default)]
    suggestions: Vec<String>,
    #[serde(default)]
    unresponsive_engines: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct UpstreamResult {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "content")]
    snippet: String,
    #[serde(default)]
    score: f64,
    #[serde(default, rename = "publishedDate")]
    published_at: Option<String>,
    #[serde(default)]
    engines: Vec<String>,
}

fn normalize_result(
    result: UpstreamResult,
    request: &ValidatedSearchRequest,
) -> Option<SearchResult> {
    let mut url = Url::parse(&result.url).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);

    let host = url.host_str()?.to_ascii_lowercase();
    if !matches_domain_list(&host, &request.include_domains, true)
        || matches_domain_list(&host, &request.exclude_domains, false)
    {
        return None;
    }

    Some(SearchResult {
        title: result.title,
        url: url.into(),
        snippet: result.snippet,
        score: result.score,
        published_at: result.published_at.filter(|value| !value.trim().is_empty()),
        engines: result
            .engines
            .into_iter()
            .filter(|engine| !engine.trim().is_empty())
            .collect(),
    })
}

fn matches_domain_list(host: &str, domains: &[String], default_when_empty: bool) -> bool {
    if domains.is_empty() {
        return default_when_empty;
    }

    domains.iter().any(|domain| {
        host == domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn option_vec_is_empty<T>(value: &Option<Vec<T>>) -> bool {
    value.as_ref().is_none_or(Vec::is_empty)
}

fn option_string_is_blank(value: &Option<String>) -> bool {
    value.as_ref().is_none_or(|value| value.trim().is_empty())
}

fn useful_strings(values: Vec<String>) -> Option<Vec<String>> {
    let values: Vec<_> = values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect();
    (!values.is_empty()).then_some(values)
}

fn useful_engines(values: Vec<serde_json::Value>) -> Option<Vec<UnavailableEngine>> {
    let values: Vec<_> = values
        .into_iter()
        .filter_map(|value| match value {
            serde_json::Value::Array(mut values) if !values.is_empty() => {
                let name = values.remove(0).as_str()?.trim().to_owned();
                (!name.is_empty()).then(|| UnavailableEngine {
                    name,
                    reason: values
                        .first()
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|reason| !reason.is_empty())
                        .map(str::to_owned),
                })
            }
            serde_json::Value::Object(values) => {
                let name = values.get("name")?.as_str()?.trim().to_owned();
                (!name.is_empty()).then(|| UnavailableEngine {
                    name,
                    reason: values
                        .get("reason")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|reason| !reason.is_empty())
                        .map(str::to_owned),
                })
            }
            _ => None,
        })
        .collect();
    (!values.is_empty()).then_some(values)
}

fn validate_language(value: &str) -> Result<String, SearchError> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(SearchError::validation(
            "language must be a non-blank SearXNG language code or all",
        ));
    }
    Ok(value.to_owned())
}

fn validate_values(
    name: &str,
    values: Vec<String>,
    maximum: usize,
    default: &[&str],
) -> Result<Vec<String>, SearchError> {
    if values.len() > maximum {
        return Err(SearchError::validation(format!(
            "{name} accepts at most {maximum} values"
        )));
    }

    let values: Vec<_> = values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .collect();
    if values.iter().any(String::is_empty) {
        return Err(SearchError::validation(format!(
            "{name} must not contain blank values"
        )));
    }

    if values.is_empty() {
        Ok(default.iter().map(|value| (*value).to_owned()).collect())
    } else {
        Ok(values)
    }
}

fn validate_domains(name: &str, domains: Vec<String>) -> Result<Vec<String>, SearchError> {
    if domains.len() > MAX_DOMAINS {
        return Err(SearchError::validation(format!(
            "{name} accepts at most {MAX_DOMAINS} domains"
        )));
    }

    domains
        .into_iter()
        .map(|domain| normalize_domain(name, &domain))
        .collect()
}

fn normalize_domain(name: &str, domain: &str) -> Result<String, SearchError> {
    let domain = domain.trim().trim_end_matches('.');
    if domain.is_empty() {
        return Err(SearchError::validation(format!(
            "{name} must contain normalized hostnames"
        )));
    }

    let url = Url::parse(&format!("http://{domain}")).map_err(|_| {
        SearchError::validation(format!("{name} must contain normalized hostnames"))
    })?;
    if url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
    {
        return Err(SearchError::validation(format!(
            "{name} must contain hostnames without ports or paths"
        )));
    }

    match url.host() {
        Some(Host::Domain(host)) => Ok(host.to_ascii_lowercase()),
        _ => Err(SearchError::validation(format!(
            "{name} must contain domain hostnames"
        ))),
    }
}

fn search_endpoint(searxng_url: Url) -> Result<Url, SearchError> {
    if searxng_url.path().ends_with("/search") {
        Ok(searxng_url)
    } else {
        searxng_url
            .join("search")
            .map_err(|_| SearchError::Transport)
    }
}

fn map_reqwest_error(error: reqwest::Error) -> SearchError {
    if error.is_timeout() {
        SearchError::Timeout
    } else {
        SearchError::Transport
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::{Duration, sleep},
    };

    use super::*;

    const RESULTS: &str = r#"{
        "results": [
            {
                "title": "First result",
                "url": "https://Example.com:443/article#section",
                "content": "First snippet",
                "score": 1.0,
                "publishedDate": "2026-08-01T00:00:00Z",
                "engines": ["brave"]
            },
            {
                "title": "Duplicate",
                "url": "https://example.com/article",
                "content": "Duplicate snippet",
                "score": 0.9,
                "engines": ["duckduckgo"]
            },
            {
                "title": "Invalid",
                "url": "not a URL",
                "content": "Ignored",
                "score": 0.8
            },
            {
                "title": "Subdomain",
                "url": "https://docs.example.com/guide",
                "content": "Second result",
                "score": 0.7
            }
        ],
        "suggestions": ["Rust SDK", "  "],
        "unresponsive_engines": [["google", "timeout"]]
    }"#;

    #[test]
    fn search_validation_applies_defaults_and_rejects_invalid_input() {
        let request = SearchRequest::new("SearXNG syntax: site:example.com");
        let request = request.validate().expect("defaults are valid");
        assert_eq!(request.query, "SearXNG syntax: site:example.com");
        assert_eq!(request.limit, DEFAULT_LIMIT);
        assert_eq!(request.page, DEFAULT_PAGE);
        assert_eq!(request.language, DEFAULT_LANGUAGE);
        assert_eq!(request.categories, [DEFAULT_CATEGORY]);
        assert_eq!(request.safe_search, 1);

        let cases = [
            (SearchRequest::new(" "), "query must not be blank"),
            (
                SearchRequest {
                    limit: Some(21),
                    ..SearchRequest::new("query")
                },
                "limit must be between",
            ),
            (
                SearchRequest {
                    page: Some(0),
                    ..SearchRequest::new("query")
                },
                "page must be between",
            ),
            (
                SearchRequest {
                    language: Some(" ".into()),
                    ..SearchRequest::new("query")
                },
                "language must be",
            ),
            (
                SearchRequest {
                    safe_search: Some(3),
                    ..SearchRequest::new("query")
                },
                "safe_search must be",
            ),
            (
                SearchRequest {
                    categories: vec![String::new()],
                    ..SearchRequest::new("query")
                },
                "categories must not",
            ),
            (
                SearchRequest {
                    engines: vec!["engine".into(); 11],
                    ..SearchRequest::new("query")
                },
                "engines accepts",
            ),
            (
                SearchRequest {
                    include_domains: vec!["example.com:443".into()],
                    ..SearchRequest::new("query")
                },
                "without ports",
            ),
        ];
        for (request, message) in cases {
            let error = request.validate().expect_err("request is invalid");
            assert!(error.to_string().contains(message));
        }
    }

    #[tokio::test]
    async fn search_encodes_valid_requests_and_preserves_query_text() {
        let seen = Arc::new(Mutex::new(None));
        let (endpoint, server) = fixture(StatusCode::OK, RESULTS, Some(seen.clone())).await;
        let service = SearchService::new(endpoint, Duration::from_secs(1)).expect("service");
        let response = service
            .search(
                SearchRequest {
                    query: "rust + MCP & \"quotes\"".into(),
                    time_range: Some(TimeRange::Month),
                    engines: vec!["brave".into(), "duckduckgo".into()],
                    ..SearchRequest::new("")
                },
                CancellationToken::new(),
            )
            .await
            .expect("search succeeds");
        server.await.expect("server completes");

        assert_eq!(response.query, "rust + MCP & \"quotes\"");
        let query: HashMap<_, _> = url::form_urlencoded::parse(
            seen.lock()
                .expect("request lock")
                .as_deref()
                .expect("request target")
                .as_bytes(),
        )
        .into_owned()
        .collect();
        assert_eq!(query.get("q"), Some(&"rust + MCP & \"quotes\"".to_owned()));
        assert_eq!(query.get("format"), Some(&"json".to_owned()));
        assert_eq!(query.get("pageno"), Some(&"1".to_owned()));
        assert_eq!(query.get("categories"), Some(&"general".to_owned()));
        assert_eq!(query.get("time_range"), Some(&"month".to_owned()));
        assert_eq!(query.get("engines"), Some(&"brave,duckduckgo".to_owned()));
    }

    #[tokio::test]
    async fn search_normalizes_filters_and_preserves_rank() {
        let (endpoint, server) = fixture(StatusCode::OK, RESULTS, None).await;
        let service = SearchService::new(endpoint, Duration::from_secs(1)).expect("service");
        let response = service
            .search(
                SearchRequest {
                    query: "rust".into(),
                    include_domains: vec!["EXAMPLE.com.".into()],
                    exclude_domains: vec!["docs.example.com".into()],
                    ..SearchRequest::new("")
                },
                CancellationToken::new(),
            )
            .await
            .expect("search succeeds");
        server.await.expect("server completes");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].title, "First result");
        assert_eq!(response.results[0].url, "https://example.com/article");
        assert_eq!(response.results[0].snippet, "First snippet");
        assert_eq!(response.results[0].engines, ["brave"]);
        assert_eq!(response.suggestions, Some(vec!["Rust SDK".into()]));
        assert_eq!(
            response.unresponsive_engines,
            Some(vec![UnavailableEngine {
                name: "google".into(),
                reason: Some("timeout".into()),
            }])
        );
    }

    #[tokio::test]
    async fn search_domain_rules_do_not_match_suffixes_or_ports() {
        let body = r#"{
            "results": [
                {"url": "https://notexample.com/", "title": "suffix", "content": "", "score": 1.0},
                {"url": "https://api.example.com:8443/", "title": "subdomain", "content": "", "score": 0.9}
            ]
        }"#;
        let (endpoint, server) = fixture(StatusCode::OK, body, None).await;
        let service = SearchService::new(endpoint, Duration::from_secs(1)).expect("service");
        let response = service
            .search(
                SearchRequest {
                    query: "rust".into(),
                    include_domains: vec!["example.com".into()],
                    ..SearchRequest::new("")
                },
                CancellationToken::new(),
            )
            .await
            .expect("search succeeds");
        server.await.expect("server completes");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].title, "subdomain");
    }

    #[tokio::test]
    async fn search_keeps_results_when_an_engine_is_unavailable() {
        let body = r#"{
            "results": [{"url": "https://example.com/", "title": "result", "content": "", "score": 1.0}],
            "unresponsive_engines": [["brave", "timeout"]]
        }"#;
        let (endpoint, server) = fixture(StatusCode::OK, body, None).await;
        let service = SearchService::new(endpoint, Duration::from_secs(1)).expect("service");
        let response = service
            .search(SearchRequest::new("rust"), CancellationToken::new())
            .await
            .expect("partial search succeeds");
        server.await.expect("server completes");

        assert_eq!(response.results.len(), 1);
        assert!(response.unresponsive_engines.is_some());
    }

    #[tokio::test]
    async fn search_maps_backend_malformed_and_cancelled_responses() {
        let (endpoint, server) = fixture(StatusCode::BAD_GATEWAY, "backend failed", None).await;
        let service = SearchService::new(endpoint, Duration::from_secs(1)).expect("service");
        assert_eq!(
            service
                .search(SearchRequest::new("rust"), CancellationToken::new())
                .await,
            Err(SearchError::Backend(StatusCode::BAD_GATEWAY))
        );
        server.await.expect("server completes");

        let (endpoint, server) = fixture(StatusCode::OK, "not json", None).await;
        let service = SearchService::new(endpoint, Duration::from_secs(1)).expect("service");
        assert_eq!(
            service
                .search(SearchRequest::new("rust"), CancellationToken::new())
                .await,
            Err(SearchError::MalformedResponse)
        );
        server.await.expect("server completes");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let endpoint = Url::parse(&format!(
            "http://{}/",
            listener.local_addr().expect("address")
        ))
        .expect("URL");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await.expect("read request");
            sleep(Duration::from_millis(50)).await;
        });
        let service = SearchService::new(endpoint, Duration::from_millis(1)).expect("service");
        assert_eq!(
            service
                .search(SearchRequest::new("rust"), CancellationToken::new())
                .await,
            Err(SearchError::Timeout)
        );
        server.abort();

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let endpoint = Url::parse(&format!(
            "http://{}/",
            listener.local_addr().expect("address")
        ))
        .expect("URL");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await.expect("read request");
            sleep(Duration::from_secs(10)).await;
        });
        let service = SearchService::new(endpoint, Duration::from_secs(30)).expect("service");
        let cancellation = CancellationToken::new();
        let cancellation_to_trigger = cancellation.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(20)).await;
            cancellation_to_trigger.cancel();
        });
        assert_eq!(
            service
                .search(SearchRequest::new("rust"), cancellation)
                .await,
            Err(SearchError::Cancelled)
        );
        server.abort();
    }

    #[test]
    fn search_json_omits_empty_optional_fields() {
        let json = serde_json::to_value(SearchResponse {
            query: "rust".into(),
            results: vec![SearchResult {
                title: "Result".into(),
                url: "https://example.com/".into(),
                snippet: String::new(),
                score: 1.0,
                published_at: None,
                engines: Vec::new(),
            }],
            suggestions: Some(Vec::new()),
            unresponsive_engines: Some(Vec::new()),
        })
        .expect("serializes");
        assert!(json.get("suggestions").is_none());
        assert!(json.get("unresponsive_engines").is_none());
        assert!(json["results"][0].get("published_at").is_none());
        assert!(json["results"][0].get("engines").is_none());
    }

    #[tokio::test]
    #[ignore = "requires XNGMCP_TEST_SEARXNG_URL and a running SearXNG endpoint"]
    async fn search_integration_normalizes_a_real_searxng_response() {
        let endpoint = std::env::var("XNGMCP_TEST_SEARXNG_URL")
            .expect("set XNGMCP_TEST_SEARXNG_URL to a running SearXNG endpoint");
        let endpoint = Url::parse(&endpoint).expect("valid XNGMCP_TEST_SEARXNG_URL");
        let service = SearchService::new(endpoint, Duration::from_secs(15)).expect("service");
        let response = service
            .search(
                SearchRequest::new("rust programming language"),
                CancellationToken::new(),
            )
            .await
            .expect("SearXNG search succeeds");

        assert_eq!(response.query, "rust programming language");
        assert!(!response.results.is_empty());
        assert!(
            response
                .results
                .iter()
                .all(|result| Url::parse(&result.url).is_ok())
        );
    }

    async fn fixture(
        status: StatusCode,
        body: &'static str,
        seen_target: Option<Arc<Mutex<Option<String>>>>,
    ) -> (Url, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let endpoint = Url::parse(&format!(
            "http://{}/",
            listener.local_addr().expect("address")
        ))
        .expect("URL");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            loop {
                let bytes = stream.read(&mut buffer).await.expect("read request");
                if bytes == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..bytes]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            if let Some(seen_target) = seen_target {
                let request = String::from_utf8(request).expect("request is UTF-8");
                let target = request
                    .split_whitespace()
                    .nth(1)
                    .and_then(|target| target.split_once('?').map(|(_, query)| query.to_owned()));
                *seen_target.lock().expect("request lock") = target;
            }
            let response = format!(
                "HTTP/1.1 {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                status.as_u16(),
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        });
        (endpoint, server)
    }
}
