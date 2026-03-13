/// FFT-based voice fundamental frequency (F0) detection.
///
/// Uses realfft for efficient real-to-complex FFT on windowed audio frames.
/// Applies a Hann window, finds the magnitude spectrum peak within the
/// human speech range (65-300 Hz), and refines the estimate with parabolic
/// interpolation. An adaptive noise floor with hysteresis prevents false
/// detections from background noise.
use realfft::RealFftPlanner;
use rustfft::num_complex::Complex;
use std::sync::Arc;

/// Lower bound of human speech fundamental frequency range in Hz.
const SPEECH_FREQ_MIN: f32 = 65.0;

/// Upper bound of human speech fundamental frequency range in Hz.
const SPEECH_FREQ_MAX: f32 = 300.0;

/// FFT window size in samples. 2048 at 48kHz gives ~23 Hz resolution
/// with ~42ms latency per frame, a good balance for real-time voice.
const FFT_SIZE: usize = 2048;

/// Hop size between consecutive FFT frames. Half of FFT_SIZE for
/// 50% overlap, giving an analysis update every ~21ms at 48kHz.
const HOP_SIZE: usize = 1024;

/// Exponential moving average smoothing factor for noise floor estimation.
/// Lower values produce slower, more stable adaptation.
const NOISE_FLOOR_ALPHA: f32 = 0.02;

/// Multiplier above noise floor required to trigger voice detection.
const VOICE_TRIGGER_MULTIPLIER: f32 = 3.0;

/// Lower multiplier for maintaining voice detection (hysteresis prevents chattering).
const VOICE_HOLD_MULTIPLIER: f32 = 1.5;

/// Minimum noise floor to prevent overly sensitive detection in silent rooms.
const MIN_NOISE_FLOOR: f32 = 0.001;

/// Real-time voice frequency analyzer using FFT with adaptive noise floor.
///
/// Feed audio samples via `push_samples()`, then call `analyze()` to get
/// detected frequencies. The analyzer processes overlapping windowed frames
/// and tracks an adaptive noise floor to distinguish speech from background noise.
///
/// # Example
///
/// ```
/// use pitchbrick::audio::analysis::FrequencyAnalyzer;
/// let mut analyzer = FrequencyAnalyzer::new(48000);
/// // Push 2048 samples of a 200 Hz sine wave
/// let samples: Vec<f32> = (0..2048)
///     .map(|i| (2.0 * std::f32::consts::PI * 200.0 * i as f32 / 48000.0).sin())
///     .collect();
/// analyzer.push_samples(&samples);
/// let freqs = analyzer.analyze();
/// ```
pub struct FrequencyAnalyzer {
    fft: Arc<dyn realfft::RealToComplex<f32>>,
    fft_size: usize,
    hop_size: usize,
    hann_window: Vec<f32>,
    input_buffer: Vec<f32>,
    fft_scratch: Vec<f32>,
    fft_output: Vec<Complex<f32>>,
    noise_floor: f32,
    voice_active: bool,
    sample_rate: f32,
}

impl FrequencyAnalyzer {
    /// Creates a new frequency analyzer for the given sample rate.
    ///
    /// Pre-computes the Hann window coefficients and allocates FFT buffers.
    ///
    /// # Arguments
    ///
    /// * `sample_rate` - Audio sample rate in Hz (e.g., 44100 or 48000).
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let fft_output = fft.make_output_vec();
        let fft_scratch = fft.make_input_vec();

