/// Reminder tone generation and playback.
///
/// Generates a gentle humming tone using FM vibrato plus AM tremolo
/// on a sine carrier. The tone plays through a cpal output stream and
/// is controlled via atomic flags so the audio callback thread can
/// check state without locking.
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

/// FM vibrato oscillation rate in Hz (natural vocal vibrato speed).
const VIBRATO_RATE: f32 = 5.0;

/// FM vibrato depth: how many Hz the pitch wobbles from center.
const VIBRATO_DEPTH: f32 = 0.3;

/// AM tremolo oscillation rate in Hz.
const TREMOLO_RATE: f32 = 4.0;

/// AM tremolo depth (0.0 = no wobble, 1.0 = full wobble).
const TREMOLO_DEPTH: f32 = 0.3;

/// Fade-in duration in seconds to avoid clicks when the tone starts.
const FADE_IN_SECONDS: f32 = 0.1;

/// Manages the reminder tone output stream and its shared control parameters.
///
/// The tone generator runs in a cpal output callback on a dedicated OS thread.
/// Control parameters (playing, frequency, volume) are shared via atomics
/// so the main thread can adjust them without locking.
///
/// # Fields
///
/// * `stream` - The cpal output stream (kept alive to maintain playback).
/// * `playing` - Atomic flag: true when the tone should be audible.
/// * `frequency` - Atomic f32-as-u32 bits: the tone's base frequency in Hz.
/// * `volume` - Atomic f32-as-u32 bits: the tone's volume (0.0-1.0).
pub struct ReminderTone {
    #[allow(dead_code)]
    stream: cpal::Stream,
    pub playing: Arc<AtomicBool>,
    pub frequency: Arc<AtomicU32>,
    pub volume: Arc<AtomicU32>,
}

impl ReminderTone {
    /// Creates a new reminder tone on the given output device.
    ///
    /// The tone starts in the "not playing" state. Call `start()` and
    /// `stop()` to control audibility. Frequency and volume can be
    /// adjusted at any time via the atomic fields.
    ///
    /// # Arguments
    ///
    /// * `device` - The cpal output device to play through.
    /// * `initial_freq` - Starting tone frequency in Hz.
    /// * `initial_volume` - Starting volume (0.0-1.0).
    ///
    /// # Returns
    ///
    /// A `ReminderTone` instance, or an error string if stream creation fails.
    pub fn new(
        device: &cpal::Device,
        initial_freq: f32,
        initial_volume: f32,
    ) -> Result<Self, String> {
        let supported_config = device
            .default_output_config()
            .map_err(|e| format!("No supported output config: {}", e))?;

        let sample_rate = supported_config.sample_rate() as f32;
        let channels = supported_config.channels() as usize;
        let config: cpal::StreamConfig = supported_config.into();

        let playing = Arc::new(AtomicBool::new(false));
        let frequency = Arc::new(AtomicU32::new(initial_freq.to_bits()));
        let volume = Arc::new(AtomicU32::new(initial_volume.to_bits()));

        let playing_ref = playing.clone();
        let frequency_ref = frequency.clone();
        let volume_ref = volume.clone();

        // Phase accumulators (f64 for precision over long durations)
        let mut carrier_phase: f64 = 0.0;
        let mut vibrato_phase: f64 = 0.0;
        let mut tremolo_phase: f64 = 0.0;
        let mut fade_samples: f32 = 0.0;
        let fade_in_samples = FADE_IN_SECONDS * sample_rate;
        let mut was_playing = false;

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let is_playing = playing_ref.load(Ordering::Relaxed);
                    let freq = f32::from_bits(frequency_ref.load(Ordering::Relaxed));
                    let vol = f32::from_bits(volume_ref.load(Ordering::Relaxed));

                    if !is_playing && !was_playing {
                        // Silent: zero the buffer
                        for sample in data.iter_mut() {
                            *sample = 0.0;
                        }
                        return;
                    }

                    if is_playing && !was_playing {
                        // Just started: reset fade
                        fade_samples = 0.0;
                    }
                    was_playing = is_playing;

                    let sr = sample_rate as f64;

                    for frame in data.chunks_mut(channels) {
                        if !is_playing {
                            // Fade out quickly
                            fade_samples = (fade_samples - 1.0).max(0.0);
                        } else {
                            fade_samples = (fade_samples + 1.0).min(fade_in_samples);
                        }

                        let fade = fade_samples / fade_in_samples;

                        // FM vibrato: slight pitch wobble
                        let vibrato =
                            (2.0 * std::f64::consts::PI * vibrato_phase).sin() as f32;
                        let instantaneous_freq = freq + VIBRATO_DEPTH * vibrato;

                        // Carrier sine
                        let carrier =
                            (2.0 * std::f64::consts::PI * carrier_phase).sin() as f32;

                        // AM tremolo: gentle volume wobble
                        let tremolo_mod =
                            (2.0 * std::f64::consts::PI * tremolo_phase).sin() as f32;
                        let tremolo_envelope = 1.0 - TREMOLO_DEPTH * 0.5 * (1.0 + tremolo_mod);

                        let value = carrier * tremolo_envelope * vol * fade;

                        for sample in frame.iter_mut() {
                            *sample = value;
                        }

                        // Advance phases
                        carrier_phase += instantaneous_freq as f64 / sr;
                        vibrato_phase += VIBRATO_RATE as f64 / sr;
                        tremolo_phase += TREMOLO_RATE as f64 / sr;

                        // Wrap phases to avoid precision loss
                        if carrier_phase >= 1.0 {
                            carrier_phase -= 1.0;
                        }
                        if vibrato_phase >= 1.0 {
                            vibrato_phase -= 1.0;
                        }
                        if tremolo_phase >= 1.0 {
                            tremolo_phase -= 1.0;
                        }
                    }

                    // If done fading out, mark as truly stopped
                    if !is_playing && fade_samples <= 0.0 {
                        was_playing = false;
                    }
                },
                move |err| {
                    tracing::error!("Audio output stream error: {}", err);
                },
                None,
            )
            .map_err(|e| format!("Failed to build output stream: {}", e))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start output stream: {}", e))?;

        tracing::info!("Reminder tone output stream created");

        Ok(ReminderTone {
            stream,
            playing,
            frequency,
            volume,
        })
    }

    /// Starts the reminder tone (audible output begins with a fade-in).
    pub fn start(&self) {
        self.playing.store(true, Ordering::Relaxed);
    }

    /// Stops the reminder tone (fades out to avoid clicks).
    pub fn stop(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    /// Returns whether the tone is currently playing.
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Updates the tone frequency in Hz.
    ///
    /// # Arguments
    ///
    /// * `freq` - New frequency in Hz (should be 100-4000 Hz for comfortable hearing).
    pub fn set_frequency(&self, freq: f32) {
        self.frequency.store(freq.to_bits(), Ordering::Relaxed);
    }

    /// Updates the tone volume.
    ///
    /// # Arguments
    ///
    /// * `vol` - New volume (0.0 = silent, 1.0 = maximum).
    pub fn set_volume(&self, vol: f32) {
        self.volume.store(vol.to_bits(), Ordering::Relaxed);
    }
}
