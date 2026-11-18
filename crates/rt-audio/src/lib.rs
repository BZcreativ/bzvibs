//! Decode and the 8-band DSP engine. Output wiring lands in PR-5.

pub mod decode;
pub mod dsp;

pub use decode::{DecodeError, DecodedTrack, decode};
pub use dsp::{BAND_FREQS_HZ, BAND_Q, Eq8, EqHandle, EqParams};
