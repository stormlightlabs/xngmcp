use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
    time::Duration,
};

use encoding_rs::{Encoding, UTF_8};
use lectito::{ReadabilityOptions, extract};
use reqwest::{
    Client, StatusCode,
    dns::{Addrs, Name, Resolve, Resolving},
    header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    redirect::Policy,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::{net::lookup_host, task::spawn_blocking, time::timeout};
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

/// The whole-request deadline for public web fetches.
pub const DEFAULT_FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 5;
const MAX_BODY_BYTES: usize = 5_000_000;
const DEFAULT_MAX_CHARS: usize = 30_000;
const MIN_MAX_CHARS: usize = 1_000;
const MAX_MAX_CHARS: usize = 100_000;

/// Input accepted by the public web-fetch service.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct FetchRequest {
    /// Public absolute HTTP or HTTPS URL to fetch.
    #[schemars(length(min = 1))]
    pub url: String,
    /// Maximum readable characters to return. Defaults to 30,000.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1_000, max = 100_000))]
    pub max_chars: Option<usize>,
    /// Readable output format. Defaults to `markdown`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<FetchFormat>,
}

impl FetchRequest {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_chars: None,
            format: None,
        }
    }

    fn validate(self) -> Result<ValidatedFetchRequest, FetchError> {
        let url = Url::parse(&self.url)
            .map_err(|_| FetchError::validation("url must be an absolute HTTP or HTTPS URL"))?;
        validate_public_url(&url)?;

        let max_chars = self.max_chars.unwrap_or(DEFAULT_MAX_CHARS);
        if !(MIN_MAX_CHARS..=MAX_MAX_CHARS).contains(&max_chars) {
            return Err(FetchError::validation(format!(
                "max_chars must be between {MIN_MAX_CHARS} and {MAX_MAX_CHARS}"
            )));
        }

        Ok(ValidatedFetchRequest {
            url,
            max_chars,
            format: self.format.unwrap_or_default(),
        })
    }
}

/// Readable output format for fetched HTML.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FetchFormat {
    #[default]
    Markdown,
    Text,
}

/// A bounded, readable public web response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FetchResponse {
    /// Final URL after validated redirects.
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub content: String,
    pub content_type: String,
    pub truncated: bool,
}

/// Failures from public URL validation, network policy, and article extraction.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("invalid fetch request: {0}")]
    Validation(String),
    #[error("fetch was cancelled")]
    Cancelled,
    #[error("URL resolves to blocked address {0}")]
    BlockedAddress(IpAddr),
    #[error("URL did not resolve to a public address")]
    NoPublicAddress,
    #[error("fetch timed out")]
    Timeout,
    #[error("too many redirects (maximum {MAX_REDIRECTS})")]
    TooManyRedirects,
    #[error("redirect response did not provide a valid public HTTP(S) URL")]
    InvalidRedirect,
    #[error("server returned HTTP {0}")]
    Backend(StatusCode),
    #[error("response body exceeds the {MAX_BODY_BYTES}-byte limit")]
    BodyTooLarge,
    #[error("unsupported content type {0:?}; only HTML and plain text are supported")]
    UnsupportedMedia(String),
    #[error("unsupported character encoding {0:?}")]
    UnsupportedCharset(String),
    #[error("could not reach the public URL")]
    Transport,
    #[error("page does not contain a readable article")]
    NoArticle,
    #[error("could not extract readable content")]
    Extraction(#[source] lectito::Error),
}

impl FetchError {
    fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

#[derive(Clone, Debug)]
struct ValidatedFetchRequest {
    url: Url,
    max_chars: usize,
    format: FetchFormat,
}

/// Reusable HTTP client that only dials public addresses.
#[derive(Clone, Debug)]
pub struct FetchService {
    client: Client,
    operation_timeout: Duration,
    response_header_timeout: Duration,
}

impl FetchService {
    pub fn with_default_timeout() -> Result<Self, FetchError> {
        Self::new(DEFAULT_FETCH_TIMEOUT)
    }

