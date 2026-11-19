//! Decode, the 8-band DSP engine, the output stage, and the visualizer tap.

pub mod decode;
pub mod dsp;
pub mod output;
pub mod tap;

pub use decode::{DecodeError, DecodedTrack, decode};
pub use dsp::{BAND_FREQS_HZ, BAND_Q, Eq8, EqHandle, EqParams};
pub use output::{DeviceSpec, OutputError, Player, linear_resample};
pub use tap::{ANALYSIS_LEN, BAND_COUNT, SpectrumAnalyzer, band_center_hz};
