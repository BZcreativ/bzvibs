//! The two state machines the shell owns: sidebar navigation and the
//! playback transport. Pure transitions, tested without any UI.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Home,
    Explore,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    UpNext,
    Lyrics,
    Related,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportEvent {
    Play,
    Pause,
    Toggle,
    Stop,
    /// The current track finished on its own.
    Ended,
}

pub fn transport_step(t: Transport, e: TransportEvent) -> Transport {
    use Transport::*;
    use TransportEvent::*;
    match (t, e) {
        (Stopped, Play) | (Stopped, Toggle) => Playing,
        (Playing, Pause) | (Playing, Toggle) => Paused,
        (Playing, Play) | (Paused, Pause) => t,
        (Playing, Stop) | (Paused, Stop) | (Playing, Ended) | (Paused, Ended) => Stopped,
        (Stopped, Pause) | (Stopped, Stop) | (Stopped, Ended) => Stopped,
        (Paused, Play) | (Paused, Toggle) => Playing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_and_toggle_reach_playing_from_stoppable_states() {
        assert_eq!(
            transport_step(Transport::Stopped, TransportEvent::Play),
            Transport::Playing
        );
        assert_eq!(
            transport_step(Transport::Paused, TransportEvent::Play),
            Transport::Playing
        );
        assert_eq!(
            transport_step(Transport::Paused, TransportEvent::Toggle),
            Transport::Playing
        );
        assert_eq!(
            transport_step(Transport::Stopped, TransportEvent::Toggle),
            Transport::Playing
        );
    }

    #[test]
    fn pause_and_toggle_pause_from_playing() {
        assert_eq!(
            transport_step(Transport::Playing, TransportEvent::Pause),
            Transport::Paused
        );
        assert_eq!(
            transport_step(Transport::Playing, TransportEvent::Toggle),
            Transport::Paused
        );
    }

    #[test]
    fn stop_and_ended_reset_to_stopped() {
        for t in [Transport::Playing, Transport::Paused] {
            for e in [TransportEvent::Stop, TransportEvent::Ended] {
                assert_eq!(transport_step(t, e), Transport::Stopped);
            }
        }
    }

    #[test]
    fn pause_on_stopped_is_a_noop() {
        assert_eq!(
            transport_step(Transport::Stopped, TransportEvent::Pause),
            Transport::Stopped
        );
    }
}
