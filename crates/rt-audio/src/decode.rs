//! Whole-track demux and decode to interleaved f32. The PRD's audio path is
//! fetch-then-decode (youtui's InMemSong model), so the decoder owns the
//! complete buffer and the output stage streams from it.

use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::default::{get_codecs, get_probe};

#[derive(Debug, Clone)]
pub struct DecodedTrack {
    /// Interleaved samples, channel-major per frame.
    pub samples: Vec<f32>,
    pub rate: u32,
    pub channels: u16,
}

impl DecodedTrack {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("no audio track")]
    NoAudioTrack,
    #[error("demux or decode: {0}")]
    Symphonia(#[from] SymphoniaError),
}

pub fn decode(source: Box<dyn MediaSource>) -> Result<DecodedTrack, DecodeError> {
    let mss = MediaSourceStream::new(source, Default::default());
    let mut format =
        get_probe().probe(&Hint::new(), mss, Default::default(), Default::default())?;
    let track = format
        .tracks()
        .iter()
        .find(|t| {
            t.codec_params
                .as_ref()
                .is_some_and(CodecParameters::is_audio)
        })
        .cloned()
        .ok_or(DecodeError::NoAudioTrack)?;
    let params = match track.codec_params.as_ref() {
        Some(CodecParameters::Audio(a)) => a,
        _ => return Err(DecodeError::NoAudioTrack),
    };
    let mut decoder = get_codecs().make_audio_decoder(params, &Default::default())?;

    let mut samples: Vec<f32> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    let mut rate = params.sample_rate.unwrap_or(44_100);
    let mut channels = 2usize;
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track.id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = decoded.spec();
                rate = spec.rate();
                channels = spec.channels().count();
                // copy_to_vec_interleaved overwrites its destination; append
                // through a scratch slice instead so the track accumulates.
                scratch.resize(decoded.samples_interleaved(), 0.0);
                decoded.copy_to_slice_interleaved::<f32, _>(scratch.as_mut_slice());
                samples.extend_from_slice(&scratch);
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(DecodedTrack {
        samples,
        rate,
        channels: channels as u16,
    })
}