        let hann_window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1.0
                    - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
            })
            .collect();

        FrequencyAnalyzer {
            fft,
            fft_size: FFT_SIZE,
            hop_size: HOP_SIZE,
            hann_window,
            input_buffer: Vec::with_capacity(FFT_SIZE * 4),
            fft_scratch,
            fft_output,
            noise_floor: 0.01,
            voice_active: false,
            sample_rate: sample_rate as f32,
        }
    }

    /// Appends new audio samples to the internal buffer for analysis.
    ///
    /// # Arguments
    ///
    /// * `samples` - Mono f32 audio samples to add to the analysis buffer.
    pub fn push_samples(&mut self, samples: &[f32]) {
        self.input_buffer.extend_from_slice(samples);
    }

    /// Processes all available overlapping FFT frames and returns detected frequencies.
    ///
    /// Each frame is 2048 samples with a hop of 1024. For each frame:
    /// 1. Applies a Hann window
    /// 2. Computes the real-to-complex FFT
    /// 3. Finds the magnitude peak within the speech frequency range (65-300 Hz)
    /// 4. Refines the peak frequency with parabolic interpolation
    /// 5. Updates the adaptive noise floor
    ///
    /// # Returns
    ///
    /// A vector of `Option<f32>` values, one per processed frame. `Some(freq)` if
    /// voice was detected, `None` if the frame was below the noise threshold.
    pub fn analyze(&mut self) -> Vec<Option<f32>> {
        let mut results = Vec::new();

        while self.input_buffer.len() >= self.fft_size {
            let result = self.analyze_frame();
            results.push(result);

            let drain_count = self.hop_size.min(self.input_buffer.len());
            self.input_buffer.drain(..drain_count);
        }

        results
    }

    /// Analyzes a single FFT frame from the front of the input buffer.
    ///
    /// # Returns
    ///
    /// `Some(frequency_hz)` if a voice fundamental is detected above the noise
    /// floor within the speech range, or `None` otherwise.
    fn analyze_frame(&mut self) -> Option<f32> {
        // Apply Hann window to the frame
        for (i, sample) in self.input_buffer[..self.fft_size].iter().enumerate() {
            self.fft_scratch[i] = sample * self.hann_window[i];
        }

        // Compute FFT
        if self.fft.process(&mut self.fft_scratch, &mut self.fft_output).is_err() {
            return None;
        }

        // Compute magnitude spectrum
        let magnitudes: Vec<f32> = self
            .fft_output
            .iter()
            .map(|c| (c.re * c.re + c.im * c.im).sqrt())
            .collect();

        // Compute frame energy (RMS of magnitudes in speech range)
        let min_bin = (SPEECH_FREQ_MIN * self.fft_size as f32 / self.sample_rate).ceil() as usize;
        let max_bin =
            (SPEECH_FREQ_MAX * self.fft_size as f32 / self.sample_rate).floor() as usize;
        let max_bin = max_bin.min(magnitudes.len() - 1);

        if min_bin >= max_bin || min_bin >= magnitudes.len() {
            return None;
        }

        let speech_magnitudes = &magnitudes[min_bin..=max_bin];
        let frame_energy: f32 =
            (speech_magnitudes.iter().map(|m| m * m).sum::<f32>() / speech_magnitudes.len() as f32)
                .sqrt();

        // Adaptive noise floor with hysteresis
        let threshold = if self.voice_active {
            self.noise_floor * VOICE_HOLD_MULTIPLIER
        } else {
            self.noise_floor * VOICE_TRIGGER_MULTIPLIER
        };

        if frame_energy <= threshold {
            // Silence: update noise floor
            self.noise_floor =
                self.noise_floor * (1.0 - NOISE_FLOOR_ALPHA) + frame_energy * NOISE_FLOOR_ALPHA;
            self.noise_floor = self.noise_floor.max(MIN_NOISE_FLOOR);
            self.voice_active = false;
            return None;
        }

        self.voice_active = true;

        // Find peak magnitude bin in speech range
        let mut peak_bin = min_bin;
        let mut peak_mag = magnitudes[min_bin];
        for (i, &mag) in magnitudes.iter().enumerate().take(max_bin + 1).skip(min_bin + 1) {
            if mag > peak_mag {
                peak_mag = mag;
                peak_bin = i;
            }
        }

        // Parabolic interpolation for sub-bin frequency accuracy
        let freq = if peak_bin > min_bin && peak_bin < max_bin {
            let alpha = magnitudes[peak_bin - 1];
            let beta = magnitudes[peak_bin];
            let gamma = magnitudes[peak_bin + 1];
            let denominator = alpha - 2.0 * beta + gamma;
            if denominator.abs() > f32::EPSILON {
                let delta = 0.5 * (alpha - gamma) / denominator;
                (peak_bin as f32 + delta) * self.sample_rate / self.fft_size as f32
            } else {
                peak_bin as f32 * self.sample_rate / self.fft_size as f32
            }
        } else {
            peak_bin as f32 * self.sample_rate / self.fft_size as f32
        };

        Some(freq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generates a pure sine wave at the given frequency.
    fn sine_wave(freq: f32, sample_rate: u32, num_samples: usize) -> Vec<f32> {
        (0..num_samples)
            .map(|i| {
                (2.0 * std::f32::consts::PI * freq * i as f32 / sample_rate as f32).sin()
            })
            .collect()
    }

    #[test]
    fn test_detect_200hz_sine() {
        let mut analyzer = FrequencyAnalyzer::new(48000);
        // Push enough samples for multiple frames to let noise floor adapt
        let samples = sine_wave(200.0, 48000, 48000);
        analyzer.push_samples(&samples);
        let results = analyzer.analyze();
        let detected: Vec<f32> = results.into_iter().flatten().collect();
        assert!(!detected.is_empty(), "Should detect frequency");
        let last = *detected.last().unwrap();
        assert!(
            (last - 200.0).abs() < 5.0,
            "Expected ~200 Hz, got {} Hz",
            last
        );
    }

    #[test]
    fn test_detect_100hz_sine() {
        let mut analyzer = FrequencyAnalyzer::new(48000);
        let samples = sine_wave(100.0, 48000, 48000);
        analyzer.push_samples(&samples);
        let results = analyzer.analyze();
        let detected: Vec<f32> = results.into_iter().flatten().collect();
        assert!(!detected.is_empty(), "Should detect frequency");
        let last = *detected.last().unwrap();
        assert!(
            (last - 100.0).abs() < 5.0,
            "Expected ~100 Hz, got {} Hz",
            last
        );
    }

    #[test]
    fn test_silence_returns_none() {
        let mut analyzer = FrequencyAnalyzer::new(48000);
        let silence = vec![0.0f32; 4096];
        analyzer.push_samples(&silence);
        let results = analyzer.analyze();
        assert!(
            results.iter().all(|r| r.is_none()),
            "Silence should produce no detections"
        );
    }
}
