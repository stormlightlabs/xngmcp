use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xngmcp"))
}

#[test]
fn application_shell_help_and_version_use_stdout_without_a_backend() {
    for arguments in [
        ["--help"].as_slice(),
        ["--version"].as_slice(),
        ["serve", "--help"].as_slice(),
    ] {
        let output = binary().args(arguments).output().expect("run xngmcp");

        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn application_shell_usage_errors_use_stderr() {
    let output = binary().arg("unknown-command").output().expect("run xngmcp");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
}

#[test]
fn application_shell_invalid_configuration_fails_before_serve_starts() {
    for arguments in [
        ["--searxng-url", "file:///tmp/searxng", "serve"].as_slice(),
        ["--log-level", "loud", "serve"].as_slice(),
    ] {
        let output = binary().args(arguments).output().expect("run xngmcp");

        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("xngmcp: "));
    }
}
