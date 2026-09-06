use anyhow::{Context as _, Result};
use collections::HashMap;
use cpal::{
    DeviceDescription, DeviceId, default_host,
    traits::{DeviceTrait, HostTrait},
};
use gpui::{App, AsyncApp, BorrowAppContext, Global};

pub(super) use cpal::Sample;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Source, mixer::Mixer, source::Buffered};
use settings::Settings;
use std::io::Cursor;
use util::ResultExt;

mod echo_canceller;
use echo_canceller::EchoCanceller;
mod rodio_ext;
pub use crate::audio_settings::AudioSettings;
pub use rodio_ext::RodioExt;

use crate::Sound;

use super::{CHANNEL_COUNT, SAMPLE_RATE};
pub const BUFFER_SIZE: usize = // echo canceller and livekit want 10ms of audio
    (SAMPLE_RATE.get() as usize / 100) * CHANNEL_COUNT.get() as usize;

pub fn init(_cx: &mut App) {}

// TODO(jk): this is currently cached only once - we should observe and react instead
pub fn ensure_devices_initialized(cx: &mut App) {
    if cx.has_global::<AvailableAudioDevices>() {
        return;
    }
    // Local: the first enumeration goes through the refresh bookkeeping so
    // `refresh_devices_if_stale` does not repeat it right away.
    refresh_devices(cx);
}

#[derive(Default)]
pub struct Audio {
    output: Option<(MixerDeviceSink, Mixer)>,
    pub echo_canceller: EchoCanceller,
    source_cache: HashMap<Sound, Buffered<Decoder<Cursor<Vec<u8>>>>>,
}

impl Global for Audio {}

impl Audio {
    fn ensure_output_exists(&mut self, output_audio_device: Option<DeviceId>) -> Result<&Mixer> {
        #[cfg(debug_assertions)]
        log::warn!(
            "Audio does not sound correct without optimizations. Use a release build to debug audio issues"
        );

        if self.output.is_none() {
            let (output_handle, output_mixer) =
                open_output_stream(output_audio_device, self.echo_canceller.clone())?;
            self.output = Some((output_handle, output_mixer));
        }

        Ok(self
            .output
            .as_ref()
            .map(|(_, mixer)| mixer)
            .expect("we only get here if opening the outputstream succeeded"))
    }

    pub fn play_sound(sound: Sound, cx: &mut App) {
        let output_audio_device = AudioSettings::get_global(cx).output_audio_device.clone();
        cx.update_default_global(|this: &mut Self, cx| {
            let source = this.sound_source(sound, cx).log_err()?;
            let output_mixer = this
                .ensure_output_exists(output_audio_device)
                .context("Could not get output mixer")
                .log_err()?;

            output_mixer.add(source);
            Some(())
        });
    }

    pub fn end_call(cx: &mut App) {
        cx.update_default_global(|this: &mut Self, _cx| {
            this.output.take();
        });
    }

    fn sound_source(&mut self, sound: Sound, cx: &App) -> Result<impl Source + use<>> {
        if let Some(wav) = self.source_cache.get(&sound) {
            return Ok(wav.clone());
        }

        let path = format!("sounds/{}.wav", sound.file());
        let bytes = cx
            .asset_source()
            .load(&path)?
            .map(anyhow::Ok)
            .with_context(|| format!("No asset available for path {path}"))??
            .into_owned();
        let cursor = Cursor::new(bytes);
        let source = Decoder::new(cursor)?.buffered();

        self.source_cache.insert(sound, source.clone());

        Ok(source)
    }
}

pub fn open_input_stream(
    device_id: Option<DeviceId>,
) -> anyhow::Result<rodio::microphone::Microphone> {
    open_input_stream_reporting(device_id).map(|(stream, _)| stream)
}

/// Local: which input a microphone stream actually opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedInputDevice {
    /// Display name of the opened device (see [`device_display_name`]).
    pub name: String,
    /// The configured device was not found and the default input was opened instead.
    pub configured_device_missing: bool,
}

