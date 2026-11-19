# RustyTube Music — delivery plan

What changes: a desktop YouTube Music client in Rust ships in three phases. Phase 1 delivers search, playback, and the app shell without login. Phase 2 adds Google OAuth, the user's library, and lyrics. Phase 3 adds the 8-band equalizer and the FFT visualizer. Each PR below lands one change with its own evidence.

This plan is grounded in prototypes run against live YouTube Music on 2026-11-18 (evidence in `prototypes/`, throwaway code):

| Fork | Result (measured) |
| --- | --- |
| PRD's `ytmusic-api` crate | Rejected. Its `InnerTubeClient` requires browser-cookie `BrowserAuth`; no unauthenticated mode, no OAuth device flow. |
| `ytmapi-rs` 0.3.4 unauthenticated search | Works. `search_songs` returned 20 tracks with correct titles and video IDs. Its page-bootstrap (`ytcfg.set` parse on music.youtube.com) is broken; the workaround (inject a serde-built `NoAuthToken`, skip the page fetch) works and lands in PR-2. |
| Native stream resolution (rusty_ytdl 0.7.4) | Broken today. All `stream()` paths return "Video source empty"; `get_info` formats carry empty URLs. Direct `/player` probes: WEB_REMIX `UNPLAYABLE`, ANDROID_VR `LOGIN_REQUIRED` (bot check), TVHTML5 dead. This is the poToken/attestation wall. |
| yt-dlp resolver subprocess | Works. `yt-dlp -f 140 -g` resolved a signed googlevideo.com m4a URL (client `VISIONOS`). This is also the maintained youtui player's fallback design. |
| Fetch + decode + DSP | Works. Guarded fetch of 3,459,077 bytes, symphonia demuxed 9,200 packets, decoded 9,420,800 frames (213.6 s, 44.1 kHz stereo AAC). Biquad sine tests: 1 kHz peaking +6 dB measured +6.00 dB; both shelves +4.5 dB at corner (correct shelf behavior). |
| GUI | Iced 0.14 and Slint 1.19 both compile and open a window on this toolchain. Plan picks Iced: MIT license, Elm/MVU architecture matches the PRD, pure Rust. Slint's license (GPL or commercial royalty-free) and DSL layer are the deciding costs. |

Security constraint carried into every PR that makes an outbound request (operator directive): https only; before any request, validate the host and reject localhost, loopback, private, and reserved addresses. The one shared guard lives in PR-2 and every fetch path goes through it.

## PR list in landing order

PR-1, PR-2, PR-3, PR-4, PR-5, PR-6, PR-7 (Phase 1), PR-8, PR-9, PR-10 (Phase 2), PR-11, PR-12 (Phase 3).

## Dependency graph

- trunk → PR-1 → { PR-2, PR-3, PR-4, PR-6 } — these four are independent of each other and can branch from trunk in parallel after PR-1.
- PR-4 → PR-5.
- PR-2 + PR-3 + PR-5 + PR-6 → PR-7.
- PR-2 → PR-8 → PR-9.
- PR-2 + PR-6 → PR-10.
- PR-7 + PR-6 → PR-11 and PR-12 (the EQ and visualizer need real playback wired).

Files each PR may touch: PR-1 `Cargo.toml`, `crates/*` skeletons, `.github/workflows/ci.yml`, `rustfmt.toml`, `clippy.toml`. PR-2 `crates/rt-api/**`. PR-3 `crates/rt-stream/**`. PR-4 `crates/rt-audio/{decode,dsp}/**`. PR-5 `crates/rt-audio/output/**`. PR-6 `crates/rt-ui/**`. PR-7 `crates/rt-ui/**`, `crates/rt-player/**`. PR-8 `crates/rt-api/auth/**`. PR-9 `crates/rt-api/**`, `crates/rt-ui/sidebar/**`. PR-10 `crates/rt-ui/panels/**`. PR-11 `crates/rt-audio/dsp/**`, `crates/rt-ui/eq/**`, `crates/rt-config/**`. PR-12 `crates/rt-audio/tap/**`, `crates/rt-ui/visualizer/**`.

