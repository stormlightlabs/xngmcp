use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    xngmcp::run().await
}