/// Local: like [`open_input_stream`], also reporting which device was opened
/// so the caller can show it and say when the configured one was missing.
pub fn open_input_stream_reporting(
    device_id: Option<DeviceId>,
) -> anyhow::Result<(rodio::microphone::Microphone, OpenedInputDevice)> {
    let builder = rodio::microphone::MicrophoneBuilder::new();
    let (builder, opened) = if let Some(id) = device_id {
        // TODO(jk): upstream patch
        // if let Some(input_device) = default_host().device_by_id(id) {
        //     builder.device(input_device);
        // }
        match find_input_device(&id) {
            Some(input) => {
                let name = input
                    .clone()
                    .into_inner()
                    .description()
                    .map(|description| device_display_name(&description))
                    .unwrap_or_else(|_| id.to_string());
                (
                    builder.device(input)?,
                    OpenedInputDevice {
                        name,
                        configured_device_missing: false,
                    },
                )
            }
            None => {
                log::warn!(
                    "Selected audio input device {id} not found, falling back to the default input"
                );
                (
                    builder.default_device()?,
                    OpenedInputDevice {
                        name: default_input_name(),
                        configured_device_missing: true,
                    },
                )
            }
        }
    } else {
        (
            builder.default_device()?,
            OpenedInputDevice {
                name: default_input_name(),
                configured_device_missing: false,
            },
        )
    };
    let stream = builder
        .default_config()?
        .prefer_sample_rates([
            SAMPLE_RATE,
            SAMPLE_RATE.saturating_mul(rodio::nz!(2)),
            SAMPLE_RATE.saturating_mul(rodio::nz!(3)),
            SAMPLE_RATE.saturating_mul(rodio::nz!(4)),
        ])
        .prefer_channel_counts([rodio::nz!(1), rodio::nz!(2), rodio::nz!(3), rodio::nz!(4)])
        .prefer_buffer_sizes(512..)
        .open_stream()?;
    log::info!("Opened microphone: {:?}", stream.config());
    Ok((stream, opened))
}

/// Local: the name of the system default input, or a placeholder when the
/// host cannot describe it.
fn default_input_name() -> String {
    default_host()
        .default_input_device()
        .and_then(|device| device.description().ok())
        .map(|description| device_display_name(&description))
        .unwrap_or_else(|| "Default input".to_string())
}

/// Local: the name a device is shown under everywhere in Zed. WASAPI reports
/// the short device description ("Microphone") as the name and puts the
/// friendly name Windows shows ("Microphone (fifine Microphone)") into the
/// extended lines, so the first extended line is preferred; without one the
/// short name is used.
pub fn device_display_name(description: &DeviceDescription) -> String {
    description
        .extended()
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| description.name())
        .to_string()
}

/// Local: rodio's microphone builder only accepts devices from its own input
/// list, so the configured id is matched against that list rather than
/// resolved through `device_by_id`. Devices that fail to report an id are
/// skipped and a listing failure counts as "not found" so the caller can fall
/// back to the default input.
fn find_input_device(id: &DeviceId) -> Option<rodio::microphone::Input> {
    let inputs = match rodio::microphone::available_inputs() {
        Ok(inputs) => inputs,
        Err(error) => {
            log::warn!("Could not list audio input devices: {error}");
            return None;
        }
    };
    inputs.into_iter().find(|input| {
        input
            .clone()
            .into_inner()
            .id()
            .map(|input_id| &input_id == id)
            .unwrap_or(false)
    })
}

pub fn resolve_device(device_id: Option<&DeviceId>, input: bool) -> anyhow::Result<cpal::Device> {
    if let Some(id) = device_id {
        if let Some(device) = default_host().device_by_id(id) {
            return Ok(device);
        }
        log::warn!("Selected audio device not found, falling back to default");
    }
    if input {
        default_host()
            .default_input_device()
            .context("no audio input device available")
    } else {
        default_host()
            .default_output_device()
            .context("no audio output device available")
    }
}

pub fn open_test_output(device_id: Option<DeviceId>) -> anyhow::Result<MixerDeviceSink> {
    let device = resolve_device(device_id.as_ref(), false)?;
    DeviceSinkBuilder::from_device(device)?
        .open_stream()
        .context("Could not open output stream")
}

