/// Audio subsystem for PitchBrick.
///
/// Provides microphone capture, FFT frequency analysis, reminder tone
/// playback, and audio device enumeration. All audio I/O uses the cpal
/// crate with WASAPI shared mode on Windows, allowing coexistence with
/// other audio applications (Discord, VRChat, etc.).
pub mod analysis;
pub mod capture;
pub mod devices;
pub mod playback;
