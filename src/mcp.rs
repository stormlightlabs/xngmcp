use rmcp::{
    ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::web::{
    fetch::{FetchRequest, FetchResponse, FetchService},
    search::{SearchRequest, SearchResponse, SearchService},
};

/// MCP server exposing xngmcp's public web tools.
#[derive(Debug, Clone)]
pub(crate) struct McpServer {
    search: SearchService,
    fetch: FetchService,
    cancellation: CancellationToken,
    tool_router: ToolRouter<Self>,
}

impl McpServer {
    pub(crate) fn new(search: SearchService, fetch: FetchService, cancellation: CancellationToken) -> Self {
        Self { search, fetch, cancellation, tool_router: Self::tool_router() }
    }

    fn request_cancellation(&self, request_cancellation: CancellationToken) -> (CancellationToken, JoinHandle<()>) {
        let cancellation = CancellationToken::new();
        let cancellation_to_trigger = cancellation.clone();
        let root_cancellation = self.cancellation.clone();
        let watcher = tokio::spawn(async move {
            tokio::select! {
                _ = request_cancellation.cancelled() => cancellation_to_trigger.cancel(),
                _ = root_cancellation.cancelled() => cancellation_to_trigger.cancel(),
            }
        });
        (cancellation, watcher)
    }
}

#[tool_router(router = tool_router)]
impl McpServer {
    #[tool(
        name = "web_search",
        description = "Search the public web. Use domain filters when the request needs a specific site. Results contain titles, URLs, snippets, scores, and available publication dates."
    )]
    async fn web_search(
        &self, Parameters(request): Parameters<SearchRequest>, request_cancellation: CancellationToken,
    ) -> CallToolResult {
        let (cancellation, watcher) = self.request_cancellation(request_cancellation);
        let result = self.search.search(request, cancellation).await;
        watcher.abort();

        match result {
            Ok(response) => {
                let text_content = search_text_fallback(&response);
                let structured_content = match serde_json::to_value(response) {
                    Ok(content) => content,
                    Err(error) => {
                        return CallToolResult::error(vec![ContentBlock::text(format!(
                            "could not serialize search result: {error}"
                        ))]);
                    }
                };
                let mut result = CallToolResult::structured(structured_content);
                result.content = vec![ContentBlock::text(text_content)];
                result
            }
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }

    #[tool(
        name = "web_fetch",
        description = "Fetch bounded, readable Markdown or text from a public HTTP(S) URL. Use a returned search URL when possible. Local networks, unsupported media, and pages without a readable article are rejected."
    )]
    async fn web_fetch(
        &self, Parameters(request): Parameters<FetchRequest>, request_cancellation: CancellationToken,
    ) -> CallToolResult {
        let (cancellation, watcher) = self.request_cancellation(request_cancellation);
        let result = self.fetch.fetch(request, cancellation).await;
        watcher.abort();

        match result {
            Ok(response) => {
                let text_content = fetch_text_fallback(&response);
                let structured_content = match serde_json::to_value(response) {
                    Ok(content) => content,
                    Err(error) => {
                        return CallToolResult::error(vec![ContentBlock::text(format!(
                            "could not serialize fetch result: {error}"
                        ))]);
                    }
                };
                let mut result = CallToolResult::structured(structured_content);
                result.content = vec![ContentBlock::text(text_content)];
                result
            }
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }
}

fn search_text_fallback(response: &SearchResponse) -> String {
    if response.results.is_empty() {
        return format!("No results for {}.", response.query);
    }

    let results = response
        .results
        .iter()
        .enumerate()
        .map(|(index, result)| format!("{}. {}\n{}\n{}", index + 1, result.title, result.url, result.snippet))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("Search results for {}:\n\n{results}", response.query)
}

