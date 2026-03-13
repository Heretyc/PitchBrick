/// Microphone audio capture using cpal in WASAPI shared mode.
///
/// Captures audio from the selected input device and pushes samples
/// into a shared ring buffer for the frequency analyzer to consume.
/// WASAPI shared mode is the cpal default on Windows, so this does
/// not exclusively lock the microphone (other apps like Discord and
/// VRChat can use it simultaneously).
use cpal::traits::{DeviceTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Holds an active audio input stream and its negotiated sample rate.
///
/// The stream runs on a dedicated OS thread managed by cpal. Incoming
/// audio samples are pushed into the shared `buffer` provided at
/// construction time.
///
/// # Fields
///
/// * `stream` - The underlying cpal input stream (must stay alive to keep capturing).
/// * `sample_rate` - The sample rate negotiated with the audio device (Hz).
pub struct AudioCapture {
    #[allow(dead_code)]
    stream: cpal::Stream,
    pub sample_rate: u32,
}

impl AudioCapture {
    /// Creates a new audio capture stream on the given device.
    ///
    /// Samples are converted to mono f32 and pushed into the provided buffer.
    /// The cpal input callback runs on a dedicated OS thread.
    ///
    /// # Arguments
    ///
    /// * `device` - The cpal input device to capture from.
    /// * `buffer` - Shared ring buffer where captured samples are accumulated.
    ///
    /// # Returns
    ///
    /// An `AudioCapture` instance, or an error string if stream creation fails.
    pub fn new(
        device: &cpal::Device,
        buffer: Arc<Mutex<VecDeque<f32>>>,
    ) -> Result<Self, String> {
        let supported_config = device
            .default_input_config()
            .map_err(|e| format!("No supported input config: {}", e))?;

        let sample_rate = supported_config.sample_rate();
        let channels = supported_config.channels() as usize;
        let sample_format = supported_config.sample_format();

        let config = cpal::StreamConfig {
            channels: supported_config.channels(),
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let buf = buffer.clone();
        let err_fn = |err: cpal::StreamError| {
            tracing::error!("Audio input stream error: {}", err);
        };

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    push_mono_samples(data, channels, &buf);
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let float_data: Vec<f32> = data
                        .iter()
                        .map(|&s| s as f32 / i16::MAX as f32)
                        .collect();
                    push_mono_samples(&float_data, channels, &buf);
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let float_data: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                        .collect();
                    push_mono_samples(&float_data, channels, &buf);
                },
                err_fn,
                None,
            ),
            other => {
                return Err(format!("Unsupported sample format: {:?}", other));
            }
        }
        .map_err(|e| format!("Failed to build input stream: {}", e))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start input stream: {}", e))?;

        tracing::info!(
            "Audio capture started: {}Hz, {} channels, {:?}",
            sample_rate,
            channels,
            sample_format
        );

        Ok(AudioCapture {
            stream,
            sample_rate,
        })
    }
}

/// Downmixes multi-channel audio to mono and pushes into the shared buffer.
///
/// # Arguments
///
/// * `data` - Interleaved audio samples from the cpal callback.
/// * `channels` - Number of audio channels in the interleaved data.
/// * `buffer` - Shared ring buffer to push mono samples into.
fn push_mono_samples(data: &[f32], channels: usize, buffer: &Arc<Mutex<VecDeque<f32>>>) {
    let mono: Vec<f32> = if channels == 1 {
        data.to_vec()
    } else {
        data.chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    if let Ok(mut buf) = buffer.lock() {
        buf.extend(mono.iter());
        // Limit buffer to ~2 seconds of audio at 48kHz to prevent unbounded growth
        const MAX_SAMPLES: usize = 96_000;
        while buf.len() > MAX_SAMPLES {
            buf.pop_front();
        }
    }
}