    pub fn new(operation_timeout: Duration) -> Result<Self, FetchError> {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(Policy::none())
            .referer(false)
            .dns_resolver(Arc::new(PublicResolver))
            .build()
            .map_err(|_| FetchError::Transport)?;
        Ok(Self {
            client,
            operation_timeout,
            response_header_timeout: RESPONSE_HEADER_TIMEOUT,
        })
    }

    pub async fn fetch(
        &self,
        request: FetchRequest,
        cancellation: CancellationToken,
    ) -> Result<FetchResponse, FetchError> {
        let request = request.validate()?;
        let operation = timeout(self.operation_timeout, self.fetch_validated(request));
        tokio::select! {
            _ = cancellation.cancelled() => Err(FetchError::Cancelled),
            result = operation => result.map_err(|_| FetchError::Timeout)?,
        }
    }

    async fn fetch_validated(
        &self,
        request: ValidatedFetchRequest,
    ) -> Result<FetchResponse, FetchError> {
        let ValidatedFetchRequest {
            mut url,
            max_chars,
            format,
        } = request;
        for redirect_count in 0..=MAX_REDIRECTS {
            validate_public_url(&url)?;
            let response = timeout(
                self.response_header_timeout,
                self.client.get(url.clone()).send(),
            )
            .await
            .map_err(|_| FetchError::Timeout)?
            .map_err(map_reqwest_error)?;

            if response.status().is_redirection() {
                if redirect_count == MAX_REDIRECTS {
                    return Err(FetchError::TooManyRedirects);
                }
                url = redirect_target(&url, response.headers().get(LOCATION))?;
                continue;
            }
            if !response.status().is_success() {
                return Err(FetchError::Backend(response.status()));
            }

            return response_to_fetch_result(response, max_chars, format).await;
        }
        Err(FetchError::TooManyRedirects)
    }

    #[cfg(test)]
    pub(crate) fn with_test_client(client: Client, operation_timeout: Duration) -> Self {
        Self {
            client,
            operation_timeout,
            response_header_timeout: RESPONSE_HEADER_TIMEOUT,
        }
    }
}

async fn response_to_fetch_result(
    mut response: reqwest::Response,
    max_chars: usize,
    format: FetchFormat,
) -> Result<FetchResponse, FetchError> {
    let final_url = response.url().clone();
    validate_public_url(&final_url)?;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| FetchError::UnsupportedMedia("missing Content-Type".into()))?;
    if response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_BODY_BYTES)
    {
        return Err(FetchError::BodyTooLarge);
    }

    let body = read_limited_body(&mut response).await?;
    let (media_type, charset) = parse_content_type(&content_type)?;
    let content = decode_body(&body, charset)?;

    let (title, content) = match media_type.as_str() {
        "text/plain" => (None, content),
        "text/html" | "application/xhtml+xml" => {
            extract_content(content, final_url.clone(), format).await?
        }
        _ => return Err(FetchError::UnsupportedMedia(content_type)),
    };
    let (content, truncated) = truncate_content(content, max_chars);

    Ok(FetchResponse {
        url: final_url.into(),
        title,
        content,
        content_type,
        truncated,
    })
}