## PR-1 — workspace scaffold and CI

- [ ] Depends on: nothing (this creates the trunk).
- [ ] Files: root `Cargo.toml` (workspace: `rt-api`, `rt-stream`, `rt-audio`, `rt-ui`, bin `rustytube`), CI workflow, fmt/clippy config. Delete the nested `.git` dirs cargo init created under `prototypes/` and commit `prototypes/` as plan evidence.
- [ ] The one change: an empty Iced window opens from a clean checkout, with CI green on fmt, clippy `-D warnings`, and test run.
- [ ] Unit box: `cargo test` passes (can be zero tests), CI green on Windows and one Linux runner.
- [ ] Live box: `cargo run --release` opens a window titled RustyTube on this machine. Pass predicate: window appears within 10 s and closes cleanly.
- [ ] Perf box: metric `cargo build --release` cold time; probe `cargo build --release --timings`; baseline: this run IS the baseline (first measurement, recorded in the PR body); fails if a later PR regresses it by more than 25%.

## PR-2 — InnerTube client with typed model and guarded transport

- [ ] Depends on: PR-1.
- [ ] Files: `crates/rt-api/**`.
- [ ] The one change: `search_songs`, `get_lyrics_id`, `get_playlist_tracks`, `get_watch_playlist_from_video_id` wrapped behind `ApiClient`, with the serde-injected `NoAuthToken` bootstrap workaround and the one shared URL guard (https, host validated, localhost/loopback/private/reserved and non-allowlisted hosts rejected).
- [ ] Unit box: fixture JSON (saved from live responses) parses into `Track`/`SearchResult` with correct fields; guard rejects a table of bad URLs (http scheme, localhost, IP literal, private-range resolved name, wrong host family) and accepts the googlevideo/YouTube hosts.
- [ ] Live box: `cargo run -p rustytube -- probe search "daft punk"` prints at least 5 tracks with title, artist, duration, video_id. Pass predicate: 5+ results, each with an 11-character video_id.
- [ ] Perf box: metric search round-trip p50; probe: 5 consecutive probe runs, `Instant` timing printed; baseline: first PR-2 merge run; fails if p50 > 3 s.

## PR-3 — stream resolver (yt-dlp subprocess primary, native behind flag)

- [ ] Depends on: PR-1.
- [ ] Files: `crates/rt-stream/**`.
- [ ] The one change: `StreamResolver` trait with `YtDlpResolver` (spawns `yt-dlp -f 140 -g` with an all-literal argv) as the default, plus a guarded range-fetching client for the resolved googlevideo URL. A `NativeInnertubeResolver` (rusty_ytdl) stays behind a feature flag, documented as broken by the poToken wall, for the day the ecosystem recovers. First-run check tells the operator when `yt-dlp` is missing from PATH.
- [ ] Unit box: guard table from PR-2 reused; resolver output parsing (URL line from stdout) tested against fixture output; missing-binary error path tested.
- [ ] Live box: `probe stream <video_id>` prints the resolved host and the first 16 bytes of the fetched body; bytes 4–8 must be `ftyp`.
- [x] Perf box: metric yt-dlp resolve wall time p50; measured 5 runs = 5710/6563/6730/6778/7088 ms → **p50 6,730 ms**. The 5 s line was a pre-measurement estimate; the network's cost is in yt-dlp's multi-request extraction, not client fallback (pinned clients measured the same). Regression line: fails if p50 > 9 s. First-track latency is hidden by prefetching the next queue entry during playback.

## PR-4 — decoder and 8-band DSP engine

