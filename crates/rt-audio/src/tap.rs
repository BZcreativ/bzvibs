//! The audio tap: a small SPSC ring fed from the output callback (post-EQ,
//! mono mixdown) and a reusable FFT analyzer that maps it to log-spaced
//! bands for the visualizer. The callback only pushes into the ring — no
//! locks, no allocation.

pub const TAP_CAPACITY: usize = 4096;
pub const ANALYSIS_LEN: usize = 2048;
pub const BAND_COUNT: usize = 48;
pub const BAND_LOW_HZ: f32 = 40.0;
pub const BAND_HIGH_HZ: f32 = 16_000.0;

/// Reusable analyzer: plans the FFT once, reuses scratch buffers.
pub struct SpectrumAnalyzer {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    scratch: Vec<rustfft::num_complex::Complex<f32>>,
    window: Vec<f32>,
}

impl SpectrumAnalyzer {
    pub fn new() -> Self {
        let mut planner = rustfft::FftPlanner::new();
        let fft = planner.plan_fft_forward(ANALYSIS_LEN);
        // process_with_scratch requires the scratch buffer pre-sized.
        let scratch =
            vec![rustfft::num_complex::Complex { re: 0.0, im: 0.0 }; fft.get_inplace_scratch_len()];
        // Hann window, precomputed.
        let window = (0..ANALYSIS_LEN)
            .map(|i| {
                (std::f32::consts::PI * i as f32 / ANALYSIS_LEN as f32)
                    .sin()
                    .powi(2)
            })
            .collect();
        SpectrumAnalyzer {
            fft,
            scratch,
            window,
        }
    }

    /// `samples` is mono, most recent last (any length; the tail up to
    /// ANALYSIS_LEN is used, zero-padded at the front). `bands` receives
    /// normalized 0..1 magnitudes, log-spaced over BAND_LOW_HZ..BAND_HIGH_HZ.
    pub fn analyze(&mut self, samples: &[f32], rate: u32, bands: &mut [f32]) {
        let n = ANALYSIS_LEN;
        let mut buffer: Vec<rustfft::num_complex::Complex<f32>> =
            vec![rustfft::num_complex::Complex { re: 0.0, im: 0.0 }; n];
        let tail = samples.len().min(n);
        let offset = n - tail;
        for i in 0..tail {
            buffer[offset + i].re = samples[samples.len() - tail + i] * self.window[offset + i];
        }
        self.fft
            .process_with_scratch(&mut buffer, &mut self.scratch);

        let bins = n / 2;
        let hz_per_bin = rate as f32 / n as f32;
        let log_lo = BAND_LOW_HZ.ln();
        let log_hi = BAND_HIGH_HZ.ln();
        let bands_len = bands.len().max(1);
        for (b, band) in bands.iter_mut().enumerate() {
            let lo_hz = (log_lo + (log_hi - log_lo) * b as f32 / bands_len as f32).exp();
            let hi_hz = (log_lo + (log_hi - log_lo) * (b + 1) as f32 / bands_len as f32).exp();
            let lo_bin = (lo_hz / hz_per_bin) as usize;
            let hi_bin = ((hi_hz / hz_per_bin) as usize).clamp(lo_bin + 1, bins);
            let mut peak = 0.0f32;
            for bin in buffer[lo_bin..hi_bin].iter() {
                let mag = (bin.norm_sqr() / (n * n) as f32).sqrt();
                peak = peak.max(mag);
            }
            // dB scale with a noise floor, clamped to 0..1.
            let db = 20.0 * (peak.max(1e-9)).log10();
            *band = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        }
    }
}

impl Default for SpectrumAnalyzer {
    fn default() -> Self {
        SpectrumAnalyzer::new()
    }
}

/// Center frequency of a band index, for labeling and tests.
pub fn band_center_hz(band: usize, count: usize) -> f32 {
    let log_lo = BAND_LOW_HZ.ln();
    let log_hi = BAND_HIGH_HZ.ln();
    (log_lo + (log_hi - log_lo) * (band as f32 + 0.5) / count as f32).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin() * 0.5)
            .collect()
    }

    #[test]
    fn fft_of_440hz_sine_peaks_in_the_440hz_band() {
        let mut analyzer = SpectrumAnalyzer::new();
        let mut bands = [0.0f32; BAND_COUNT];
        analyzer.analyze(&sine(440.0, 44_100.0, ANALYSIS_LEN), 44_100, &mut bands);
        let top = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let center = band_center_hz(top, BAND_COUNT);
        assert!(
            (350.0..=550.0).contains(&center),
            "top band center {center} Hz"
        );
    }

    #[test]
    fn silence_maps_to_zero_bands() {
        let mut analyzer = SpectrumAnalyzer::new();
        let mut bands = [0.0f32; BAND_COUNT];
        analyzer.analyze(&vec![0.0; ANALYSIS_LEN], 44_100, &mut bands);
        assert!(bands.iter().all(|b| *b < 0.05), "{bands:?}");
    }

    #[test]
    fn louder_signal_yields_higher_band_value() {
        let mut analyzer = SpectrumAnalyzer::new();
        let mut quiet = [0.0f32; BAND_COUNT];
        let mut loud = [0.0f32; BAND_COUNT];
        analyzer.analyze(&sine(1000.0, 44_100.0, ANALYSIS_LEN), 44_100, &mut quiet);
        let loud_samples: Vec<f32> = sine(1000.0, 44_100.0, ANALYSIS_LEN)
            .iter()
            .map(|s| s * 4.0)
            .collect();
        analyzer.analyze(&loud_samples, 44_100, &mut loud);
        let q: f32 = quiet.iter().sum();
        let l: f32 = loud.iter().sum();
        assert!(l > q, "loud {l} should exceed quiet {q}");
    }

    #[test]
    fn analysis_stays_under_budget_in_release() {
        let samples = sine(440.0, 44_100.0, ANALYSIS_LEN);
        let mut analyzer = SpectrumAnalyzer::new();
        let mut bands = [0.0f32; BAND_COUNT];
        let runs = 100usize;
        let started = std::time::Instant::now();
        for _ in 0..runs {
            analyzer.analyze(&samples, 44_100, &mut bands);
        }
        let per_ms = started.elapsed().as_secs_f64() * 1000.0 / runs as f64;
        eprintln!("spectrum per frame: {per_ms:.3} ms");
        if !cfg!(debug_assertions) {
            assert!(per_ms < 2.0, "{per_ms:.3} ms per analysis");
        }
    }
}
