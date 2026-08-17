use rmcp::{
    ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use tokio_util::sync::CancellationToken;

use crate::web::search::{SearchRequest, SearchService};

/// MCP server exposing xngmcp's web-search capability.
#[derive(Debug, Clone)]
pub(crate) struct McpServer {
    search: SearchService,
    cancellation: CancellationToken,
    tool_router: ToolRouter<Self>,
}

impl McpServer {
    pub(crate) fn new(search: SearchService, cancellation: CancellationToken) -> Self {
        Self {
            search,
            cancellation,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl McpServer {
    #[tool(
        name = "web_search",
        description = "Search the public web. Use domain filters when the request needs a specific site. Results contain titles, URLs, snippets, scores, and available publication dates."
    )]
    async fn web_search(
        &self,
        Parameters(request): Parameters<SearchRequest>,
        request_cancellation: CancellationToken,
    ) -> CallToolResult {
        let cancellation = CancellationToken::new();
        let cancellation_to_trigger = cancellation.clone();
        let root_cancellation = self.cancellation.clone();
        let watcher = tokio::spawn(async move {
            tokio::select! {
                _ = request_cancellation.cancelled() => cancellation_to_trigger.cancel(),
                _ = root_cancellation.cancelled() => cancellation_to_trigger.cancel(),
            }
        });

        let result = self.search.search(request, cancellation).await;
        watcher.abort();

        match result {
            Ok(response) => {
                let result_count = response.results.len();
                let query = response.query.clone();
                let structured_content = match serde_json::to_value(response) {
                    Ok(content) => content,
                    Err(error) => {
                        return CallToolResult::error(vec![ContentBlock::text(format!(
                            "could not serialize search result: {error}"
                        ))]);
                    }
                };
                let mut result = CallToolResult::structured(structured_content);
                result.content = vec![ContentBlock::text(format!(
                    "Found {result_count} result(s) for {query}."
                ))];
                result
            }
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        }
    }
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
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    use rmcp::{
        ClientHandler, ServiceExt,
        model::{CallToolRequestParams, ClientInfo},
    };
    use serde_json::json;

    use super::*;
    use crate::output;

    #[derive(Debug, Default)]
    struct TestClient;

    impl ClientHandler for TestClient {
        fn get_info(&self) -> ClientInfo {
            ClientInfo::default()
        }
    }

    #[tokio::test]
    async fn web_search_mcp_discovers_schema_and_keeps_sessions_usable_after_errors()
    -> anyhow::Result<()> {
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
            CancellationToken::new(),
        );
        let server_task = tokio::spawn(async move {
            server.serve(server_transport).await?.waiting().await?;
            anyhow::Ok(())
        });
        let client = TestClient.serve(client_transport).await?;

        let tools = client.list_tools(Default::default()).await?;
        assert_eq!(tools.tools.len(), 1);
        let tool = &tools.tools[0];
        assert_eq!(tool.name, "web_search");
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

        let backend_failure = client
            .call_tool(tool_call(json!({ "query": "rust" })))
            .await?;
        assert_eq!(backend_failure.is_error, Some(true));
        assert!(
            backend_failure.content[0]
                .as_text()
                .is_some_and(|text| text.text.contains("SearXNG returned HTTP 503"))
        );

        let result = client
            .call_tool(tool_call(json!({ "query": "rust" })))
            .await?;
        assert_eq!(result.is_error, Some(false));
        let structured = result
            .structured_content
            .expect("successful tool result includes structured content");
        let expected = SearchService::with_default_timeout(url.parse()?)?;
        let expected = expected
            .search(SearchRequest::new("rust"), CancellationToken::new())
            .await?;
        let mut cli_json = Vec::new();
        output::write_json(&mut cli_json, &expected)?;
        assert_eq!(
            structured,
            serde_json::from_slice::<serde_json::Value>(&cli_json)?
        );

        client.cancel().await?;
        server_task.await??;
        backend.join().expect("fixture completes");
        Ok(())
    }

    #[tokio::test]
    async fn web_search_mcp_passes_request_cancellation_to_search() -> anyhow::Result<()> {
        let (url, request_started, release, backend) = hanging_fixture();
        let server = McpServer::new(
            SearchService::with_default_timeout(url.parse()?)?,
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
        CallToolRequestParams::new("web_search").with_arguments(
            arguments
                .as_object()
                .expect("tool arguments are an object")
                .clone(),
        )
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
