//! Decode, the 8-band DSP engine, and the output stage.

pub mod decode;
pub mod dsp;
pub mod output;

pub use decode::{DecodeError, DecodedTrack, decode};
pub use dsp::{BAND_FREQS_HZ, BAND_Q, Eq8, EqHandle, EqParams};
pub use output::{DeviceSpec, OutputError, Player, linear_resample};