fn fetch_text_fallback(response: &FetchResponse) -> String {
    let title = response
        .title
        .as_deref()
        .map(|title| format!("# {title}\n\n"))
        .unwrap_or_default();
    let truncated = if response.truncated { "\n\n[Content truncated.]" } else { "" };

    format!(
        "Source: {}\nContent type: {}\n\n{title}{}{truncated}",
        response.url, response.content_type, response.content
    )
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("xngmcp", env!("CARGO_PKG_VERSION")))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{SocketAddr, TcpListener},
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    use reqwest::{
        Client,
        dns::{Addrs, Name, Resolve, Resolving},
        redirect::Policy,
    };
    use rmcp::{
        ClientHandler, ServiceExt,
        model::{CallToolRequestParams, ClientInfo},
    };
    use serde_json::json;

    use super::*;
    use crate::{output, web::fetch::FetchFormat};

    #[derive(Debug, Default)]
    struct TestClient;

    impl ClientHandler for TestClient {
        fn get_info(&self) -> ClientInfo {
            ClientInfo::default()
        }
    }

    #[tokio::test]
    async fn web_search_mcp_discovers_schema_and_keeps_sessions_usable_after_errors() -> anyhow::Result<()> {
        let (url, backend) = fixture_server(vec![
            (503, r#"{"error":"unavailable"}"#),
            (
                200,
                r#"{"results":[{"title":"Result title","url":"https://example.com/article","content":"Result snippet","score":1.0}]}"#,
            ),
            (
                200,
                r#"{"results":[{"title":"Result title","url":"https://example.com/article","content":"Result snippet","score":1.0}]}"#,
            ),
        ]);
        let (server_transport, client_transport) = tokio::io::duplex(8_192);
        let server = McpServer::new(
            SearchService::with_default_timeout(url.parse()?)?,
            FetchService::with_default_timeout()?,
            CancellationToken::new(),
        );
        let server_task = tokio::spawn(async move {
            server.serve(server_transport).await?.waiting().await?;
            anyhow::Ok(())
        });
        let client = TestClient.serve(client_transport).await?;

        let tools = client.list_tools(Default::default()).await?;
        assert_eq!(tools.tools.len(), 2);
        let tool = tools
            .tools
            .iter()
            .find(|tool| tool.name == "web_search")
            .expect("web_search is discovered");
        let properties = tool.input_schema["properties"]
            .as_object()
            .expect("tool schema has properties");
        assert_eq!(properties["limit"]["maximum"], 20);
        assert_eq!(properties["page"]["maximum"], 10);
        assert_eq!(properties["query"]["maxLength"], 1_000);
        assert_eq!(properties["categories"]["maxItems"], 10);
        assert!(
            properties["limit"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("Defaults to 8"))
        );
        assert!(
            properties["categories"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("Defaults to"))
        );
        assert!(
            properties["safe_search"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("Defaults to 1"))
        );
        assert!(
            tool.input_schema["required"]
                .as_array()
                .is_some_and(|fields| fields.contains(&json!("query")))
        );

        let validation_failure = client.call_tool(tool_call(json!({ "query": " " }))).await?;
        assert_eq!(validation_failure.is_error, Some(true));
        assert!(
            validation_failure.content[0]
                .as_text()
                .is_some_and(|text| text.text.contains("query must not be blank"))
        );

        let backend_failure = client.call_tool(tool_call(json!({ "query": "rust" }))).await?;
        assert_eq!(backend_failure.is_error, Some(true));
        assert!(
            backend_failure.content[0]
                .as_text()
                .is_some_and(|text| text.text.contains("SearXNG returned HTTP 503"))
        );

        let result = client.call_tool(tool_call(json!({ "query": "rust" }))).await?;
        assert_eq!(result.is_error, Some(false));
        let structured = result
            .structured_content
            .expect("successful tool result includes structured content");
        assert!(
            result.content[0]
                .as_text()
                .is_some_and(|text| text.text.contains("https://example.com/article"))
        );
        let expected = SearchService::with_default_timeout(url.parse()?)?;
        let expected = expected
            .search(SearchRequest::new("rust"), CancellationToken::new())
            .await?;
        let mut cli_json = Vec::new();
        output::write_json(&mut cli_json, &expected)?;
        assert_eq!(structured, serde_json::from_slice::<serde_json::Value>(&cli_json)?);

        client.cancel().await?;
        server_task.await??;
        backend.join().expect("fixture completes");
        Ok(())
    }

    #[tokio::test]
    async fn web_fetch_mcp_uses_shared_schema_and_response() -> anyhow::Result<()> {
        let (url, fetch, backend) = fetch_fixture(vec![
            plain_text_response("Readable article content."),
            plain_text_response("Readable article content."),
        ]);
        let expected_fetch = fetch.clone();
        let (server_transport, client_transport) = tokio::io::duplex(8_192);
        let server = McpServer::new(
            SearchService::with_default_timeout("http://127.0.0.1:8080".parse()?)?,
            fetch,
            CancellationToken::new(),
        );
        let server_task = tokio::spawn(async move {
            server.serve(server_transport).await?.waiting().await?;
            anyhow::Ok(())
        });
        let client = TestClient.serve(client_transport).await?;

        let tools = client.list_tools(Default::default()).await?;
        let mut tool_names = tools.tools.iter().map(|tool| tool.name.to_string()).collect::<Vec<_>>();
        tool_names.sort();
        assert_eq!(tool_names, ["web_fetch", "web_search"]);
        let tool = tools
            .tools
            .iter()
            .find(|tool| tool.name == "web_fetch")
            .expect("web_fetch is discovered");
        let properties = tool.input_schema["properties"]
            .as_object()
            .expect("tool schema has properties");
        assert_eq!(properties["max_chars"]["minimum"], 1_000);
        assert_eq!(properties["max_chars"]["maximum"], 100_000);
        assert!(
            properties["format"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("Defaults to `markdown`"))
        );
        assert!(
            tool.input_schema["required"]
                .as_array()
                .is_some_and(|fields| fields.contains(&json!("url")))
        );

        let validation_failure = client
            .call_tool(fetch_tool_call(json!({ "url": "file:///tmp/article" })))
            .await?;
        assert_eq!(validation_failure.is_error, Some(true));
        assert!(
            validation_failure.content[0]
                .as_text()
                .is_some_and(|text| text.text.contains("absolute HTTP or HTTPS"))
        );

        let arguments = json!({ "url": url, "max_chars": 1_000, "format": "text" });
        let result = client.call_tool(fetch_tool_call(arguments.clone())).await?;
        assert_eq!(result.is_error, Some(false));
        assert!(
            result.content[0]
                .as_text()
                .is_some_and(|text| text.text.contains("Readable article content."))
        );
        let expected = expected_fetch
            .fetch(
                FetchRequest {
                    url: arguments["url"].as_str().expect("URL argument").into(),
                    max_chars: Some(1_000),
                    format: Some(FetchFormat::Text),
                },
                CancellationToken::new(),
            )
            .await?;
        let mut cli_json = Vec::new();
        output::write_json(&mut cli_json, &expected)?;
        assert_eq!(
            result.structured_content,
            Some(serde_json::from_slice::<serde_json::Value>(&cli_json)?)
        );

        client.cancel().await?;
        server_task.await??;
        backend.join().expect("fixture completes");
        Ok(())
    }

    #[tokio::test]
    async fn web_fetch_mcp_passes_request_cancellation_to_fetch() -> anyhow::Result<()> {
        let (url, fetch, request_started, release, backend) = hanging_fetch_fixture();
        let server = McpServer::new(
            SearchService::with_default_timeout("http://127.0.0.1:8080".parse()?)?,
            fetch,
            CancellationToken::new(),
        );
        let request_cancellation = CancellationToken::new();
        let call_cancellation = request_cancellation.clone();
        let call = tokio::spawn(async move {
            server
                .web_fetch(Parameters(FetchRequest::new(url)), call_cancellation)
                .await
        });

        request_started.await?;
        request_cancellation.cancel();
        let result = call.await?;
        release.send(()).expect("release fixture");
        backend.join().expect("fixture completes");

        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .is_some_and(|text| text.text.contains("fetch was cancelled"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn web_search_mcp_passes_request_cancellation_to_search() -> anyhow::Result<()> {
        let (url, request_started, release, backend) = hanging_fixture();
        let server = McpServer::new(
            SearchService::with_default_timeout(url.parse()?)?,
            FetchService::with_default_timeout()?,
            CancellationToken::new(),
        );
        let request_cancellation = CancellationToken::new();
        let call_cancellation = request_cancellation.clone();
        let call = tokio::spawn(async move {
            server
                .web_search(Parameters(SearchRequest::new("rust")), call_cancellation)
                .await
        });

        request_started.await?;
        request_cancellation.cancel();
        let result = call.await?;
        release.send(()).expect("release fixture");
        backend.join().expect("fixture completes");

        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .is_some_and(|text| text.text.contains("search was cancelled"))
        );
        Ok(())
    }

    fn tool_call(arguments: serde_json::Value) -> CallToolRequestParams {
        CallToolRequestParams::new("web_search")
            .with_arguments(arguments.as_object().expect("tool arguments are an object").clone())
    }

    fn fetch_tool_call(arguments: serde_json::Value) -> CallToolRequestParams {
        CallToolRequestParams::new("web_fetch")
            .with_arguments(arguments.as_object().expect("tool arguments are an object").clone())
    }

    fn fetch_fixture(responses: Vec<String>) -> (String, FetchService, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept request");
                read_request(&mut stream);
                stream.write_all(response.as_bytes()).expect("write response");
            }
        });
        let client = Client::builder()
            .redirect(Policy::none())
            .dns_resolver(Arc::new(FixtureResolver { address }))
            .build()
            .expect("build fixture client");
        let service = FetchService::with_test_client(client, Duration::from_secs(1));
        (
            format!("http://public.example:{}/article", address.port()),
            service,
            server,
        )
    }

    fn hanging_fetch_fixture() -> (
        String,
        FetchService,
        tokio::sync::oneshot::Receiver<()>,
        mpsc::Sender<()>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            read_request(&mut stream);
            started_sender.send(()).expect("signal request");
            release_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("wait for test cleanup");
        });
        let client = Client::builder()
            .redirect(Policy::none())
            .dns_resolver(Arc::new(FixtureResolver { address }))
            .build()
            .expect("build fixture client");
        let service = FetchService::with_test_client(client, Duration::from_secs(1));
        (
            format!("http://public.example:{}/article", address.port()),
            service,
            started_receiver,
            release_sender,
            server,
        )
    }

    fn plain_text_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
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

    fn fixture_server(responses: Vec<(u16, &'static str)>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let url = format!("http://{}", listener.local_addr().expect("fixture address"));
        let server = thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("accept request");
                read_request(&mut stream);
                write!(
                    stream,
                    "HTTP/1.1 {status} fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len(),
                )
                .expect("write response");
            }
        });
        (url, server)
    }

    fn hanging_fixture() -> (
        String,
        tokio::sync::oneshot::Receiver<()>,
        mpsc::Sender<()>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let url = format!("http://{}", listener.local_addr().expect("fixture address"));
        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            read_request(&mut stream);
            started_sender.send(()).expect("signal request");
            release_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("wait for test cleanup");
        });
        (url, started_receiver, release_sender, server)
    }

    fn read_request(stream: &mut std::net::TcpStream) {
        let mut request = Vec::new();
        let mut buffer = [0; 1_024];
        loop {
            let read = stream.read(&mut buffer).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
    }
}
