use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

fn server() -> Child {
    Command::new(env!("CARGO_BIN_EXE_xngmcp"))
        .args(["--log-level", "debug", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start xngmcp MCP server")
}

fn wait_for_exit(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().expect("check server status") {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("stop stalled server");
            panic!("stdio server did not stop promptly");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn stdio_eof_keeps_verbose_logs_out_of_mcp_stdout() {
    let mut child = server();
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "stdio-test", "version": "1.0" }
        }
    });
    let mut stdin = child.stdin.take().expect("server stdin");
    writeln!(stdin, "{initialize}").expect("send initialize request");
    drop(stdin);

    assert!(wait_for_exit(&mut child).success());

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("server stdout")
        .read_to_string(&mut stdout)
        .expect("read MCP stdout");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("server stderr")
        .read_to_string(&mut stderr)
        .expect("read diagnostics");

    let messages = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("stdout line is MCP JSON"))
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|message| message["id"] == 1));
    assert!(!stdout.contains("starting stdio MCP server"));
    assert!(stderr.contains("starting stdio MCP server"));
}

#[cfg(unix)]
#[test]
fn stdio_server_stops_promptly_on_sigterm() {
    let mut child = server();
    let mut stdin = child.stdin.take().expect("server stdin");
    writeln!(
        stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "stdio-test", "version": "1.0" }
            }
        })
    )
    .expect("send initialize request");
    let mut stdout = BufReader::new(child.stdout.take().expect("server stdout"));
    let mut response = String::new();
    stdout
        .read_line(&mut response)
        .expect("read initialize response");
    assert!(serde_json::from_str::<Value>(&response).is_ok());

    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("send SIGTERM");
    assert!(status.success());
    drop(stdin);
    assert!(wait_for_exit(&mut child).success());
}
