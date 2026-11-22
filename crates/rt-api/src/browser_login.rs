//! One-click browser sign-in, two windows.
//!
//! Google's sign-in page blocks password entry ("This browser or app
//! may not be secure") whenever the browser runs a DevTools debugging
//! server — an attached client is not even required; the open port
//! alone trips it (verified live 2026-11-21, twice: attached and
//! unattached). And an external process can only read a browser's
//! httpOnly session cookies through that same DevTools channel. So the
//! login and the harvest must be different browser starts:
//!
//! 1. **Login window** — opens on music.youtube.com on the app's own
//!    profile with *no* debugging flags at all. To Google it is an
//!    ordinary browser; signing in works normally. The app watches the
//!    profile's cookie database *file* for the `SAPISID` cookie name
//!    (stored as plain bytes inside the SQLite b-tree even though
//!    values are encrypted) — the login signal, with no debugger
//!    anywhere near the browser. The user closes this window when done
//!    signing in; the profile's `lockfile` (held by Chromium while it
//!    runs) tells the app that happened.
//! 2. **Harvest window** — the same profile relaunches briefly *with*
//!    the debugging port; by then the password step is over and Google
//!    does not re-check. The app reads the youtube.com cookies through
//!    the browser's own cookie API and closes this window itself via
//!    `Browser.close`.
//!
//! The profile lives under `%LOCALAPPDATA%\bzvibs\login-profile` and is
//! kept between runs, so a later re-login often starts already signed
//! in and finishes in seconds. Chrome refuses `--remote-debugging-port`
//! on the default profile precisely so an app can never silently watch
//! the user's real browsing; using our own profile is the honest
//! version of that contract. The user's default browser is preferred
//! when it is a Chromium this code can drive (Chrome or Edge); the
//! default browser's own cookies can never be read from outside
//! (App-Bound Encryption), which is why a dedicated window exists.
//!
//! Windows detail: `chrome.exe`/`msedge.exe` are stubs that re-exec the
//! versioned browser and exit, so the spawned `Child` handle is NOT a
//! liveness oracle — liveness comes from the profile lockfile, and the
//! harvest browser is closed through CDP, never through the handle.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The page the sign-in window opens on.
const LOGIN_URL: &str = "https://music.youtube.com/";

/// A live sign-in flow: the profile under management plus the
/// cancellation flag. Shared as `Arc` so the UI can cancel while the
/// wait task polls.
pub struct LoginBrowser {
    profile: PathBuf,
    /// Set once the harvest browser is up, so `cancel` can close it.
    harvest_endpoint: Arc<Mutex<Option<(u16, String)>>>,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl std::fmt::Debug for LoginBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginBrowser").field("profile", &self.profile).finish()
    }
}

const MISSING_BROWSER: &str = "couldn't find Chrome or Edge to open the sign-in \
     window. Install either browser (standard install location), or use \
     Advanced options to paste a cookie manually.";

/// The dedicated login profile directory, created on demand.
fn login_profile_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    Ok(base.join("bzvibs").join("login-profile"))
}

/// The ProgId of the user's default HTTPS browser, if the registry
/// offers one. `ChromeHTML…` means Chrome, `MSEdgeHTM…` means Edge.
fn default_browser_progid() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice")
        .ok()?;
    key.get_value::<String, _>("ProgId").ok()
}

/// Chromium's cookie database inside a profile. The value blobs are
/// encrypted, but the *names* (including `SAPISID`) sit in the SQLite
/// b-tree as plain bytes — enough to detect "the login happened"
/// without a DevTools connection that Google's sign-in page could
/// detect.
fn cookie_db_path(profile: &Path) -> PathBuf {
    profile.join("Default").join("Network").join("Cookies")
}

