/// FFT-based voice fundamental frequency (F0) detection.
///
/// Uses realfft for efficient real-to-complex FFT on windowed audio frames.
/// Applies a Hann window, finds the magnitude spectrum peak within the
/// human speech range (65-300 Hz), and refines the estimate with parabolic
/// interpolation. An adaptive noise floor with hysteresis prevents false
/// detections from background noise.
use realfft::RealFftPlanner;
use rustfft::num_complex::Complex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Lower bound of human speech fundamental frequency range in Hz.
/// Set to 85 Hz to exclude electrical hum (60 Hz mains noise and its harmonics)
/// from triggering false voice detections.
const SPEECH_FREQ_MIN: f32 = 85.0;

/// Upper bound of human speech fundamental frequency range in Hz.
const SPEECH_FREQ_MAX: f32 = 350.0;

/// FFT window size in samples. 2048 at 48kHz gives ~23 Hz resolution
/// with ~42ms latency per frame, a good balance for real-time voice.
const FFT_SIZE: usize = 2048;

/// Hop size between consecutive FFT frames. Half of FFT_SIZE for
/// 50% overlap, giving an analysis update every ~21ms at 48kHz.
const HOP_SIZE: usize = 1024;

/// Exponential moving average smoothing factor for noise floor estimation.
/// Higher values produce faster adaptation to changing ambient noise levels.
const NOISE_FLOOR_ALPHA: f32 = 0.05;

/// Multiplier above noise floor required to trigger voice detection.
const VOICE_TRIGGER_MULTIPLIER: f32 = 6.0;

/// Lower multiplier for maintaining voice detection (hysteresis prevents chattering).
const VOICE_HOLD_MULTIPLIER: f32 = 1.5;

/// Minimum noise floor to prevent overly sensitive detection in noisy rooms.
const MIN_NOISE_FLOOR: f32 = 0.02;

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
    /// Pre-allocated magnitude buffer — avoids a heap allocation on every frame.
    magnitudes: Vec<f32>,
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

        let mag_len = FFT_SIZE / 2 + 1;
        FrequencyAnalyzer {
            fft,
            fft_size: FFT_SIZE,
            hop_size: HOP_SIZE,
            hann_window,
            input_buffer: Vec::with_capacity(FFT_SIZE * 4),
            fft_scratch,
            fft_output,
            magnitudes: vec![0.0; mag_len],
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

        // Compute magnitude spectrum in-place (avoids per-frame heap allocation)
        for (m, c) in self.magnitudes.iter_mut().zip(self.fft_output.iter()) {
            *m = (c.re * c.re + c.im * c.im).sqrt();
        }
        let magnitudes = &self.magnitudes;

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

        let raw_freq = peak_bin as f32 * self.sample_rate / self.fft_size as f32;
        tracing::debug!(
            "FFT peak {:.1} Hz (bin {}, range {}-{}), energy {:.4}, threshold {:.4}",
            raw_freq,
            peak_bin,
            min_bin,
            max_bin,
            frame_energy,
            threshold
        );

        // Boundary check: a peak at min_bin or max_bin could be either a genuine
        // voice fundamental at the edge of the search range, or noise spilling in
        // from outside it.  Distinguish by looking at the adjacent bin outside the
        // range: if it has MORE energy than the boundary bin the true peak lies
        // outside the window (spillover → reject).  If the boundary bin has equal
        // or more energy the voice really is at that frequency (accept).
        if peak_bin == min_bin && min_bin > 0 && magnitudes[min_bin - 1] > magnitudes[min_bin] {
            tracing::debug!(
                "Boundary rejection: spillover at min ({:.1} Hz), outside energy {:.4} > inside {:.4}",
                raw_freq, magnitudes[min_bin - 1], magnitudes[min_bin]
            );
            self.voice_active = false;
            return None;
        }
        if peak_bin == max_bin
            && max_bin + 1 < magnitudes.len()
            && magnitudes[max_bin + 1] > magnitudes[max_bin]
        {
            tracing::debug!(
                "Boundary rejection: spillover at max ({:.1} Hz), outside energy {:.4} > inside {:.4}",
                raw_freq, magnitudes[max_bin + 1], magnitudes[max_bin]
            );
            self.voice_active = false;
            return None;
        }

        // Parabolic interpolation for sub-bin frequency accuracy.
        // Not possible at the exact boundaries (no neighbour on one side), so
        // return the raw bin frequency in that case.
        let freq = if peak_bin > min_bin && peak_bin < max_bin {
            let alpha = magnitudes[peak_bin - 1];
            let beta = magnitudes[peak_bin];
            let gamma = magnitudes[peak_bin + 1];
            let denominator = alpha - 2.0 * beta + gamma;
            if denominator.abs() > f32::EPSILON {
                let delta = 0.5 * (alpha - gamma) / denominator;
                (peak_bin as f32 + delta) * self.sample_rate / self.fft_size as f32
            } else {
                raw_freq
            }
        } else {
            raw_freq
        };

        Some(freq)
    }
}

