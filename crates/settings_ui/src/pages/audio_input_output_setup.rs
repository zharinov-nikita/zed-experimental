use audio::{AudioDeviceInfo, AvailableAudioDevices};
use cpal::DeviceId;
use gpui::{AnyElement, App, ElementId, ReadGlobal, SharedString, Window};
use settings::{AudioInputDeviceName, AudioOutputDeviceName, SettingsStore};
use std::str::FromStr;
use std::time::Duration;
use ui::{
    ContextMenu, DropdownMenu, DropdownStyle, FluentBuilder, IconPosition, IntoElement, Tooltip,
};
use util::ResultExt;

use crate::{SettingField, SettingsFieldMetadata, SettingsUiFile, update_settings_file};

pub(crate) const SYSTEM_DEFAULT: &str = "System Default";

/// Local: a dropdown rendered within this long after the last enumeration
/// does not enumerate devices again; opening it always does.
const DEVICE_LIST_MAX_AGE: Duration = Duration::from_secs(2);

pub(crate) fn get_current_device(
    current_id: Option<&DeviceId>,
    is_input: bool,
    devices: &[AudioDeviceInfo],
) -> Option<AudioDeviceInfo> {
    let Some(current_id) = current_id else {
        return None;
    };
    devices
        .iter()
        .find(|d| d.matches(current_id, is_input))
        .cloned()
}

pub(crate) fn render_audio_device_dropdown<F>(
    dropdown_id: impl Into<ElementId>,
    current_device_id: Option<DeviceId>,
    is_input: bool,
    on_select: F,
    aria_label: Option<SharedString>,
    aria_description: Option<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement
where
    F: Fn(Option<DeviceId>, &mut Window, &mut App) + Clone + 'static,
{
    audio::ensure_devices_initialized(cx);
    // Local: a microphone plugged in after Zed started must show up here.
    audio::refresh_devices_if_stale(DEVICE_LIST_MAX_AGE, cx);
    let devices = cx.global::<AvailableAudioDevices>().0.clone();
    let current_device = get_current_device(current_device_id.as_ref(), is_input, &devices);
    // Local: the id is shown only in the trigger tooltip; names are the ones Windows uses.
    let current_device_tooltip: Option<SharedString> = current_device
        .as_ref()
        .map(|info| format!("{}\n{}", info.display_name(), info.id).into());

    let menu = ContextMenu::build(window, cx, {
        let current_device = current_device.clone();
        move |mut menu, _, _cx| {
            let is_system_default = current_device.is_none();
            menu = menu.toggleable_entry(
                SYSTEM_DEFAULT,
                is_system_default,
                IconPosition::Start,
                None,
                {
                    let on_select = on_select.clone();
                    move |window, cx| {
                        on_select(None, window, cx);
                    }
                },
            );

            for device in devices.iter().filter(|d| d.matches_input(is_input)) {
                let is_current = current_device
                    .as_ref()
                    .map(|info| info.matches(&device.id, is_input))
                    .unwrap_or(false);
                let device_id = device.id.clone();

                menu = menu.toggleable_entry(
                    device.display_name(),
                    is_current,
                    IconPosition::Start,
                    None,
                    {
                        let on_select = on_select.clone();
                        move |window, cx| {
                            on_select(Some(device_id.clone()), window, cx);
                        }
                    },
                );
            }
            menu
        }
    });

    DropdownMenu::new(
        dropdown_id,
        current_device
            .map(|info| info.display_name())
            .unwrap_or(SYSTEM_DEFAULT.to_string()),
        menu,
    )
    .style(DropdownStyle::Outlined)
    .full_width(true)
    .on_open(|_, cx| audio::refresh_devices(cx))
    .when_some(current_device_tooltip, |this, tooltip| {
        this.trigger_tooltip(Tooltip::text(tooltip))
    })
    .when_some(aria_label, |this, label| this.aria_label(label))
    .when_some(aria_description, |this, description| {
        this.aria_description(description)
    })
    .into_any_element()
}

fn render_settings_audio_device_dropdown<T: AsRef<Option<String>> + From<Option<String>> + Send>(
    field: SettingField<T>,
    file: SettingsUiFile,
    is_input: bool,
    title: &'static str,
    description: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (_, current_value): (_, Option<&T>) =
        SettingsStore::global(cx).get_value_from_file(file.to_settings(), field.pick);
    let current_device_id =
        current_value.and_then(|x| x.as_ref().clone().and_then(|x| DeviceId::from_str(&x).ok()));

    let dropdown_id: SharedString = if is_input {
        "input-audio-device-dropdown".into()
    } else {
        "output-audio-device-dropdown".into()
    };

    render_audio_device_dropdown(
        dropdown_id,
        current_device_id,
        is_input,
        move |device_id, window, cx| {
            let value: Option<T> = device_id.map(|id| T::from(Some(id.to_string())));
            update_settings_file(
                file.clone(),
                field.json_path,
                window,
                cx,
                move |settings, app| {
                    (field.write)(settings, value, app);
                },
            )
            .log_err();
        },
        Some(SharedString::new_static(title)),
        (!description.is_empty()).then(|| SharedString::new_static(description)),
        window,
        cx,
    )
}

pub fn render_input_audio_device_dropdown(
    field: SettingField<AudioInputDeviceName>,
    file: SettingsUiFile,
    _metadata: Option<&SettingsFieldMetadata>,
    title: &'static str,
    description: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    render_settings_audio_device_dropdown(field, file, true, title, description, window, cx)
}

pub fn render_output_audio_device_dropdown(
    field: SettingField<AudioOutputDeviceName>,
    file: SettingsUiFile,
    _metadata: Option<&SettingsFieldMetadata>,
    title: &'static str,
    description: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    render_settings_audio_device_dropdown(field, file, false, title, description, window, cx)
}
