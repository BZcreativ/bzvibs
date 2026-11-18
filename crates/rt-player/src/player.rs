//! The player engine: one thread owning the output device, the feeder, and
//! the queue; one async loader (resolve, fetch with retries, decode,
//! resample, channel-fix); a prefetch cache so advancing inside a queue is
//! instant. The UI talks to it through [`PlayerHandle`] commands and reads
//! a snapshot of [`Status`].

use crate::queue::{AdvanceOutcome, RepeatMode, TrackQueue};
use rt_api::Track;
use rt_audio::{Player, decode, linear_resample};
use rt_stream::{StreamResolver, VideoId, YtDlpResolver, fetch_full};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum TransportState {
    Stopped,
    Loading(String),
    Playing,
    Paused,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub current: Option<Track>,
    pub transport: TransportState,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub underruns: u64,
    pub queue_len: usize,
    pub queue_index: usize,
    pub volume_milli: u32,
    pub error: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            current: None,
            transport: TransportState::Stopped,
            position_secs: 0.0,
            duration_secs: 0.0,
            underruns: 0,
            queue_len: 0,
            queue_index: 0,
            volume_milli: 1000,
            error: None,
        }
    }
}

#[derive(Debug)]
pub enum Cmd {
    PlayQueue(Vec<Track>, usize),
    Pause,
    Resume,
    Toggle,
    Next,
    Prev,
    Seek(f32),
    SetVolume(u32),
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    Shutdown,
}

enum Internal {
    Loaded {
        video_id: String,
        samples: Arc<Vec<f32>>,
    },
    LoadFailed {
        video_id: String,
        error: String,
    },
}

#[derive(Clone)]
pub struct PlayerHandle {
    tx: Sender<Cmd>,
    status: Arc<Mutex<Status>>,
    /// Clone of the engine's EQ handle, handed back during spawn; None only
    /// when the output device failed to open before the handshake.
    eq: Option<rt_audio::EqHandle>,
}

