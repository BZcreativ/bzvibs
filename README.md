# bzvibs

A lightweight desktop YouTube Music client for Windows, written in Rust.
Search, queue, and play music from YouTube Music with a built-in 8-band DSP
equalizer, an FFT visualizer, synced lyrics, and three hand-tuned themes —
no browser, no Electron.

The GUI is [iced](https://iced.rs) (Elm/MVU), audio is decoded by
`symphonia`, streams are resolved by a `yt-dlp` subprocess (audio-only,
AAC m4a / itag 140), and the 8-band EQ is a biquad chain measured against
sine tests (+6.00 dB measured for a +6 dB 1 kHz peaking filter).

## Features

- **Search & play** — search songs on YouTube Music without signing in;
  click a track to play. The first track takes a few seconds to resolve;
  tracks advanced to from the queue start instantly (prefetched).
- **One-click sign-in** — click "Sign in with Google", a browser window
  opens on music.youtube.com, sign in as usual and close the window. The
  app picks up the session by itself; nothing to copy, no codes. The
  session is stored in the Windows credential manager. Manual cookie paste
  and OAuth live behind "Advanced options".
- **Your library** — all your playlists in the sidebar (scrollable) and a
  Library stage for saved playlists and liked music.
- **8-band equalizer** — vertical faders, dB scale, 0 dB reference line,
  Rock/Classical builtins, save/rename/delete your own presets.
- **FFT visualizer** — real-time spectrum ring driven by the decode tap.
- **Lyrics** — synced-lyric view in the right panel, next to the queue.
- **Themes** — ember, nebula, and volt, cycled from the top bar.
- **Keyboard-first** — full shortcut set, `?` shows the in-app help.

## Keyboard shortcuts

| Action | Key |
| --- | --- |
| Play / Pause | `Space` |
| Seek forward / backward | `→` / `←` |
| Volume up / down | `↑` / `↓` |
| Search | `Ctrl + K` |
| Home / Explore / Library | `Ctrl + 1` / `Ctrl + 2` / `Ctrl + 3` |
| Equalizer | `Ctrl + E` |
| Mute | `Ctrl + M` |
| Like current track | `L` |
| Show shortcuts | `?` |

## Getting started

Prerequisites: Rust (stable, MSVC toolchain) and
[`yt-dlp`](https://github.com/yt-dlp/yt-dlp) on `PATH` or next to the exe.

```
cargo build --release
```

Then run `target\release\bzvibs.exe` from a folder that contains
`yt-dlp.exe` (or keep `yt-dlp` on `PATH`). Release bundles are packaged as
`bzvibs-<version>-win64.zip` with the exe, README, and resolver together.

## Workspace layout

| Crate | Role |
| --- | --- |
| `crates/rustytube` | the `bzvibs` binary: window/DPI bootstrap and app wiring |
| `crates/rt-ui` | iced UI: sidebar, stages, EQ fader canvas, visualizer, shortcuts |
| `crates/rt-api` | InnerTube client for YouTube Music: `ytmapi-rs` wrapper, browser/OAuth sign-in, keychain persistence |
| `crates/rt-audio` | decode, the 8-band DSP engine, output stage, visualizer tap |
| `crates/rt-player` | player controller gluing search results to the audio pipeline |
| `crates/rt-stream` | stream resolution and guarded fetching for itag-140 audio |
| `crates/rt-config` | EQ presets and appearance preferences, persisted under the config dir |

Every PR's design and measured verification is documented in
[PLAN.md](PLAN.md).

## Security posture

Every outbound request goes through one shared URL guard: `https` only,
and the host is resolved and checked before connecting — localhost,
loopback, private, and reserved addresses are refused. Stream URLs are
fetched from `youtube.com` / `googlevideo.com` hosts only.

## Known limits

- Windows x64 only for now (credential-manager session, DPI bootstrap).
- Video mode is not included; audio-only (AAC m4a, itag 140).
- The first track after launch pays the full resolve cost (several network
  round-trips); queue-advanced tracks are prefetched.

## Contributors

- **[BZcreativ](https://github.com/BZcreativ)** — project owner, product
  design, themes, and operator verification ([Forgejo](https://forgejo.bzlab.duckdns.org/buzman)).

## License

Not yet chosen — all rights reserved until a license file is added.