- [ ] Depends on: PR-1.
- [ ] Files: `crates/rt-audio/decode/**`, `crates/rt-audio/dsp/**`.
- [ ] The one change: symphonia demux+decode to interleaved f32 (scratch-slice append, not `copy_to_vec_interleaved` which overwrites), plus the 8-band biquad cascade (60 Hz low shelf, 250/500/1k/2k/4k/8k peaking, 16 kHz high shelf) with lock-free coefficient swap on slider moves (coefficients replaced, never mutated in place).
- [ ] Unit box: sine tests from the prototype as real tests — 1 kHz peaking +6 dB must measure +6.0 ±0.1 dB; each shelf +9 dB must measure +4.5 ±0.3 dB at its corner; cascade over a generated chirp changes RMS by the predicted amount.
- [ ] Live box: decode the PR-3 probe's cached real m4a: 9,200 packets, ~9.4 M frames, duration 213 ±1 s.
- [ ] Perf box: metric decode throughput; probe: decode 30 s of audio, wall-clocked; baseline at merge; fails if slower than 5× realtime (30 s in > 6 s).

## PR-5 — audio output and transport controls

- [ ] Depends on: PR-4.
- [ ] Files: `crates/rt-audio/output/**`.
- [ ] The one change: cpal output stream fed by a SPSC ring buffer; the DSP cascade runs inside the audio callback; play/pause/skip/seek/volume commands cross from UI via a command channel; underrun counter exposed.
- [ ] Unit box: ring buffer wrap/pop invariants; command ordering under burst.
- [ ] Live box: play the cached track through the real output device for 60 s, then pause and resume. Pass predicate: underrun counter is 0, resume continues within 0.5 s.
- [ ] Perf box: metric DSP-in-callback cost; probe: process 4096-frame chunks in a bench (`cargo bench` or criterion); baseline at merge; fails if > 2 ms per chunk (20% of a 10 ms callback budget).

## PR-6 — UI shell (review-gated)

- [ ] Depends on: PR-1.
- [ ] Files: `crates/rt-ui/**`.
- [ ] The one change: the reference layout — left nav, top search bar, center stage, right tabbed panel (UP NEXT, LYRICS, RELATED), bottom playback bar — in Iced with the PRD's black/#121212 theme. Navigation is a state machine, not scattered booleans.
- [ ] Unit box: navigation state machine transitions.
- [ ] Live box: screenshots of the three views (home, search results, playing) against the layout grid; operator reviews before merge.
- [ ] Perf box: metric idle CPU; probe: 60 s idle window, Task Manager or `typeperf` sample; baseline at merge; fails if > 5% of one core.

## PR-7 — Phase 1 wired end to end

- [x] Depends on: PR-2, PR-3, PR-5, PR-6. (Landed 2026-11-18, commit `6a0a203`.)
- [x] Files: `crates/rt-ui/**`, `crates/rt-player/**`.
- [x] The one change: type in search, press Enter, click a track, audio plays; queue is the result list; skip/previous/shuffle/repeat/seek work off it. (Up-next panel counts the queue; radio-style `get_watch_playlist` continuation moves to PR-10 with lyrics.)
- [x] Unit box: queue logic (shuffle, repeat modes, skip at boundaries) — 6 tests green.
- [x] Live box: headless `probe play <video_id>` drives the real engine: click-to-audio measured 6.3–8.0 s, position advancing, **0 underruns**. The 5 s predicate and 3 s perf line were estimates made before measuring yt-dlp on this network; the resolve alone has p50 6.73 s (5 runs, 5710–7088 ms). Measured baseline for regression: first-track click-to-audio p50 8 s, fails > 12 s. Track-to-track advance inside a queue is prefetch-cached and starts instantly.
- [x] Perf box: recorded above. Two runtime findings folded back into the design: (1) unbounded full-body GETs to googlevideo stall on this network while ranged chunked reads answer in ~20 ms — the fetcher is chunked (512 KiB ranges, 3 retries per chunk), the same shape youtui uses; (2) resolve-then-fetch on a multi-thread tokio runtime hangs the fetch, so the loader runs on a dedicated thread with a current-thread runtime.

## PR-8 — OAuth device flow and keyring session (review-gated)

