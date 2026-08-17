use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xngmcp"))
}

#[test]
fn cli_search_json_and_plain_keep_results_on_stdout() {
    let (url, server) = searxng_fixture();
    let output = binary()
        .args([
            "--searxng-url",
            &url,
            "--log-level",
            "debug",
            "search",
            "--json",
            "rust MCP",
        ])
        .output()
        .expect("run JSON search");
    let request = server.join().expect("fixture completes");

    assert!(output.status.success());
    assert!(request.contains("q=rust+MCP"));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(json["results"][0]["title"], "Result title");
    assert!(String::from_utf8_lossy(&output.stderr).contains("resolved application settings"));

    let (url, server) = searxng_fixture();
    let output = binary()
        .args(["--searxng-url", &url, "search", "--plain", "rust MCP"])
        .output()
        .expect("run plain search");
    let request = server.join().expect("fixture completes");

    assert!(output.status.success());
    assert!(request.contains("q=rust+MCP"));
    assert_eq!(
        String::from_utf8(output.stdout).expect("plain output is UTF-8"),
        "Result title\thttps://example.com/article\tResult snippet\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn cli_repeated_flags_reach_the_shared_search_request() {
    let (url, server) = searxng_fixture_with_body(
        r#"{"results":[{"title":"Outside","url":"https://outside.example/article","content":"Result snippet","score":1.0}]}"#,
    );
    let output = binary()
        .args([
            "--searxng-url",
            &url,
            "search",
            "--json",
            "--category",
            "general",
            "--category",
            "science",
            "--engine",
            "brave",
            "--engine",
            "duckduckgo",
            "--include-domain",
            "example.com",
            "--exclude-domain",
            "ads.example.com",
            "rust",
        ])
        .output()
        .expect("run search");
    let request = server.join().expect("fixture completes");

    assert!(output.status.success());
    assert!(request.contains("categories=general%2Cscience"));
    assert!(request.contains("engines=brave%2Cduckduckgo"));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(json["results"].as_array().is_some_and(Vec::is_empty));
}

#[test]
fn cli_validation_and_help_do_not_need_a_backend() {
    for arguments in [
        ["search"].as_slice(),
        ["search", "--limit", "21", "rust"].as_slice(),
        ["fetch", "--max-chars", "999", "https://example.com/article"].as_slice(),
        ["search", "--json", "--plain", "rust"].as_slice(),
    ] {
        let output = binary().args(arguments).output().expect("run xngmcp");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }

    for arguments in [
        ["-h"].as_slice(),
        ["--help"].as_slice(),
        ["help", "search"].as_slice(),
        ["search", "--help"].as_slice(),
        ["--version"].as_slice(),
    ] {
        let output = binary().args(arguments).output().expect("run xngmcp");
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn cli_signal_cancels_an_active_search() {
    let (url, request_started, release, server) = hanging_searxng_fixture();
    let child = binary()
        .args(["--searxng-url", &url, "search", "rust"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start search");
    request_started
        .recv_timeout(Duration::from_secs(5))
        .expect("search reaches fixture");

    let signal = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .output()
        .expect("send SIGINT");
    assert!(signal.status.success());
    let output = child.wait_with_output().expect("search exits after SIGINT");
    release.send(()).expect("release fixture");
    server.join().expect("fixture completes");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("search was cancelled"));
}

fn searxng_fixture() -> (String, thread::JoinHandle<String>) {
    searxng_fixture_with_body(
        r#"{"results":[{"title":"Result title","url":"https://example.com/article","content":"Result snippet","score":1.0}]}"#,
    )
}

fn hanging_searxng_fixture() -> (String, mpsc::Receiver<()>, mpsc::Sender<()>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
    let url = format!("http://{}", listener.local_addr().expect("fixture address"));
    let (started_sender, started_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0; 1024];
        let _ = stream.read(&mut request).expect("read request");
        started_sender.send(()).expect("signal request");
        release_receiver.recv().expect("wait for test cleanup");
    });
    (url, started_receiver, release_sender, server)
}

fn searxng_fixture_with_body(body: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
    let url = format!("http://{}", listener.local_addr().expect("fixture address"));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
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
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write response");
        String::from_utf8(request).expect("request is UTF-8")
    });
    (url, server)
}