async fn read_limited_body(response: &mut reqwest::Response) -> Result<Vec<u8>, FetchError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
            return Err(FetchError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn extract_content(
    html: String,
    url: Url,
    format: FetchFormat,
) -> Result<(Option<String>, String), FetchError> {
    let extraction =
        spawn_blocking(move || extract(&html, Some(url.as_str()), &ReadabilityOptions::default()))
            .await
            .map_err(|_| FetchError::NoArticle)?
            .map_err(FetchError::Extraction)?;
    let article = extraction.ok_or(FetchError::NoArticle)?;
    let content = match format {
        FetchFormat::Markdown => article.markdown,
        FetchFormat::Text => article.text_content,
    };
    if content.trim().is_empty() {
        return Err(FetchError::NoArticle);
    }
    Ok((
        article.title.filter(|title| !title.trim().is_empty()),
        content,
    ))
}

fn validate_public_url(url: &Url) -> Result<(), FetchError> {
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(FetchError::validation(
            "url must be an absolute HTTP or HTTPS URL",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FetchError::validation("URLs must not contain credentials"));
    }
    if let Some(Host::Ipv4(address)) = url.host() {
        validate_public_address(IpAddr::V4(address))?;
    }
    if let Some(Host::Ipv6(address)) = url.host() {
        validate_public_address(IpAddr::V6(address))?;
    }
    Ok(())
}

fn redirect_target(
    current: &Url,
    location: Option<&reqwest::header::HeaderValue>,
) -> Result<Url, FetchError> {
    let location = location
        .and_then(|value| value.to_str().ok())
        .ok_or(FetchError::InvalidRedirect)?;
    let target = current
        .join(location)
        .map_err(|_| FetchError::InvalidRedirect)?;
    validate_public_url(&target).map_err(|_| FetchError::InvalidRedirect)?;
    Ok(target)
}

fn parse_content_type(content_type: &str) -> Result<(String, Option<&str>), FetchError> {
    let mut parts = content_type.split(';');
    let media_type = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    if media_type.is_empty() {
        return Err(FetchError::UnsupportedMedia(content_type.to_owned()));
    }
    let charset = parts.find_map(|parameter| {
        let (name, value) = parameter.trim().split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches('"'))
    });
    Ok((media_type, charset))
}

fn decode_body(body: &[u8], charset: Option<&str>) -> Result<String, FetchError> {
    let encoding = match charset {
        Some(charset) => Encoding::for_label(charset.as_bytes())
            .ok_or_else(|| FetchError::UnsupportedCharset(charset.to_owned()))?,
        None => UTF_8,
    };
    Ok(encoding.decode(body).0.into_owned())
}

fn truncate_content(content: String, max_chars: usize) -> (String, bool) {
    let Some((byte_index, _)) = content.char_indices().nth(max_chars) else {
        return (content, false);
    };
    (content[..byte_index].to_owned(), true)
}

fn map_reqwest_error(error: reqwest::Error) -> FetchError {
    if error.is_timeout() {
        FetchError::Timeout
    } else {
        FetchError::Transport
    }
}

#[derive(Debug)]
struct PublicResolver;

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses: Vec<_> = lookup_host((host.as_str(), 0)).await?.collect();
            if addresses.is_empty() {
                return Err(io::Error::other("host did not resolve").into());
            }
            for address in &addresses {
                if is_blocked_address(address.ip()) {
                    return Err(io::Error::other(format!(
                        "host resolved to blocked address {}",
                        address.ip()
                    ))
                    .into());
                }
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

fn validate_public_address(address: IpAddr) -> Result<(), FetchError> {
    if is_blocked_address(address) {
        Err(FetchError::BlockedAddress(address))
    } else {
        Ok(())
    }
}

fn is_blocked_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_blocked_ipv4(address),
        IpAddr::V6(address) => is_blocked_ipv6(address),
    }
}

fn is_blocked_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, _, _] = address.octets();
    first == 0
        || first == 10
        || first == 127
        || (first == 100 && (64..=127).contains(&second))
        || (first == 169 && second == 254)
        || (first == 172 && (16..=31).contains(&second))
        || (first == 192 && (second == 0 || second == 168))
        || (first == 198 && (second == 18 || second == 19))
        || (first == 198 && second == 51)
        || (first == 203 && second == 0)
        || first >= 224
}

