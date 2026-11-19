//! One purpose-built subprocess runner: runs yt-dlp with a completely
//! fixed, all-literal command line and feeds it a single URL over stdin
//! (batch-file mode). Nothing that varies at runtime — no path fragment,
//! no URL, no option — is ever part of the argument vector or the program
//! name. The binary is resolved by the OS from PATH (or the working
//! directory on Windows).

// `-J` (dump-json) instead of `-g`: the JSON carries the format's
// http_headers, which googlevideo URLs bound to a non-browser client (for
// example c=VISIONOS) require on replay. A bare URL with the wrong UA
// answers 403.
const FIXED_ARGV: [&str; 6] = ["-f", "140", "--no-warnings", "-J", "--batch-file", "-"];

/// Run the resolver and return its captured output. `url_line` goes to the
/// child's stdin only.
pub async fn run_with_stdin_line(url_line: &str) -> std::io::Result<std::process::Output> {
    let mut child = tokio::process::Command::new("yt-dlp")
        .args(FIXED_ARGV)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(url_line.as_bytes()).await?;
        stdin.shutdown().await?;
    }
    child.wait_with_output().await
}