pub fn open_output_stream(
    device_id: Option<DeviceId>,
    mut echo_canceller: EchoCanceller,
) -> anyhow::Result<(MixerDeviceSink, Mixer)> {
    let device = resolve_device(device_id.as_ref(), false)?;
    let mut output_handle = DeviceSinkBuilder::from_device(device)?
        .open_stream()
        .context("Could not open output stream")?;
    output_handle.log_on_drop(false);
    log::info!("Output stream: {:?}", output_handle);

    let (output_mixer, source) = rodio::mixer::mixer(CHANNEL_COUNT, SAMPLE_RATE);
    // otherwise the mixer ends as it's empty
    output_mixer.add(rodio::source::Zero::new(CHANNEL_COUNT, SAMPLE_RATE));
    let echo_cancelling_source = source // apply echo cancellation just before output
        .inspect_buffer::<BUFFER_SIZE, _>(move |buffer| {
            let mut buf: [i16; _] = buffer.map(|s| s.to_sample());
            echo_canceller.process_reverse_stream(&mut buf)
        });
    output_handle.mixer().add(echo_cancelling_source);

    Ok((output_handle, output_mixer))
}

#[derive(Clone, Debug)]
pub struct AudioDeviceInfo {
    pub id: DeviceId,
    pub desc: DeviceDescription,
}

impl AudioDeviceInfo {
    /// Local: the name shown in device lists; the id belongs in a tooltip.
    pub fn display_name(&self) -> String {
        device_display_name(&self.desc)
    }

    pub fn matches_input(&self, is_input: bool) -> bool {
        if is_input {
            self.desc.supports_input()
        } else {
            self.desc.supports_output()
        }
    }

    pub fn matches(&self, id: &DeviceId, is_input: bool) -> bool {
        &self.id == id && self.matches_input(is_input)
    }
}

impl std::fmt::Display for AudioDeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.desc.name(), self.id)
    }
}

fn get_available_audio_devices() -> Vec<AudioDeviceInfo> {
    let Some(devices) = default_host().devices().ok() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?;
            let desc = device.description().ok()?;
            Some(AudioDeviceInfo { id, desc })
        })
        .collect()
}

#[derive(Default, Clone, Debug)]
pub struct AvailableAudioDevices(pub Vec<AudioDeviceInfo>);

impl Global for AvailableAudioDevices {}

/// Local: bookkeeping for on-demand device refreshes, so a dropdown that is
/// re-rendered every frame does not enumerate devices every frame.
#[derive(Default)]
struct DeviceRefresh {
    last_started: Option<std::time::Instant>,
    in_flight: bool,
}

impl Global for DeviceRefresh {}

/// Local: enumerates the audio devices again on a background thread and
/// replaces [`AvailableAudioDevices`] when done, so a microphone plugged in
/// after Zed started shows up without a restart. Concurrent calls coalesce.
pub fn refresh_devices(cx: &mut App) {
    {
        let refresh = cx.default_global::<DeviceRefresh>();
        if refresh.in_flight {
            return;
        }
        refresh.in_flight = true;
        refresh.last_started = Some(std::time::Instant::now());
    }
    cx.default_global::<AvailableAudioDevices>();
    let task = cx
        .background_executor()
        .spawn(async move { get_available_audio_devices() });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let devices = task.await;
        cx.update(|cx| {
            cx.set_global(AvailableAudioDevices(devices));
            cx.default_global::<DeviceRefresh>().in_flight = false;
        });
        cx.refresh();
    })
    .detach();
}

/// Local: [`refresh_devices`] unless a refresh started less than `max_age`
/// ago. Called from render code that wants a fresh list without paying for
/// it on every frame.
pub fn refresh_devices_if_stale(max_age: std::time::Duration, cx: &mut App) {
    let stale = cx
        .default_global::<DeviceRefresh>()
        .last_started
        .is_none_or(|started| started.elapsed() >= max_age);
    if stale {
        refresh_devices(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::DeviceDescriptionBuilder;

    #[test]
    fn the_friendly_name_is_preferred_over_the_short_name() {
        let description = DeviceDescriptionBuilder::new("Microphone")
            .add_extended_line("Microphone (fifine Microphone)")
            .build();
        assert_eq!(
            device_display_name(&description),
            "Microphone (fifine Microphone)"
        );
    }

    #[test]
    fn without_a_friendly_name_the_short_name_is_used() {
        let description = DeviceDescriptionBuilder::new("Headset Microphone").build();
        assert_eq!(device_display_name(&description), "Headset Microphone");

        let blank = DeviceDescriptionBuilder::new("Microphone")
            .add_extended_line("   ")
            .build();
        assert_eq!(device_display_name(&blank), "Microphone");
    }
}