fn is_blocked_ipv6(address: Ipv6Addr) -> bool {
    if address.is_unspecified() || address.is_loopback() || address.is_multicast() {
        return true;
    }

    let segments = address.segments();
    if (segments[0] & 0xfe00) == 0xfc00 || (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }

    let embedded_ipv4 =
        match segments {
            [0, 0, 0, 0, 0, 0, high, low] | [0, 0, 0, 0, 0, 0xffff, high, low] => Some(
                Ipv4Addr::new((high >> 8) as u8, high as u8, (low >> 8) as u8, low as u8),
            ),
            _ => None,
        };
    embedded_ipv4.is_some_and(is_blocked_ipv4)
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, net::SocketAddr, str::FromStr};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::sleep,
    };

    use super::*;

    #[test]
    fn fetch_validation_applies_defaults_and_rejects_invalid_input() {
        let request = FetchRequest::new("https://example.com/article")
            .validate()
            .expect("valid request");
        assert_eq!(request.max_chars, DEFAULT_MAX_CHARS);
        assert_eq!(request.format, FetchFormat::Markdown);

        for request in [
            FetchRequest::new("file:///tmp/article"),
            FetchRequest::new("http://user:secret@example.com/article"),
            FetchRequest {
                max_chars: Some(999),
                ..FetchRequest::new("https://example.com")
            },
            FetchRequest {
                max_chars: Some(100_001),
                ..FetchRequest::new("https://example.com")
            },
        ] {
            assert!(request.validate().is_err());
        }
    }

    #[test]
    fn fetch_blocks_private_reserved_and_ipv4_mapped_addresses() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.0.1",
            "224.0.0.1",
            "::",
            "::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(
                is_blocked_address(address.parse().expect("IP address")),
                "{address}"
            );
        }
        assert!(!is_blocked_address("8.8.8.8".parse().expect("public IP")));
        assert!(!is_blocked_address(
            "2606:4700:4700::1111".parse().expect("public IP")
        ));
    }

    #[tokio::test]
    async fn fetch_resolver_rejects_blocked_dns_answers() {
        let resolver = PublicResolver;
        let name = Name::from_str("localhost").expect("DNS name");
        assert!(resolver.resolve(name).await.is_err());
    }

    #[tokio::test]
    async fn fetch_handles_html_plain_text_redirects_and_limits() {
        let article = format!(
            "<html><head><title>Example title</title></head><body><article><h1>Example title</h1><p>{}</p></article></body></html>",
            "Readable article content. ".repeat(40)
        );
        let responses = vec![
            http_response(
                "200 OK",
                "text/html; charset=windows-1252",
                article.as_bytes(),
            ),
            http_response(
                "200 OK",
                "text/plain; charset=windows-1252",
                b"plain public text",
            ),
        ];
        let (url, server) = fixture(responses).await;
        let service = fixture_service(url.port().expect("fixture port"));

        let html = service
            .fetch(FetchRequest::new(url.as_str()), CancellationToken::new())
            .await
            .expect("HTML fetch succeeds");
        assert_eq!(html.title.as_deref(), Some("Example title"));
        assert!(html.content.contains("Readable article content"));
        assert_eq!(html.content_type, "text/html; charset=windows-1252");

        let text = service
            .fetch(
                FetchRequest {
                    url: url.into(),
                    max_chars: Some(1_000),
                    format: Some(FetchFormat::Text),
                },
                CancellationToken::new(),
            )
            .await
            .expect("plain text fetch succeeds");
        assert_eq!(text.content, "plain public text");
        assert_eq!(text.title, None);
        assert_eq!(
            decode_body(b"caf\xe9", Some("windows-1252")).expect("supported charset"),
            "café"
        );
        server.await.expect("fixture completes");
    }

    #[tokio::test]
    async fn fetch_rejects_private_redirects_and_oversized_or_slow_responses() {
        let redirect = http_response("302 Found", "text/plain", b"").replace(
            "content-length: 0\r\n",
            "content-length: 0\r\nlocation: http://127.0.0.1/\r\n",
        );
        let oversized = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        let (url, server) = fixture(vec![redirect, oversized]).await;
        let service = fixture_service(url.port().expect("fixture port"));

        assert!(matches!(
            service
                .fetch(FetchRequest::new(url.as_str()), CancellationToken::new())
                .await,
            Err(FetchError::InvalidRedirect)
        ));
        assert!(matches!(
            service
                .fetch(FetchRequest::new(url.as_str()), CancellationToken::new())
                .await,
            Err(FetchError::BodyTooLarge)
        ));
        server.await.expect("fixture completes");

        let (url, server) = delayed_fixture().await;
        let service = FetchService::with_test_client(
            fixture_client(url.port().expect("fixture port")),
            Duration::from_millis(10),
        );
        assert!(matches!(
            service
                .fetch(FetchRequest::new(url.as_str()), CancellationToken::new())
                .await,
            Err(FetchError::Timeout)
        ));
        server.await.expect("fixture completes");

        let (url, server) = delayed_fixture().await;
        let service = FetchService::with_test_client(
            fixture_client(url.port().expect("fixture port")),
            Duration::from_secs(1),
        );
        let cancellation = CancellationToken::new();
        let cancellation_to_trigger = cancellation.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await;
            cancellation_to_trigger.cancel();
        });
        assert!(matches!(
            service
                .fetch(FetchRequest::new(url.as_str()), cancellation)
                .await,
            Err(FetchError::Cancelled)
        ));
        server.await.expect("fixture completes");
    }

    #[tokio::test]
    async fn fetch_selects_readable_formats_and_truncates_on_unicode_boundaries() {
        assert_eq!(truncate_content("ab🦀cd".into(), 3), ("ab🦀".into(), true));
        assert_eq!(
            truncate_content("ab🦀cd".into(), 5),
            ("ab🦀cd".into(), false)
        );

        let html = format!(
            "<html><head><title>Example title</title></head><body><article><h1>Example title</h1><p>{}</p></article></body></html>",
            "Readable article content. ".repeat(40)
        );
        let url = Url::parse("https://example.com/article").expect("test URL");
        let (markdown_title, markdown) =
            extract_content(html.clone(), url.clone(), FetchFormat::Markdown)
                .await
                .expect("Markdown extraction");
        let (_, text) = extract_content(html, url, FetchFormat::Text)
            .await
            .expect("text extraction");
        assert_eq!(markdown_title.as_deref(), Some("Example title"));
        assert!(markdown.contains("Readable article content"));
        assert!(text.contains("Readable article content"));

        assert!(matches!(
            extract_content(
                "<html><body><nav><a href='/'>Home</a></nav></body></html>".into(),
                Url::parse("https://example.com/").expect("test URL"),
                FetchFormat::Markdown,
            )
            .await,
            Err(FetchError::NoArticle)
        ));
    }

    fn fixture_service(port: u16) -> FetchService {
        FetchService::with_test_client(fixture_client(port), Duration::from_secs(1))
    }

    fn fixture_client(port: u16) -> Client {
        Client::builder()
            .redirect(Policy::none())
            .dns_resolver(Arc::new(FixtureResolver {
                address: SocketAddr::from(([127, 0, 0, 1], port)),
            }))
            .build()
            .expect("fixture client")
    }

    #[derive(Debug)]
    struct FixtureResolver {
        address: SocketAddr,
    }

    impl Resolve for FixtureResolver {
        fn resolve(&self, _name: Name) -> Resolving {
            let address = self.address;
            Box::pin(async move { Ok(Box::new([address].into_iter()) as Addrs) })
        }
    }

    async fn fixture(responses: Vec<String>) -> (Url, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let mut responses = VecDeque::from(responses);
            while let Some(response) = responses.pop_front() {
                let (mut stream, _) = listener.accept().await.expect("accept request");
                let mut request = [0; 1024];
                let _ = stream.read(&mut request).await.expect("read request");
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
            }
        });
        (
            Url::parse(&format!("http://example.test:{}/", address.port())).expect("fixture URL"),
            server,
        )
    }

    async fn delayed_fixture() -> (Url, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await.expect("read request");
            sleep(Duration::from_millis(50)).await;
            stream
                .write_all(http_response("200 OK", "text/plain", b"late").as_bytes())
                .await
                .expect("write response");
        });
        (
            Url::parse(&format!("http://example.test:{}/", address.port())).expect("fixture URL"),
            server,
        )
    }

    fn http_response(status: &str, content_type: &str, body: &[u8]) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        )
    }
}