/// Chromium holds `<profile>\lockfile` open (delete-sharing denied)
/// for as long as the browser runs on that profile. Probing it by
/// deletion is therefore a liveness oracle that survives the stub-exit
/// problem: busy ⇒ alive, removable ⇒ browser gone. Deleting the
/// stale lockfile of a dead browser is harmless — Chromium recreates
/// it on next start.
fn profile_locked(profile: &Path) -> bool {
    match std::fs::remove_file(profile.join("lockfile")) {
        Ok(()) => false,
        Err(e) => matches!(e.raw_os_error(), Some(32) | Some(5)),
    }
}

/// Pure scan: do these raw cookie-database bytes contain a SAPISID
/// cookie? Kept pure for the unit test.
fn cookie_db_has_session(bytes: &[u8]) -> bool {
    const NEEDLE: &[u8] = b"SAPISID";
    bytes.windows(NEEDLE.len()).any(|w| w == NEEDLE)
}

/// `DevToolsActivePort` holds two lines: the port and the browser's
/// WebSocket path. Kept pure for the unit test.
fn parse_devtools_port(contents: &str) -> Option<(u16, String)> {
    let mut lines = contents.lines();
    let port = lines.next()?.trim().parse::<u16>().ok()?;
    let ws_path = lines.next()?.trim().to_string();
    (port > 0 && ws_path.starts_with('/')).then_some((port, ws_path))
}