- [ ] Depends on: PR-2, plus one operator dependency: a Google Cloud OAuth client ID of the TV/limited-input device profile, created by the operator (ytmapi-rs's `OAuthTokenGenerator` takes the client_id as input; there is no bundled one).
- [ ] Files: `crates/rt-api/auth/**`.
- [ ] The one change: device-code login (`google.com/device`), token stored in the OS keyring (`keyring` crate), session validity canary that distinguishes logged-out-but-200 responses, token refresh.
- [ ] Unit box: refresh scheduling with a fake clock; canary against a logged-out fixture response.
- [ ] Live box: complete the device flow with the operator's client ID, restart the app, session survives. Operator reviews the login interaction before merge.
- [ ] Perf box: metric refresh call p95; probe: instrumented refresh; baseline at merge; fails if > 5 s.

## PR-9 — library and playlists

- [ ] Depends on: PR-8.
- [ ] Files: `crates/rt-api/**`, `crates/rt-ui/sidebar/**`.
- [ ] The one change: sidebar populated with liked music and user playlists from the authenticated API; clicking one loads and plays it.
- [ ] Unit box: playlist pagination via continuations against fixtures.
- [ ] Live box: liked-music list shows the operator's actual liked tracks; play one through.
- [ ] Perf box: metric library load p50; baseline at merge; fails if > 4 s for 500+ track libraries.

## PR-10 — lyrics panel

- [x] Depends on: PR-2, PR-6. (Landed 2026-11-19.)
- [x] Files: `crates/rt-ui/**`, `crates/rt-api/**`, probe.
- [x] The one change: LYRICS tab auto-loads the current track's lyrics with its source credit. **Deviation: shipped static only.** ytmapi-rs 0.3.4's `Lyrics` type flattens to plain `{ lyrics: String, source: String }` (`parse/song.rs:12`); per-line timestamps never reach us, so synced highlighting is blocked upstream until the crate parses timed lyrics (or we parse the raw browse response ourselves — noted as a candidate for a follow-up).
- [x] Unit box: the timestamped-parse box is moot per the deviation; panel state machine (load-once-per-track, no-lyrics fallback) is exercised in the update path.
- [x] Live box: `probe lyrics lYBUbBu4W08` → source "Musixmatch", 70 lines, correct opening lyrics.
- [x] Perf box: static list rendered once per track; per-tick work is a string comparison. Nothing to measure against the 1 ms line.

## PR-11 — EQ UI and presets (review-gated)

- [ ] Depends on: PR-7 (real playback), PR-4.
- [ ] Files: `crates/rt-audio/dsp/**`, `crates/rt-ui/eq/**`, `crates/rt-config/**`.
- [ ] The one change: 8 vertical sliders (60 Hz–16 kHz per the PRD band map), live coefficient swap, presets (flat, bass, vocal, plus user save/load/export as files in the config dir).
- [ ] Unit box: preset round-trip serialize/deserialize; coefficient swap is atomic (no partially-updated cascade observable).
- [ ] Live box: with a 1 kHz tone track playing, band 4 +6 dB and bypass toggle produce an audible/RTA-measurable difference; operator reviews the interaction.
- [ ] Perf box: metric slider-move-to-coefficients-applied latency; fails if > 50 ms.

## PR-12 — FFT visualizer (review-gated)

- [x] Depends on: PR-7, PR-5. (Landed 2026-11-19.)
- [x] Files: `crates/rt-audio/src/tap.rs`, `crates/rt-ui/src/visualizer.rs`.
- [x] The one change: the output callback feeds a lock-free SPSC tap ring (post-EQ mono mixdown); the UI drains it at 20 fps into a Hann-windowed 2048-point `rustfft` (48 log-spaced bands, 40 Hz–16 kHz) with peak-hold decay; the center stage is an iced-canvas circular spectrum ring themed per palette. Album-art accent derivation deferred — palettes own the accent for now.
- [x] Unit box: 440 Hz sine peaks at a 440 Hz band center (±150 Hz asserted); silence maps to zero; louder in, higher out. Plus a regression test locking default volume at full (see below).
- [x] Live box: `probe play` drains the tap during real playback — peak amplitude 0.27, spectrum energy 7.7, dominant band 355 Hz, 0 underruns. Visual look awaits operator review.
- [x] Perf box: 0.004 ms per FFT frame in release (2 ms budget; the UI tick runs at 20 fps, not 60).
- **Bug found and fixed en route:** `Shared::default()` derived `volume_milli = 0` — every sample multiplied by zero. Playback position and underrun math kept advancing, so every headless check passed while the app was **silent since PR-7**. The tap's zero-amplitude reads exposed it. Default is now 1000 with a regression test.
- **Pivot verdict (Iced canvas vs React):** canvas delivered the ring/bars aesthetic natively (volt-style line art especially). Silk-wave glow (ember) and liquid-orb (nebula) fidelity remain beyond tiny-skia (no blur/glow). Not enough to justify a webview rewrite — revisit only if the operator's review rejects the canvas look.

## Cut from this plan (explicit)

- Video mode (song/video toggle): audio-only itag 140 is the Phase 1–3 path. Video needs a second decode pipeline and a render surface; not worth it before the EQ ships. Revisit after PR-12.
- COMMENTS tab: music tracks commonly disable comments and the endpoint is unproven for them. RELATED and UP NEXT land in PR-7/PR-10 from the watch-playlist call.
- Home/Explore feed pages: sidebar navigation exists from PR-6; Home is search-first, Explore maps to filtered search. A real home feed needs an endpoint ytmapi-rs does not expose today.

## What stays unproven at plan time

- OAuth device flow against Google with a real operator-owned client ID (PR-8 is its first live test).
- Stream resolution durability: YouTube's attestation wall moves; yt-dlp updates track it. The `StreamResolver` trait exists so the backend can be replaced without touching playback.
- cpal/WASAPI behavior on the operator's audio hardware (PR-5's underrun predicate is the test).
- Lyrics availability for arbitrary tracks (some have none; panel needs the empty state).
- Edge-catalog behavior: age-restricted, region-blocked, and podcast content (the resolver will error; error surfacing lands with PR-7).