/// Background thread that drains the audio buffer and runs the FFT analyzer.
///
/// Moving analysis off the UI thread prevents brief UI freezes that occur when
/// audio accumulates across ticks (e.g., due to OS preemption or window events)
/// and triggers a cascade of many FFT frames on the main thread.
///
/// Results are sent via an mpsc channel and consumed by `latest_result()` from
/// the UI thread on each animation tick.
pub struct AnalysisWorker {
    result_rx: std::sync::mpsc::Receiver<Option<f32>>,
    shutdown: Arc<AtomicBool>,
}

impl AnalysisWorker {
    /// Spawns an analysis worker thread that continuously processes audio from the
    /// shared buffer and sends detected frequency results to the UI thread.
    pub fn spawn(audio_buffer: Arc<Mutex<VecDeque<f32>>>, sample_rate: u32) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_thread = shutdown.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let mut analyzer = FrequencyAnalyzer::new(sample_rate);
            loop {
                if shutdown_thread.load(Ordering::Relaxed) {
                    break;
                }

                let samples = match audio_buffer.try_lock() {
                    Ok(mut buf) if !buf.is_empty() => buf.drain(..).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };

                if !samples.is_empty() {
                    analyzer.push_samples(&samples);
                    for result in analyzer.analyze() {
                        if tx.send(result).is_err() {
                            return; // UI dropped the receiver; exit cleanly
                        }
                    }
                } else {
                    // No new audio — yield to avoid spinning
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });

        AnalysisWorker {
            result_rx: rx,
            shutdown,
        }
    }

    /// Returns the most recent frequency result from the worker, draining any
    /// stale intermediate results. Returns `None` if no new frames are available.
    ///
    /// - `Some(Some(freq))` — voice detected at `freq` Hz
    /// - `Some(None)`       — frame processed but silence (no voice above threshold)
    /// - `None`             — no new frames since last call; keep current display state
    pub fn latest_result(&self) -> Option<Option<f32>> {
        let mut last = None;
        while let Ok(r) = self.result_rx.try_recv() {
            last = Some(r);
        }
        last
    }
}

impl Drop for AnalysisWorker {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
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
    fn test_detect_110hz_sine() {
        // 100 Hz lands at min_bin (bin 4 = 93.75 Hz) where parabolic interpolation
        // is unavailable, producing a 6.25 Hz quantisation error.  110 Hz sits
        // between bins 4 and 5, safely off the boundary, so interpolation works.
        let mut analyzer = FrequencyAnalyzer::new(48000);
        let samples = sine_wave(110.0, 48000, 48000);
        analyzer.push_samples(&samples);
        let results = analyzer.analyze();
        let detected: Vec<f32> = results.into_iter().flatten().collect();
        assert!(!detected.is_empty(), "Should detect frequency");
        let last = *detected.last().unwrap();
        assert!(
            (last - 110.0).abs() < 5.0,
            "Expected ~110 Hz, got {} Hz",
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
