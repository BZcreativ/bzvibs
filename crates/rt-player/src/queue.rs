//! Queue ordering as pure logic: play order over a track list, shuffle,
//! repeat, next/prev. No I/O, fully testable.

use rt_api::Track;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvanceOutcome {
    /// Play the track at this index into `order`.
    Play(usize),
    /// Stay on the current track.
    SameTrack,
    /// Queue exhausted.
    Stop,
}

#[derive(Debug, Clone)]
pub struct TrackQueue {
    tracks: Vec<Track>,
    /// Play order as indices into `tracks`.
    order: Vec<usize>,
    /// Cursor into `order`.
    cursor: usize,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

fn shuffled(n: usize, seed: u64) -> Vec<usize> {
    // Deterministic Fisher-Yates over a seed so tests are reproducible.
    let mut idx: Vec<usize> = (0..n).collect();
    let mut state = seed | 1;
    for i in (1..n).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = ((state >> 33) as usize) % (i + 1);
        idx.swap(i, j);
    }
    idx
}

impl TrackQueue {
    pub fn new(tracks: Vec<Track>, start: usize) -> Self {
        let order: Vec<usize> = (0..tracks.len()).collect();
        let cursor = start.min(order.len().saturating_sub(1));
        TrackQueue {
            tracks,
            order,
            cursor,
            repeat: RepeatMode::Off,
            shuffle: false,
        }
    }

    pub fn current(&self) -> Option<&Track> {
        self.order.get(self.cursor).map(|i| &self.tracks[*i])
    }

    pub fn current_video_id(&self) -> Option<String> {
        self.current().map(|t| t.video_id.clone())
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn queue_index(&self) -> usize {
        self.cursor
    }

    pub fn up_next(&self) -> Option<&Track> {
        match self.repeat {
            RepeatMode::One => self.current(),
            _ => self.order.get(self.cursor + 1).map(|i| &self.tracks[*i]),
        }
    }

    /// Jump to an absolute position in the current play order (a click on
    /// row `i` of the visible list).
    pub fn jump(&mut self, list_index: usize) -> Option<&Track> {
        let pos = self.order.iter().position(|i| *i == list_index)?;
        self.cursor = pos;
        self.current()
    }

    pub fn advance(&mut self) -> AdvanceOutcome {
        match self.repeat {
            RepeatMode::One => AdvanceOutcome::SameTrack,
            RepeatMode::All if self.cursor + 1 >= self.order.len() => {
                self.cursor = 0;
                AdvanceOutcome::Play(0)
            }
            _ => {
                if self.cursor + 1 < self.order.len() {
                    self.cursor += 1;
                    AdvanceOutcome::Play(self.cursor)
                } else {
                    AdvanceOutcome::Stop
                }
            }
        }
    }

    pub fn go_back(&mut self) -> AdvanceOutcome {
        if self.cursor == 0 {
            return match self.repeat {
                RepeatMode::All if !self.order.is_empty() => {
                    self.cursor = self.order.len() - 1;
                    AdvanceOutcome::Play(self.cursor)
                }
                _ => AdvanceOutcome::SameTrack,
            };
        }
        self.cursor -= 1;
        AdvanceOutcome::Play(self.cursor)
    }

    /// Toggle shuffle, keeping the current track under the cursor.
    pub fn set_shuffle(&mut self, on: bool, seed: u64) {
        if on == self.shuffle || self.order.is_empty() {
            self.shuffle = on;
            return;
        }
        self.shuffle = on;
        let current = self.order[self.cursor];
        let mut order = if on {
            shuffled(self.order.len(), seed)
        } else {
            (0..self.tracks.len()).collect()
        };
        if on {
            // Bring the playing track to the front.
            if let Some(pos) = order.iter().position(|i| *i == current) {
                order.swap(0, pos);
            }
            self.cursor = 0;
        } else {
            self.cursor = current;
        }
        self.order = order;
    }
}

pub fn seed_from(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: format!("t-{id}"),
            artist: "a".into(),
            album: None,
            duration: "3:00".into(),
        }
    }

    fn queue(n: usize) -> TrackQueue {
        TrackQueue::new((0..n).map(|i| track(&format!("v{i}"))).collect(), 0)
    }

    fn ids(q: &TrackQueue) -> Vec<String> {
        q.order
            .iter()
            .map(|i| q.tracks[*i].video_id.clone())
            .collect()
    }

    #[test]
    fn linear_order_and_advance_until_stop() {
        let mut q = queue(3);
        assert_eq!(q.current().unwrap().video_id, "v0");
        assert_eq!(q.advance(), AdvanceOutcome::Play(1));
        assert_eq!(q.advance(), AdvanceOutcome::Play(2));
        assert_eq!(q.advance(), AdvanceOutcome::Stop);
    }

    #[test]
    fn repeat_all_wraps_and_repeat_one_holds() {
        let mut q = queue(2);
        q.repeat = RepeatMode::All;
        assert_eq!(q.advance(), AdvanceOutcome::Play(1));
        assert_eq!(q.advance(), AdvanceOutcome::Play(0));
        q.repeat = RepeatMode::One;
        assert_eq!(q.advance(), AdvanceOutcome::SameTrack);
    }

    #[test]
    fn go_back_stops_at_head_without_repeat() {
        let mut q = queue(3);
        q.advance();
        assert_eq!(q.advance(), AdvanceOutcome::Play(2));
        assert_eq!(q.go_back(), AdvanceOutcome::Play(1));
        assert_eq!(q.go_back(), AdvanceOutcome::Play(0));
        assert_eq!(q.go_back(), AdvanceOutcome::SameTrack);
    }

    #[test]
    fn shuffle_keeps_current_first_and_unshuffle_restores() {
        let mut q = queue(5);
        q.advance();
        q.advance(); // now at v2
        q.set_shuffle(true, seed_from("test"));
        assert_eq!(q.current().unwrap().video_id, "v2");
        assert_eq!(q.order[0], 2);
        // All indices still present exactly once.
        let mut sorted = q.order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
        q.set_shuffle(false, 0);
        assert_eq!(ids(&q), vec!["v0", "v1", "v2", "v3", "v4"]);
        assert_eq!(q.current().unwrap().video_id, "v2");
    }

    #[test]
    fn jump_positions_by_list_index() {
        let mut q = queue(4);
        let t = q.jump(3).unwrap();
        assert_eq!(t.video_id, "v3");
        assert_eq!(q.cursor, 3);
    }

    #[test]
    fn up_next_follows_repeat_mode() {
        let mut q = queue(3);
        assert_eq!(q.up_next().unwrap().video_id, "v1");
        q.repeat = RepeatMode::One;
        assert_eq!(q.up_next().unwrap().video_id, "v0");
    }
}