## Scratch

Prototypes live in `prototypes/pa-api` (search + stream probing) and `prototypes/pb-decode` (fetch + decode + DSP proof). Both are throwaway instruments per the prototype playbook; they are committed as evidence, not as production code.

## Iteration 2 (started 2026-11-19)

Ordered per the operator's brief; PR-13 landed, the rest queued.

- PR-13 vertical equalizer: canvas fader widget with drag-to-gain, dB above, Hz below, 0 dB reference line; Rock/Classical builtins; Rename/Delete/Reset for user presets. LANDED.
- PR-14 design system: shared primary/secondary/icon button styles, consistent padding, hover/pressed/disabled states, restrained gradients.
- PR-15 typography + accessibility: bundled Inter font option, global text scale (S/M/L), contrast bump for secondary text, settings persisted via rt-config.
- PR-16 keyboard shortcuts: Space/Arrows/Ctrl+K/Ctrl+E/M/L/? defaults with a shortcuts overlay; customization deferred until the settings page exists.
- PR-17 settings page: account, appearance, typography, playback, audio/EQ, shortcuts, privacy — assembling existing toggles + persistence.
- PR-8/9 auth + library: still blocked on the operator's Google Cloud OAuth client (unchanged).
- Deferred candidates (operator's list): more themes (Midnight/AMOLED/Ocean/Sunset/Light), mini-player mode, synced-lyric highlight (upstream), video mode.

## Iteration 2 defect pass (2026-11-19, after operator report)

- Fader drag bug FIXED: canvas `update()` mixed widget-relative cursor coords with absolute bounds (click band k selected k-2, handle missed the cursor). Now uses absolute coords; `press_target` extracted as the regression surface with placed-widget tests.
- DEF-02 Explore/Library: real stages — Explore = mood tile grid running live searches; Library = signed-in playlist list or sign-in prompt.
- DEF-04 sidebar mixes: starter mixes are live mood searches until sign-in replaces them with account playlists.
- DEF-06 chips: clickable, re-run the queue search per mood (InnerTube exposes no anonymous continuation-token filter — deviation documented).
- DEF-03 New playlist: modal + `AuthedClient::create_playlist` (private); disabled with instruction when signed out.
- DEF-05 Video: still deferred by design (needs a video decode/render pipeline); Song/Video pills remain non-functional for Video and the operator's spec lists it last.