/// Poll for the port file: the browser writes it once it is ready.
fn wait_for_devtools_port(profile: &Path) -> Option<(u16, String)> {
    let path = profile.join("DevToolsActivePort");
    for _ in 0..100 {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Some(found) = parse_devtools_port(&contents) {
                return Some(found);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// Shared decoration for the fixed launch sites. The executable is
/// always a fixed literal (see the call sites); the one derived
/// argument is `--user-data-dir`, an absolute app-built path — required
/// because Edge/Chrome re-exec themselves on Windows and drop the
/// working directory, and the two-argument form is silently ignored
/// (verified live 2026-11-21). `debug_port` is passed only for the
/// harvest window; the login window must carry no debugging flags at
/// all or Google refuses the sign-in.
fn arm_login_window(mut cmd: Command, profile: &Path, debug_port: bool) -> Result<Child, String> {
    let profile_arg = profile
        .as_os_str()
        .to_string_lossy()
        .trim_end_matches('\\')
        .to_string();
    cmd.arg(format!("--user-data-dir={profile_arg}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg(LOGIN_URL);
    if debug_port {
        cmd.arg("--remote-debugging-port=0");
    }
    cmd.spawn()
        .map_err(|e| format!("couldn't open the sign-in browser: {e}"))
}

/// Launch a window. The executable is one of four fixed absolute
/// literals. Group order honours the user's default browser when it is
/// Chrome or Edge (the two this flow can drive); nothing environment-
/// or user-derived ever becomes the program.
fn open_login_command(profile: &Path, debug_port: bool) -> Result<Child, String> {
    let edge_first = default_browser_progid().is_some_and(|p| p.contains("MSEdgeHTM"));
    let chrome_first = |profile: &Path| -> Result<Child, String> {
        if Path::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe").exists() {
            return arm_login_window(
                Command::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
                profile,
                debug_port,
            );
        }
        if Path::new(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe").exists() {
            return arm_login_window(
                Command::new(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
                profile,
                debug_port,
            );
        }
        Err(MISSING_BROWSER.to_string())
    };
    let edge_first_group = |profile: &Path| -> Result<Child, String> {
        if Path::new(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe").exists() {
            return arm_login_window(
                Command::new(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
                profile,
                debug_port,
            );
        }
        if Path::new(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe").exists() {
            return arm_login_window(
                Command::new(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"),
                profile,
                debug_port,
            );
        }
        Err(MISSING_BROWSER.to_string())
    };
    if edge_first {
        edge_first_group(profile).or_else(|_| chrome_first(profile))
    } else {
        chrome_first(profile).or_else(|_| edge_first_group(profile))
    }
}

/// Start the sign-in flow: the plain login window, no debugging flags.
pub fn open_login_browser() -> Result<LoginBrowser, String> {
    let profile = login_profile_dir()?;
    std::fs::create_dir_all(&profile).map_err(|e| format!("login profile: {e}"))?;
    // Stale artifacts from a previous crash would mislead the harvest
    // discovery, and a leftover lockfile reads as "browser running".
    let _ = std::fs::remove_file(profile.join("DevToolsActivePort"));
    let _ = std::fs::remove_file(profile.join("lockfile"));
    let child = open_login_command(&profile, false)?;
    Ok(LoginBrowser {
        profile,
        harvest_endpoint: Arc::new(Mutex::new(None)),
        child: Arc::new(Mutex::new(Some(child))),
        cancelled: Arc::new(AtomicBool::new(false)),
    })
}

/// Assemble a request-header cookie string out of a CDP cookie array,
/// keeping only `youtube.com` cookies (the music session lives there;
/// accounts.google.com cookies are neither needed nor wanted).
fn cookie_header_for(cookies: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    let Some(arr) = cookies.as_array() else {
        return String::new();
    };
    for c in arr {
        let domain = c.get("domain").and_then(|d| d.as_str()).unwrap_or("");
        if domain != "youtube.com" && !domain.ends_with(".youtube.com") {
            continue;
        }
        let name = c.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() || value.is_empty() {
            continue;
        }
        parts.push(format!("{name}={value}"));
    }
    parts.join("; ")
}

type CdpSocket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

/// One CDP round trip: send a command, skip events until the matching
/// response id arrives. `None` means the method errored or the socket
/// died — callers treat that as "try again next poll".
fn send_cdp(ws: &mut CdpSocket, id: u64, method: &str) -> Option<serde_json::Value> {
    let request = serde_json::json!({"id": id, "method": method});
    ws.send(tungstenite::Message::text(request.to_string())).ok()?;
    for _ in 0..16 {
        match ws.read() {
            Ok(tungstenite::Message::Text(text)) => {
                let value: serde_json::Value = serde_json::from_str(&text).ok()?;
                if value.get("id").and_then(|i| i.as_u64()) == Some(id) {
                    return value.get("result").cloned();
                }
                // A CDP event between request and response — skip it.
            }
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Read the browser's youtube.com cookies once. `Ok(None)` = signed out
/// so far (no SAPISID yet); `Err` = transport hiccup to retry.
fn fetch_youtube_cookie(port: u16, ws_path: &str) -> Result<Option<String>, String> {
    let url = format!("ws://127.0.0.1:{port}{ws_path}");
    let (mut socket, _response) =
        tungstenite::connect(url).map_err(|e| format!("cdp connect: {e}"))?;
    // Storage.getCookies works on the browser-level target with no prior
    // enable; older builds only implement Network.getAllCookies.
    let cookies = send_cdp(&mut socket, 1, "Storage.getCookies")
        .or_else(|| send_cdp(&mut socket, 2, "Network.getAllCookies"))
        .and_then(|result| result.get("cookies").cloned())
        .ok_or_else(|| "cdp cookie query failed".to_string())?;
    let header = cookie_header_for(&cookies);
    Ok(header.contains("SAPISID=").then_some(header))
}

/// Ask the browser to shut itself down over CDP — the only reliable
/// close, since the spawned handle may already be a dead stub.
fn close_browser_via_cdp(port: u16, ws_path: &str) {
    let url = format!("ws://127.0.0.1:{port}{ws_path}");
    let Ok((mut socket, _)) = tungstenite::connect(url) else {
        return;
    };
    let _ = send_cdp(&mut socket, 1, "Browser.close");
}

/// Liveness of the *harvest* window: its plain HTTP info endpoint
/// answers while it runs. This is not the DevTools WebSocket and
/// attaches no debugger to any target.
async fn harvest_alive(port: u16) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(1200))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.get(format!("http://127.0.0.1:{port}/json/version")).send().await {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

impl LoginBrowser {
    /// The user gave up (or the flow finished): close whichever window
    /// is open. The login window has no control channel, so its part
    /// of the message tells the user to close it by hand.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some((port, ws_path)) = self
            .harvest_endpoint
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
        {
            close_browser_via_cdp(port, &ws_path);
        }
        self.kill_stub();
    }

    fn kill_stub(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Run the whole flow: wait for the Google login in the plain
    /// window, wait for the user to close it, then open the harvest
    /// window, read the session cookies, and close it. Returns the
    /// cookie header ready for [`crate::AuthedClient::from_browser_cookie`].
    pub async fn wait_for_cookie(&self, timeout: Duration) -> Result<String, String> {
        // ---- phase 1: plain window, watch the cookie database ----
        let deadline = Instant::now() + timeout;
        // The browser needs a few seconds to create its lockfile; do
        // not declare it dead before it had a chance to start.
        let started_by = Instant::now() + Duration::from_secs(20);
        loop {
            if self.cancelled() {
                return Err(
                    "sign-in cancelled — you can close the sign-in browser window".to_string(),
                );
            }
            if !profile_locked(&self.profile) && Instant::now() > started_by {
                self.kill_stub();
                // A graceful close flushes cookies to the database at
                // shutdown — the SAPISID the scan missed in memory can
                // land right after the browser exits (seen live
                // 2026-11-21: the first attempt after a real login
                // reported failure while the session was in fact there;
                // the second attempt harvested instantly). Re-read a
                // few times before declaring the login unfinished.
                for _ in 0..4 {
                    tokio::time::sleep(Duration::from_millis(700)).await;
                    if let Ok(bytes) = std::fs::read(cookie_db_path(&self.profile)) {
                        if cookie_db_has_session(&bytes) {
                            // Phase 1b passes immediately: the profile
                            // is already unlocked.
                            return self.finish_harvest(timeout).await;
                        }
                    }
                }
                return Err(
                    "the sign-in browser was closed before the Google login finished".to_string(),
                );
            }
            if Instant::now() >= deadline {
                self.cancel();
                return Err(
                    "sign-in timed out — try again and finish the Google login in the window"
                        .to_string(),
                );
            }
            match std::fs::read(cookie_db_path(&self.profile)) {
                Ok(bytes) if cookie_db_has_session(&bytes) => break,
                // The database appears only once the first cookie is
                // written; a sharing violation while Chromium rewrites
                // it just retries on the next poll.
                _ => {}
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }

        self.finish_harvest(timeout).await
    }

    /// Phases 1b and 2: wait for the login window to close, then open
    /// the harvest window and read the session cookies.
    async fn finish_harvest(&self, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        // ---- phase 1b: the user closes the login window ----
        loop {
            if self.cancelled() {
                return Err(
                    "sign-in cancelled — you can close the sign-in browser window".to_string(),
                );
            }
            if !profile_locked(&self.profile) {
                break;
            }
            if Instant::now() >= deadline {
                self.cancel();
                return Err(
                    "you're signed in — close the sign-in browser window so bzvibs can connect, \
                     then press Sign in again if this times out"
                        .to_string(),
                );
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // ---- phase 2: harvest window with the debug port ----
        // The profile is free now, but a hard window close can leave
        // dying child processes holding it for another moment — retry
        // the launch a few times before giving up.
        let mut endpoint = None;
        for _ in 0..3 {
            if self.cancelled() {
                self.cancel();
                return Err(
                    "sign-in cancelled — you can close the sign-in browser window".to_string(),
                );
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
            let _ = std::fs::remove_file(self.profile.join("DevToolsActivePort"));
            match open_login_command(&self.profile, true) {
                Ok(harvest_child) => {
                    if let Ok(mut guard) = self.child.lock() {
                        *guard = Some(harvest_child);
                    }
                    if let Some(found) = wait_for_devtools_port(&self.profile) {
                        endpoint = Some(found);
                        break;
                    }
                }
                Err(e) => return Err(e),
            }
        }
        let Some((port, ws_path)) = endpoint else {
            self.cancel();
            return Err(
                "signed in, but the session window could not be opened; try again".to_string(),
            );
        };
        if let Ok(mut guard) = self.harvest_endpoint.lock() {
            *guard = Some((port, ws_path.clone()));
        }
        let harvest_deadline = Instant::now() + Duration::from_secs(40);
        // A just-started browser may briefly not answer its info
        // endpoint; only report it dead once it answered at least once.
        let mut seen_alive = false;
        loop {
            if self.cancelled() {
                self.cancel();
                return Err(
                    "sign-in cancelled — you can close the sign-in browser window".to_string(),
                );
            }
            if harvest_alive(port).await {
                seen_alive = true;
            } else if seen_alive {
                self.cancel();
                return Err("the session window closed too early; try again".to_string());
            }
            if Instant::now() >= harvest_deadline {
                self.cancel();
                return Err(
                    "signed in, but the session could not be read in time; try again".to_string(),
                );
            }
            match fetch_youtube_cookie(port, &ws_path) {
                Ok(Some(cookie)) => {
                    self.cancel();
                    return Ok(cookie);
                }
                // The file scan is a heuristic; the CDP jar is truth.
                // If the jar disagrees, keep waiting — Chrome may still
                // be restoring the session.
                Ok(None) | Err(_) => {}
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devtools_port_file_parses_port_and_ws_path() {
        let parsed = parse_devtools_port("9222\n/devtools/browser/abc-123\n");
        assert_eq!(parsed, Some((9222, "/devtools/browser/abc-123".to_string())));
        assert_eq!(parse_devtools_port(""), None);
        assert_eq!(parse_devtools_port("not-a-port\n/x"), None);
    }

    #[test]
    fn cookie_header_keeps_youtube_only_and_skips_empties() {
        let jar = serde_json::json!([
            {"name": "YSC", "value": "v1", "domain": ".youtube.com"},
            {"name": "SAPISID", "value": "v2", "domain": "youtube.com"},
            {"name": "SID", "value": "v3", "domain": ".youtube.com"},
            {"name": "NID", "value": "v4", "domain": ".google.com"},
            {"name": "EMPTY", "value": "", "domain": ".youtube.com"},
        ]);
        let header = cookie_header_for(&jar);
        assert!(header.contains("YSC=v1"), "{header}");
        assert!(header.contains("SAPISID=v2"), "{header}");
        assert!(header.contains("SID=v3"), "{header}");
        assert!(!header.contains("NID"), "{header}");
        assert!(!header.contains("EMPTY"), "{header}");
    }

    #[test]
    fn an_empty_jar_is_not_a_session() {
        assert_eq!(cookie_header_for(&serde_json::json!([])), "");
    }

    #[test]
    fn cookie_db_scan_finds_the_sapisid_cookie_name_in_raw_sqlite_bytes() {
        // A trimmed but honest slice of what a Chromium cookie DB looks
        // like: table text plus a row whose name column holds SAPISID.
        let signed_in: &[u8] = b"SQLite format 3\x00cookies\x00meta\x00SAPISID\x00expiry";
        assert!(cookie_db_has_session(signed_in));
        let signed_out: &[u8] = b"SQLite format 3\x00cookies\x00meta\x00YSC\x00expiry";
        assert!(!cookie_db_has_session(signed_out));
        assert!(!cookie_db_has_session(b""));
    }

    #[test]
    fn lockfile_probe_maps_errors_to_running_state() {
        // The decision layer of profile_locked: a sharing violation or
        // access denial means the browser holds the lockfile and is
        // running; a successful delete (or a missing file) means not.
        use std::io;
        let busy = io::Error::from_raw_os_error(32); // ERROR_SHARING_VIOLATION
        let denied = io::Error::from_raw_os_error(5); // ERROR_ACCESS_DENIED
        let gone = io::Error::from_raw_os_error(2); // ERROR_FILE_NOT_FOUND
        let map = |res: io::Result<()>| match res {
            Ok(()) => false,
            Err(e) => matches!(e.raw_os_error(), Some(32) | Some(5)),
        };
        assert!(map(Err(busy)));
        assert!(map(Err(denied)));
        assert!(!map(Err(gone)));
        assert!(!map(Ok(())));
        let _ = profile_locked; // keep the impure wrapper linked
    }
}