impl PlayerHandle {
    pub fn spawn() -> Result<PlayerHandle, String> {
        let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
        let status = Arc::new(Mutex::new(Status::default()));
        let status_thread = status.clone();
        let (boot_tx, boot_rx) = std::sync::mpsc::channel::<Option<rt_audio::EqHandle>>();
        std::thread::Builder::new()
            .name("rt-player".into())
            .spawn(move || engine(rx, status_thread, boot_tx))
            .map_err(|e| format!("player thread spawn: {e}"))?;
        let eq = boot_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| format!("engine boot handshake: {e}"))?;
        Ok(PlayerHandle { tx, status, eq })
    }

    /// Live EQ control: store new gains from any thread; the audio callback
    /// picks them up on its next pass (lock-free, no glitch).
    pub fn eq(&self) -> Option<&rt_audio::EqHandle> {
        self.eq.as_ref()
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

struct Feeder {
    samples: Arc<Vec<f32>>,
    channels: usize,
    rate: u32,
    fed_samples: usize,
}

impl Feeder {
    fn frames_total(&self) -> f64 {
        self.samples.len() as f64 / self.channels.max(1) as f64
    }

    fn feed(&mut self, player: &mut Player) {
        let chunk = 4800 * self.channels;
        while self.fed_samples < self.samples.len() {
            let end = (self.fed_samples + chunk).min(self.samples.len());
            let n = player.queue(&self.samples[self.fed_samples..end]);
            self.fed_samples += n;
            if n == 0 {
                break;
            }
        }
    }

    fn finished_pushing(&self) -> bool {
        self.fed_samples >= self.samples.len()
    }

    fn position_secs(&self, player: &Player) -> f64 {
        let buffered = player.buffered_samples() as f64 / self.channels.max(1) as f64;
        ((self.fed_samples as f64 / self.channels.max(1) as f64) - buffered).max(0.0)
            / self.rate as f64
    }

    fn seek_fraction(&mut self, player: &mut Player, fraction: f32) {
        let mut target = (fraction.clamp(0.0, 1.0) * self.samples.len() as f32) as usize;
        target -= target % self.channels.max(1);
        self.fed_samples = target;
        player.request_flush();
    }
}

fn engine(rx: Receiver<Cmd>, status: Arc<Mutex<Status>>, boot: Sender<Option<rt_audio::EqHandle>>) {
    let mut player = match Player::open() {
        Ok(p) => p,
        Err(e) => {
            if let Ok(mut s) = status.lock() {
                s.error = Some(format!("output device: {e}"));
            }
            let _ = boot.send(None);
            return;
        }
    };
    let device = player.spec();
    let _ = player.start();
    let _ = boot.send(Some(player.eq_handle()));

    let (itx, irx) = std::sync::mpsc::channel::<Internal>();
    // Loader thread with a current-thread runtime: resolve (yt-dlp child) +
    // fetch (reqwest) + decode. This exact shape is the one measured working;
    // running the same chain on a multi-thread runtime hung the fetch
    // (2026-11-18), so the loader stays single-threaded by design.
    let (job_tx, job_rx) = std::sync::mpsc::channel::<(String, u32, u16, Sender<Internal>)>();
    std::thread::Builder::new()
        .name("rt-loader".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("loader runtime: {e}");
                    return;
                }
            };
            while let Ok((video_id, rate, channels, itx)) = job_rx.recv() {
                let msg = match VideoId::parse(&video_id) {
                    Ok(id) => {
                        let resolver = YtDlpResolver;
                        match rt.block_on(load_track(&resolver, &id, rate, channels)) {
                            Ok(samples) => Internal::Loaded {
                                video_id: id.as_str().to_string(),
                                samples: Arc::new(samples),
                            },
                            Err(e) => Internal::LoadFailed {
                                video_id: id.as_str().to_string(),
                                error: e,
                            },
                        }
                    }
                    Err(e) => Internal::LoadFailed {
                        video_id,
                        error: e.to_string(),
                    },
                };
                let _ = itx.send(msg);
            }
        })
        .ok();

    let mut queue: Option<TrackQueue> = None;
    let mut feeder: Option<Feeder> = None;
    let mut cache: HashMap<String, Arc<Vec<f32>>> = HashMap::new();
    let mut pending: HashSet<String> = HashSet::new();

    fn start_load(
        job_tx: &Sender<(String, u32, u16, Sender<Internal>)>,
        itx: &Sender<Internal>,
        pending: &mut HashSet<String>,
        track: &Track,
        rate: u32,
        channels: u16,
    ) {
        if pending.contains(&track.video_id) {
            return;
        }
        pending.insert(track.video_id.clone());
        let _ = job_tx.send((track.video_id.clone(), rate, channels, itx.clone()));
    }

    fn install(
        player: &mut Player,
        feeder: &mut Option<Feeder>,
        status: &Mutex<Status>,
        samples: Arc<Vec<f32>>,
        rate: u32,
        channels: u16,
    ) {
        player.request_flush();
        *feeder = Some(Feeder {
            samples,
            channels: channels as usize,
            rate,
            fed_samples: 0,
        });
        // Wait for the callback to consume the flush, pre-fill the ring,
        // then resume — the device never sees an empty unpaused ring.
        let waited = std::time::Instant::now();
        while player.flush_pending() && waited.elapsed() < Duration::from_millis(100) {
            std::thread::sleep(Duration::from_millis(1));
        }
        if let Some(f) = feeder.as_mut() {
            f.feed(player);
        }
        player.resume();
        if let Ok(mut s) = status.lock() {
            s.duration_secs = feeder.as_ref().unwrap().frames_total() / rate as f64;
            s.position_secs = 0.0;
            s.transport = TransportState::Playing;
        }
    }

    loop {
        let cmd = match rx.try_recv() {
            Ok(cmd) => Some(cmd),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Cmd::Shutdown),
        };
        let had_cmd = cmd.is_some();
        let internal = irx.try_recv().ok();

        if let Some(msg) = internal {
            match msg {
                Internal::Loaded { video_id, samples } => {
                    pending.remove(&video_id);
                    cache.insert(video_id.clone(), samples.clone());
                    let is_current = queue
                        .as_ref()
                        .and_then(|q| q.current())
                        .map(|t| t.video_id == video_id)
                        .unwrap_or(false);
                    if is_current {
                        install(
                            &mut player,
                            &mut feeder,
                            &status,
                            samples,
                            device.rate,
                            device.channels,
                        );
                    }
                }
                Internal::LoadFailed { video_id, error } => {
                    pending.remove(&video_id);
                    eprintln!("load failed for {video_id}: {error}");
                    if let Ok(mut s) = status.lock()
                        && s.current
                            .as_ref()
                            .map(|t| t.video_id == video_id)
                            .unwrap_or(false)
                    {
                        s.transport = TransportState::Stopped;
                        s.error = Some(error);
                    }
                }
            }
            // Keep the loop responsive: handle one event per iteration.
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        match cmd {
            Some(Cmd::PlayQueue(tracks, start)) => {
                let mut q = TrackQueue::new(tracks, start);
                if let Some(prev) = queue.as_ref() {
                    q.repeat = prev.repeat;
                    q.shuffle = prev.shuffle;
                }
                let track = q.current().cloned();
                queue = Some(q);
                feeder = None;
                player.pause();
                if let Some(track) = &track {
                    if let Ok(mut s) = status.lock() {
                        s.current = Some(track.clone());
                        s.transport = TransportState::Loading(track.title.clone());
                        s.position_secs = 0.0;
                        s.duration_secs = 0.0;
                        s.queue_len = queue.as_ref().unwrap().len();
                        s.queue_index = queue.as_ref().unwrap().queue_index();
                        s.error = None;
                    }
                    if let Some(hit) = cache.get(&track.video_id).cloned() {
                        install(
                            &mut player,
                            &mut feeder,
                            &status,
                            hit,
                            device.rate,
                            device.channels,
                        );
                    } else {
                        start_load(
                            &job_tx,
                            &itx,
                            &mut pending,
                            track,
                            device.rate,
                            device.channels,
                        );
                    }
                }
            }
            Some(Cmd::Pause) => {
                player.pause();
                if let Ok(mut s) = status.lock()
                    && s.transport == TransportState::Playing
                {
                    s.transport = TransportState::Paused;
                }
            }
            Some(Cmd::Resume) => {
                player.resume();
                if let Ok(mut s) = status.lock()
                    && s.transport == TransportState::Paused
                {
                    s.transport = TransportState::Playing;
                }
            }
            Some(Cmd::Toggle) => {
                let playing = status
                    .lock()
                    .map(|s| s.transport == TransportState::Playing)
                    .unwrap_or(false);
                if playing {
                    player.pause();
                } else {
                    player.resume();
                }
                if let Ok(mut s) = status.lock() {
                    match s.transport {
                        TransportState::Playing => s.transport = TransportState::Paused,
                        TransportState::Paused => s.transport = TransportState::Playing,
                        _ => {}
                    }
                }
            }
            Some(Cmd::Next) | Some(Cmd::Prev) => {
                let is_next = matches!(cmd, Some(Cmd::Next));
                let Some(q) = queue.as_mut() else { continue };
                let outcome = if is_next { q.advance() } else { q.go_back() };
                match outcome {
                    AdvanceOutcome::Play(_) | AdvanceOutcome::SameTrack => {
                        if let Some(track) = q.current().cloned() {
                            feeder = None;
                            player.pause();
                            player.request_flush();
                            if let Ok(mut s) = status.lock() {
                                s.current = Some(track.clone());
                                s.transport = TransportState::Loading(track.title.clone());
                                s.queue_index = q.queue_index();
                                s.position_secs = 0.0;
                                s.duration_secs = 0.0;
                            }
                            if let Some(hit) = cache.get(&track.video_id).cloned() {
                                install(
                                    &mut player,
                                    &mut feeder,
                                    &status,
                                    hit,
                                    device.rate,
                                    device.channels,
                                );
                            } else {
                                start_load(
                                    &job_tx,
                                    &itx,
                                    &mut pending,
                                    &track,
                                    device.rate,
                                    device.channels,
                                );
                            }
                        }
                    }
                    AdvanceOutcome::Stop => {
                        feeder = None;
                        player.pause();
                        player.request_flush();
                        if let Ok(mut s) = status.lock() {
                            s.transport = TransportState::Stopped;
                        }
                    }
                }
            }
            Some(Cmd::Seek(fraction)) => {
                if let Some(f) = feeder.as_mut() {
                    f.seek_fraction(&mut player, fraction);
                    if let Ok(mut s) = status.lock() {
                        s.position_secs = f.position_secs(&player);
                    }
                }
            }
            Some(Cmd::SetVolume(m)) => {
                player.set_volume_milli(m);
                if let Ok(mut s) = status.lock() {
                    s.volume_milli = m.min(1000);
                }
            }
            Some(Cmd::SetShuffle(on)) => {
                if let Some(q) = queue.as_mut() {
                    let seed = crate::queue::seed_from("ui");
                    q.set_shuffle(on, seed);
                    if let Ok(mut s) = status.lock() {
                        s.queue_index = q.queue_index();
                    }
                }
            }
            Some(Cmd::SetRepeat(r)) => {
                if let Some(q) = queue.as_mut() {
                    q.repeat = r;
                }
            }
            Some(Cmd::Shutdown) => break,
            None => {}
        }

        // Feeder tick: push more samples, mirror position, detect end.
        if let Some(f) = feeder.as_mut() {
            // Hold off only while a flush is still pending; otherwise keep
            // the ring topped up.
            if !player.flush_pending() {
                f.feed(&mut player);
            }
            let drained = player.buffered_samples() == 0 && f.finished_pushing();
            let pos = f.position_secs(&player);
            {
                let mut s = status.lock().unwrap();
                if s.transport == TransportState::Playing {
                    s.position_secs = pos;
                }
                s.underruns = player.underruns();
            }
            if drained {
                let next = queue.as_mut().and_then(|q| {
                    let out = q.advance();
                    match out {
                        AdvanceOutcome::Play(_) | AdvanceOutcome::SameTrack => q.current().cloned(),
                        AdvanceOutcome::Stop => None,
                    }
                });
                match next {
                    Some(track) => {
                        feeder = None;
                        player.pause();
                        if let Ok(mut s) = status.lock() {
                            s.current = Some(track.clone());
                            s.transport = TransportState::Loading(track.title.clone());
                            s.queue_index = queue.as_ref().map(|q| q.queue_index()).unwrap_or(0);
                            s.position_secs = 0.0;
                            s.duration_secs = 0.0;
                        }
                        if let Some(hit) = cache.get(&track.video_id).cloned() {
                            install(
                                &mut player,
                                &mut feeder,
                                &status,
                                hit,
                                device.rate,
                                device.channels,
                            );
                        } else {
                            start_load(
                                &job_tx,
                                &itx,
                                &mut pending,
                                &track,
                                device.rate,
                                device.channels,
                            );
                        }
                    }
                    None => {
                        feeder = None;
                        player.pause();
                        if let Ok(mut s) = status.lock() {
                            s.transport = TransportState::Stopped;
                            s.position_secs = 0.0;
                        }
                    }
                }
            } else if let Some(q) = queue.as_ref() {
                // Prefetch the next track while this one plays.
                if let Some(next) = q.up_next() {
                    let id = next.video_id.clone();
                    if !cache.contains_key(&id) && !pending.contains(&id) {
                        start_load(
                            &job_tx,
                            &itx,
                            &mut pending,
                            next,
                            device.rate,
                            device.channels,
                        );
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_millis(if had_cmd { 5 } else { 20 }));
    }
}

async fn load_track(
    resolver: &YtDlpResolver,
    id: &VideoId,
    device_rate: u32,
    device_channels: u16,
) -> Result<Vec<f32>, String> {
    let resolved = resolver.resolve(id).await.map_err(|e| e.to_string())?;
    // Transient CDN resets happen; retry the fetch a few times.
    let mut bytes: Option<Vec<u8>> = None;
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        match fetch_full(&resolved).await {
            Ok(b) => {
                bytes = Some(b);
                break;
            }
            Err(e) => {
                last_err = e.to_string();
                tokio::time::sleep(Duration::from_millis(300 * u64::from(attempt + 1))).await;
            }
        }
    }
    let bytes = bytes.ok_or_else(|| format!("fetch: {last_err}"))?;
    let decoded = decode(Box::new(Cursor::new(bytes))).map_err(|e| e.to_string())?;
    let resampled = linear_resample(
        &decoded.samples,
        decoded.channels as usize,
        decoded.rate,
        device_rate,
    );
    Ok(fix_channels(
        resampled,
        decoded.channels as usize,
        device_channels as usize,
    ))
}

/// Duplicate or drop channels to match the device layout.
fn fix_channels(samples: Vec<f32>, from: usize, to: usize) -> Vec<f32> {
    if from == to || to == 0 || from == 0 {
        return samples;
    }
    let frames = samples.len() / from;
    let mut out = Vec::with_capacity(frames * to);
    for f in 0..frames {
        let frame = &samples[f * from..f * from + from];
        for c in 0..to {
            out.push(frame[c % from]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_fix_duplicates_mono_to_stereo_and_back() {
        let mono = vec![1.0, 2.0, 3.0];
        let stereo = fix_channels(mono.clone(), 1, 2);
        assert_eq!(stereo, vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
        let back = fix_channels(stereo, 2, 1);
        assert_eq!(back, mono);
    }

    #[test]
    fn channel_fix_is_identity_when_layouts_match() {
        let s = vec![0.5, 0.25, 0.5, 0.25];
        assert_eq!(fix_channels(s.clone(), 2, 2), s);
    }
}
