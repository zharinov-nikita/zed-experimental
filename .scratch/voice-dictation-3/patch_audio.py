import sys

p = 'crates/audio/src/audio_pipeline.rs'
s = open(p, encoding='utf-8').read()

def rep(old, new):
    global s
    if old not in s:
        print('NOT FOUND:\n' + old[:200])
        sys.exit(1)
    s = s.replace(old, new, 1)

rep('''pub fn open_input_stream(
    device_id: Option<DeviceId>,
) -> anyhow::Result<rodio::microphone::Microphone> {
    let builder = rodio::microphone::MicrophoneBuilder::new();
    let builder = if let Some(id) = device_id {
        // TODO(jk): upstream patch
        // if let Some(input_device) = default_host().device_by_id(id) {
        //     builder.device(input_device);
        // }
        match find_input_device(&id) {
            Some(input) => builder.device(input)?,
            None => {
                log::warn!(
                    "Selected audio input device {id} not found, falling back to the default input"
                );
                builder.default_device()?
            }
        }
    } else {
        builder.default_device()?
    };
    let stream = builder
''', '''pub fn open_input_stream(
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
''')

rep('''    log::info!("Opened microphone: {:?}", stream.config());
    Ok(stream)
}
''', '''    log::info!("Opened microphone: {:?}", stream.config());
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
/// extended lines, so the extended line that spells out the name is
/// preferred; without one the short name is used.
pub fn device_display_name(description: &DeviceDescription) -> String {
    let name = description.name();
    description
        .extended()
        .iter()
        .find(|line| {
            let line = line.trim();
            !line.is_empty() && line.to_lowercase().contains(&name.to_lowercase())
        })
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| name.to_string())
}
''')

rep('''impl AudioDeviceInfo {
    pub fn matches_input(&self, is_input: bool) -> bool {''', '''impl AudioDeviceInfo {
    /// Local: the name shown in device lists; the id belongs in a tooltip.
    pub fn display_name(&self) -> String {
        device_display_name(&self.desc)
    }

    pub fn matches_input(&self, is_input: bool) -> bool {''')

rep('''#[derive(Default, Clone, Debug)]
pub struct AvailableAudioDevices(pub Vec<AudioDeviceInfo>);

impl Global for AvailableAudioDevices {}
''', '''#[derive(Default, Clone, Debug)]
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

        let unrelated = DeviceDescriptionBuilder::new("Microphone")
            .add_extended_line("USB Audio Class 2.0")
            .build();
        assert_eq!(device_display_name(&unrelated), "Microphone");
    }
}
''')

open(p, 'w', encoding='utf-8').write(s)

p = 'crates/audio/src/audio.rs'
s = open(p, encoding='utf-8').read()
rep('''pub use audio_pipeline::{ensure_devices_initialized, resolve_device};''',
'''pub use audio_pipeline::{ensure_devices_initialized, resolve_device};
// Local: device display names, on-demand refresh and the opened-input report.
pub use audio_pipeline::{
    OpenedInputDevice, device_display_name, open_input_stream_reporting, refresh_devices,
    refresh_devices_if_stale,
};''')
open(p, 'w', encoding='utf-8').write(s)
print('ok')
