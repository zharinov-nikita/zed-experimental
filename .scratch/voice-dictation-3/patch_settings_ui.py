import sys

def patch(path, pairs):
    s = open(path, encoding='utf-8').read()
    for old, new in pairs:
        if old not in s:
            print('NOT FOUND in ' + path + ':\n' + old[:400]); sys.exit(1)
        s = s.replace(old, new, 1)
    open(path, 'w', encoding='utf-8').write(s)

patch('crates/settings_ui/src/pages.rs', [
('''mod dictation_page;
''', '''mod dictation_downloads;
mod dictation_page;
'''),
])

patch('crates/settings_ui/Cargo.toml', [
('''cpal.workspace = true
''', '''cpal.workspace = true
dictation.workspace = true
'''),
])

patch('crates/audio/src/audio_pipeline.rs', [
('''pub fn ensure_devices_initialized(cx: &mut App) {
    if cx.has_global::<AvailableAudioDevices>() {
        return;
    }
    cx.default_global::<AvailableAudioDevices>();
    let task = cx
        .background_executor()
        .spawn(async move { get_available_audio_devices() });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let devices = task.await;
        cx.update(|cx| cx.set_global(AvailableAudioDevices(devices)));
        cx.refresh();
    })
    .detach();
}
''', '''pub fn ensure_devices_initialized(cx: &mut App) {
    if cx.has_global::<AvailableAudioDevices>() {
        return;
    }
    // Local: the first enumeration goes through the refresh bookkeeping so
    // `refresh_devices_if_stale` does not repeat it right away.
    refresh_devices(cx);
}
'''),
])

patch('crates/settings_ui/src/pages/audio_input_output_setup.rs', [
('''use ui::{ContextMenu, DropdownMenu, DropdownStyle, FluentBuilder, IconPosition, IntoElement};
''', '''use std::time::Duration;
use ui::{
    ContextMenu, DropdownMenu, DropdownStyle, FluentBuilder, IconPosition, IntoElement, Tooltip,
};
'''),
('''pub(crate) const SYSTEM_DEFAULT: &str = "System Default";
''', '''pub(crate) const SYSTEM_DEFAULT: &str = "System Default";

/// Local: a dropdown rendered within this long after the last enumeration
/// does not enumerate devices again; opening it always does.
const DEVICE_LIST_MAX_AGE: Duration = Duration::from_secs(2);
'''),
('''    audio::ensure_devices_initialized(cx);
    let devices = cx.global::<AvailableAudioDevices>().0.clone();
    let current_device = get_current_device(current_device_id.as_ref(), is_input, &devices);
''', '''    audio::ensure_devices_initialized(cx);
    // Local: a microphone plugged in after Zed started must show up here.
    audio::refresh_devices_if_stale(DEVICE_LIST_MAX_AGE, cx);
    let devices = cx.global::<AvailableAudioDevices>().0.clone();
    let current_device = get_current_device(current_device_id.as_ref(), is_input, &devices);
    // Local: the id is shown only in the trigger tooltip; names are the ones Windows uses.
    let current_device_tooltip: Option<SharedString> = current_device
        .as_ref()
        .map(|info| format!("{}\\n{}", info.display_name(), info.id).into());
'''),
('''                menu = menu.toggleable_entry(
                    device.to_string(),
                    is_current,
''', '''                menu = menu.toggleable_entry(
                    device.display_name(),
                    is_current,
'''),
('''    DropdownMenu::new(
        dropdown_id,
        current_device
            .map(|info| info.desc.name().to_string())
            .unwrap_or(SYSTEM_DEFAULT.to_string()),
        menu,
    )
    .style(DropdownStyle::Outlined)
    .full_width(true)
''', '''    DropdownMenu::new(
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
'''),
])

# ---------------- dictation page ----------------
patch('crates/settings_ui/src/pages/dictation_page.rs', [
('''//! Local: the Settings > AI > Dictation sub-page. Every widget here edits
//! `agent.dictation` in settings.json (the microphone lives under
//! `audio.experimental.input_audio_device`); nothing is stored elsewhere.
''', '''//! Local: the Settings > AI > Dictation sub-page. Every widget here edits
//! `agent.dictation` in settings.json (the microphone lives under
//! `audio.experimental.input_audio_device`); nothing is stored elsewhere.
//! Engine Download state lives in `dictation_downloads` and outlives the
//! window.
'''),
('''use agent_settings::AgentSettings;
use editor::{Editor, EditorEvent};
''', '''use agent_settings::AgentSettings;
use dictation::engine_download::{AssetSpec, WHISPER_MODELS};
use editor::{Editor, EditorEvent};
'''),
('''use ui::{
    ContextMenu, Disableable as _, Divider, DropdownMenu, DropdownStyle, IconPosition, Tooltip,
    prelude::*,
};
use util::ResultExt as _;

use crate::{
    SettingField, SettingItem, SettingsFieldMetadata, SettingsPageItem, SettingsWindow, USER,
    components::{SettingsInputField, SettingsSectionHeader},
    render_settings_item_layout,
};
''', '''use ui::{
    Banner, ContextMenu, Disableable as _, Divider, DropdownMenu, DropdownStyle, IconButtonShape,
    IconPosition, PopoverMenu, Tooltip, prelude::*,
};
use util::ResultExt as _;

use super::audio_test_window::open_audio_test_window;
use super::dictation_downloads::{
    DownloadStatus, DownloadTarget, cancel_download, dismiss_error, download_status,
    start_download,
};
use crate::{
    ActionLink, SettingField, SettingItem, SettingsFieldMetadata, SettingsPageItem, SettingsUiFile,
    SettingsWindow, USER,
    components::{SettingsInputField, SettingsSectionHeader},
    render_settings_item_layout,
};
'''),
('''const GLOSSARY_DESCRIPTION: &str =
    "Terms the recognizer and post-processing should spell exactly as written, one per row.";
''', '''const GLOSSARY_DESCRIPTION: &str =
    "Terms the recognizer and post-processing should spell exactly as written. Paste a comma-separated list to add several at once.";
const MODEL_PATH_DESCRIPTION: &str = "Path to the Whisper model file (ggml `.bin` or `.gguf`). Dictation is unavailable until this is set.";
const BACKENDS_DIR_DESCRIPTION: &str = "Directory with the ggml backend modules (`ggml-vulkan.dll` etc.). When empty, backends are looked up next to the speech library.";
'''),
('''/// Glossary edits are applied to the resolved list (defaults included) so the
/// terms the user sees are exactly the terms that end up in settings.json.
fn glossary_with_term(glossary: &[String], term: &str) -> Vec<String> {
    let term = term.trim();
    let mut result = glossary.to_vec();
    if !term.is_empty() && !result.iter().any(|existing| existing == term) {
        result.push(term.to_string());
    }
    result
}
''', '''/// Splits what the user typed or pasted into the add field into terms:
/// commas and line breaks separate them, whitespace around each is dropped,
/// empty parts and repeats are dropped, order is kept.
fn split_terms(input: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for part in input.split([',', '\\n', '\\r']) {
        let term = part.trim();
        if !term.is_empty() && !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_string());
        }
    }
    terms
}

/// Glossary edits are applied to the resolved list (defaults included) so the
/// terms the user sees are exactly the terms that end up in settings.json.
fn glossary_with_term(glossary: &[String], term: &str) -> Vec<String> {
    let term = term.trim();
    let mut result = glossary.to_vec();
    if !term.is_empty() && !result.iter().any(|existing| existing == term) {
        result.push(term.to_string());
    }
    result
}

fn glossary_with_terms(glossary: &[String], terms: &[String]) -> Vec<String> {
    terms
        .iter()
        .fold(glossary.to_vec(), |glossary, term| glossary_with_term(&glossary, term))
}
'''),
('''fn engine_items() -> Box<[SettingsPageItem]> {
    Box::new([
        SettingsPageItem::SettingItem(SettingItem {
            title: "Whisper Model Path",
            description: "Path to the Whisper model file (ggml `.bin` or `.gguf`). Dictation is unavailable until this is set.",
            field: Box::new(SettingField {
                organization_override: None,
                json_path: Some("agent.dictation.model_path"),
                pick: |settings| {
                    dictation_settings(settings)?
                        .model_path
                        .as_ref()
                        .or(DEFAULT_EMPTY_STRING)
                },
                write: |settings, value, _| {
                    dictation_content(settings.agent.get_or_insert_default()).model_path =
                        value.filter(|path| !path.trim().is_empty());
                },
            }),
            metadata: Some(Box::new(SettingsFieldMetadata {
                placeholder: Some("path/to/ggml-large-v3-q5_0.bin"),
                ..Default::default()
            })),
            files: USER,
        }),
        SettingsPageItem::SettingItem(SettingItem {
            title: "Backends Folder",
            description: "Directory with the ggml backend modules (`ggml-vulkan.dll` etc.). When empty, backends are looked up next to the speech library.",
            field: Box::new(SettingField {
                organization_override: None,
                json_path: Some("agent.dictation.backends_dir"),
                pick: |settings| {
                    dictation_settings(settings)?
                        .backends_dir
                        .as_ref()
                        .or(DEFAULT_EMPTY_STRING)
                },
                write: |settings, value, _| {
                    dictation_content(settings.agent.get_or_insert_default()).backends_dir =
                        value.filter(|path| !path.trim().is_empty());
                },
            }),
            metadata: Some(Box::new(SettingsFieldMetadata {
                placeholder: Some("path/to/transcribe-native"),
                ..Default::default()
            })),
            files: USER,
        }),
        SettingsPageItem::SettingItem(SettingItem {
            title: "Language",''', '''fn engine_items() -> Box<[SettingsPageItem]> {
    Box::new([
        SettingsPageItem::SettingItem(SettingItem {
            title: "Language",'''),
('''            metadata: None,
            files: USER,
        }),
    ])
}

fn flag_items() -> Box<[SettingsPageItem]> {''', '''            metadata: None,
            files: USER,
        }),
        SettingsPageItem::ActionLink(ActionLink {
            title: "Test Microphone".into(),
            description: Some(
                "Hear the selected microphone through the selected output to check it works."
                    .into(),
            ),
            button_text: "Test Microphone…".into(),
            on_click: Arc::new(|_, window, cx| open_audio_test_window(window, cx)),
            files: USER,
        }),
    ])
}

/// The Engine Assets rows: a path field plus a Download button that fills
/// it in. Rendered by hand because a plain text setting has no room for a
/// button, a progress and a Cancel in its row.
struct EngineAssetRow {
    target: DownloadTarget,
    title: &'static str,
    description: &'static str,
    json_path: &'static str,
    placeholder: &'static str,
    pick: fn(&SettingsContent) -> Option<&String>,
    write: fn(&mut AgentSettingsContent, Option<String>),
}

const ENGINE_ASSET_ROWS: [EngineAssetRow; 2] = [
    EngineAssetRow {
        target: DownloadTarget::Model,
        title: "Whisper Model Path",
        description: MODEL_PATH_DESCRIPTION,
        json_path: "agent.dictation.model_path",
        placeholder: "path/to/ggml-large-v3-q5_0.bin",
        pick: |settings| dictation_settings(settings)?.model_path.as_ref(),
        write: |agent, value| {
            dictation_content(agent).model_path = value.filter(|path| !path.trim().is_empty());
        },
    },
    EngineAssetRow {
        target: DownloadTarget::Backends,
        title: "Backends Folder",
        description: BACKENDS_DIR_DESCRIPTION,
        json_path: "agent.dictation.backends_dir",
        placeholder: "path/to/transcribe-native",
        pick: |settings| dictation_settings(settings)?.backends_dir.as_ref(),
        write: |agent, value| {
            dictation_content(agent).backends_dir = value.filter(|path| !path.trim().is_empty());
        },
    },
];

fn render_engine_asset_rows(
    settings_window: &SettingsWindow,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let mut rows = v_flex().px_8();
    for row in &ENGINE_ASSET_ROWS {
        let status = download_status(row.target, cx);
        let control = render_engine_asset_control(row, &status, cx);
        let layout = render_settings_item_layout(
            settings_window,
            row.title,
            row.description,
            control,
            None,
            None,
            Some(row.json_path),
            false,
            cx,
        )
        .pt_4()
        .pb_4();
        rows = rows.child(layout);
        if let DownloadStatus::Failed(error) = status {
            let target = row.target;
            rows = rows.child(
                div().pb_4().child(
                    Banner::new()
                        .severity(Severity::Error)
                        .wrap_content(true)
                        .child(Label::new(format!("Download failed: {error}")).size(LabelSize::Small))
                        .action_slot(
                            IconButton::new(("dictation-download-dismiss", target_index(target)), IconName::Close)
                                .icon_size(IconSize::Small)
                                .tooltip(Tooltip::text("Dismiss"))
                                .on_click(move |_, _, cx| dismiss_error(target, cx)),
                        ),
                ),
            );
        }
        rows = rows.child(Divider::horizontal());
    }
    rows.into_any_element()
}

fn target_index(target: DownloadTarget) -> usize {
    match target {
        DownloadTarget::Model => 0,
        DownloadTarget::Backends => 1,
    }
}

fn render_engine_asset_control(
    row: &'static EngineAssetRow,
    status: &DownloadStatus,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let (_, current) =
        SettingsStore::global(cx).get_value_from_file(SettingsUiFile::User.to_settings(), row.pick);
    let current = current.filter(|path| !path.is_empty()).cloned();
    let write = row.write;
    let field = SettingsInputField::new(row.json_path)
        .tab_index(0)
        .aria_label(row.title)
        .aria_description(row.description)
        .with_placeholder(row.placeholder)
        .when_some(current, |field, text| field.with_initial_text(text))
        .on_confirm(move |text, _window, cx| {
            update_agent_settings(cx, move |agent| write(agent, text));
        });

    let action: AnyElement = match status {
        DownloadStatus::Running {
            description,
            percent,
        } => {
            let target = row.target;
            let progress: SharedString = match percent {
                Some(percent) => format!("{percent}%").into(),
                None => "…".into(),
            };
            h_flex()
                .gap_1()
                .flex_none()
                .child(
                    Label::new(progress)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    IconButton::new(("dictation-download-cancel", target_index(target)), IconName::Close)
                        .icon_size(IconSize::Small)
                        .shape(IconButtonShape::Square)
                        .tooltip(Tooltip::text(format!("Cancel downloading {description}")))
                        .on_click(move |_, _, cx| cancel_download(target, cx)),
                )
                .into_any_element()
        }
        DownloadStatus::Idle | DownloadStatus::Failed(_) => match row.target {
            DownloadTarget::Model => render_model_download_menu(),
            DownloadTarget::Backends => Button::new("dictation-download-backends", "Download…")
                .style(ButtonStyle::Outlined)
                .label_size(LabelSize::Small)
                .tooltip(Tooltip::text(format!(
                    "Download transcribe.cpp {} (CPU + Vulkan) into the Zed data directory",
                    dictation::engine_download::TRANSCRIBE_CPP_VERSION
                )))
                .on_click(|_, _, cx| {
                    start_download(
                        DownloadTarget::Backends,
                        AssetSpec::backends(),
                        "backends".into(),
                        cx,
                    );
                })
                .into_any_element(),
        },
    };

    h_flex()
        .w_full()
        .gap_2()
        .child(div().flex_1().min_w_0().child(field))
        .child(action)
        .into_any_element()
}

fn render_model_download_menu() -> AnyElement {
    PopoverMenu::new("dictation-download-model")
        .trigger(
            Button::new("dictation-download-model-trigger", "Download…")
                .style(ButtonStyle::Outlined)
                .label_size(LabelSize::Small)
                .tooltip(Tooltip::text(
                    "Download a whisper.cpp model into the Zed data directory",
                )),
        )
        .menu(move |window, cx| {
            Some(ContextMenu::build(window, cx, |mut menu, _, _| {
                for model in &WHISPER_MODELS {
                    menu = menu.entry(
                        format!("{} · {}", model.name, model.size_label()),
                        None,
                        move |_, cx| {
                            start_download(
                                DownloadTarget::Model,
                                AssetSpec::model(model),
                                model.name.into(),
                                cx,
                            );
                        },
                    );
                }
                menu
            }))
        })
        .anchor(gpui::Corner::TopRight)
        .into_any_element()
}

fn flag_items() -> Box<[SettingsPageItem]> {'''),
('''        SettingsPageItem::SettingItem(SettingItem {
            title: "Save Last Recording",
            description: "Save the audio of the last session as a WAV file in the temp directory for diagnostics.",
            field: Box::new(SettingField {
                organization_override: None,
                json_path: Some("agent.dictation.save_last_recording"),
                pick: |settings| dictation_settings(settings)?.save_last_recording.as_ref(),
                write: |settings, value, _| {
                    dictation_content(settings.agent.get_or_insert_default()).save_last_recording =
                        value;
                },
            }),
            metadata: None,
            files: USER,
        }),
    ])
}
''', '''        SettingsPageItem::SettingItem(SettingItem {
            title: "Keep Session Audio",
            description: "How many dictation sessions keep their audio (16 kHz mono WAV in the `dictation/audio` folder of the Zed data directory) for replay and diagnostics. The oldest files are deleted first; 0 turns it off.",
            field: Box::new(SettingField {
                organization_override: None,
                json_path: Some("agent.dictation.session_audio.keep"),
                pick: |settings| {
                    dictation_settings(settings)?
                        .session_audio
                        .as_ref()?
                        .keep
                        .as_ref()
                },
                write: |settings, value, _| {
                    dictation_content(settings.agent.get_or_insert_default())
                        .session_audio
                        .get_or_insert_default()
                        .keep = value;
                },
            }),
            metadata: None,
            files: USER,
        }),
    ])
}
'''),
('''    let engine = render_items(&engine_items, window, cx);
    let flags = render_items(&flag_items, window, cx);
    let post_processing_enabled = render_items(&post_processing_items, window, cx);
''', '''    let engine_assets = render_engine_asset_rows(settings_window, cx);
    let engine = render_items(&engine_items, window, cx);
    let flags = render_items(&flag_items, window, cx);
    let post_processing_enabled = render_items(&post_processing_items, window, cx);
'''),
('''        .track_scroll(scroll_handle)
        .child(engine)
        .child(glossary)
''', '''        .track_scroll(scroll_handle)
        .child(engine_assets)
        .child(engine)
        .child(glossary)
'''),
('''fn render_glossary_section(cx: &mut Context<SettingsWindow>) -> AnyElement {
    let glossary = current_glossary(cx);
    let rows: Vec<AnyElement> = glossary
        .iter()
        .enumerate()
        .map(|(index, term)| render_glossary_row(index, term.clone(), cx))
        .collect();
    let add_input = render_add_glossary_input(cx);
    let is_empty = rows.is_empty();
    let empty_border = cx.theme().colors().border_variant;
''', '''fn render_glossary_section(cx: &mut Context<SettingsWindow>) -> AnyElement {
    let glossary = current_glossary(cx);
    let chips: Vec<AnyElement> = glossary
        .iter()
        .enumerate()
        .map(|(index, term)| render_glossary_chip(index, term.clone(), cx))
        .collect();
    let add_input = render_add_glossary_input(cx);
    let is_empty = chips.is_empty();
    let empty_border = cx.theme().colors().border_variant;
'''),
('''                .when(!is_empty, |this| {
                    this.child(v_flex().gap_1p5().children(rows))
                })
                .child(add_input),
''', '''                .when(!is_empty, |this| {
                    this.child(h_flex().flex_wrap().gap_1().children(chips))
                })
                .child(add_input),
'''),
('''/// The row's element id includes the term because the input field caches
/// its editor (and the confirm closure) by id: a renamed or removed term
/// must not keep an editor whose closure still knows the old term.
fn render_glossary_row(index: usize, term: String, cx: &mut Context<SettingsWindow>) -> AnyElement {
    let term_for_delete = term.clone();
    let term_for_update = term.clone();
    let settings_window = cx.entity().downgrade();

    SettingsInputField::new(format!("dictation-glossary-{index}-{term}"))
        .with_initial_text(term)
        .tab_index(0)
        .with_buffer_font()
        .color(Color::Default)
        .action_slot(
            IconButton::new(
                format!("dictation-glossary-delete-{index}"),
                IconName::Trash,
            )
            .icon_size(IconSize::Small)
            .icon_color(Color::Muted)
            .tooltip(Tooltip::text("Remove Term"))
            .on_click(cx.listener(move |_, _, _, cx| {
                let glossary = glossary_without_term(&current_glossary(cx), &term_for_delete);
                update_agent_settings(cx, move |agent| set_glossary(agent, glossary));
            })),
        )
        .on_confirm(move |new_term, _window, cx| {
            let Some(new_term) = new_term else {
                return;
            };
            if new_term.trim() == term_for_update {
                return;
            }
            let glossary =
                glossary_with_replaced_term(&current_glossary(cx), &term_for_update, &new_term);
            update_agent_settings(cx, move |agent| set_glossary(agent, glossary));
            settings_window.update(cx, |_, cx| cx.notify()).log_err();
        })
        .into_any_element()
}
''', '''/// One term as a chip with a remove cross. Editing a term is remove and
/// add, so the chip itself is not editable.
fn render_glossary_chip(index: usize, term: String, cx: &mut Context<SettingsWindow>) -> AnyElement {
    let colors = cx.theme().colors();
    let term_for_delete = term.clone();
    h_flex()
        .id(("dictation-glossary-chip", index))
        .flex_none()
        .h(px(24.))
        .pl_1p5()
        .pr_0p5()
        .gap_0p5()
        .rounded_sm()
        .border_1()
        .border_color(colors.border)
        .bg(colors.element_background)
        .child(
            Label::new(term)
                .size(LabelSize::Small)
                .buffer_font(cx)
                .single_line(),
        )
        .child(
            IconButton::new(("dictation-glossary-remove", index), IconName::Close)
                .icon_size(IconSize::XSmall)
                .icon_color(Color::Muted)
                .shape(IconButtonShape::Square)
                .tooltip(Tooltip::text("Remove Term"))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let glossary = glossary_without_term(&current_glossary(cx), &term_for_delete);
                    update_agent_settings(cx, move |agent| set_glossary(agent, glossary));
                })),
        )
        .into_any_element()
}
'''),
('''    SettingsInputField::new("dictation-glossary-new")
        .with_placeholder("Add a term (e.g. Kubernetes)…")
        .tab_index(0)
        .with_buffer_font()
        .display_clear_button()
        .display_confirm_button()
        .clear_on_confirm()
        .on_confirm(move |term, _window, cx| {
            let Some(term) = term else {
                return;
            };
            if term.trim().is_empty() {
                return;
            }
            let glossary = glossary_with_term(&current_glossary(cx), &term);
            update_agent_settings(cx, move |agent| set_glossary(agent, glossary));
            settings_window.update(cx, |_, cx| cx.notify()).log_err();
        })
        .into_any_element()
''', '''    SettingsInputField::new("dictation-glossary-new")
        .with_placeholder("Add terms (e.g. Kubernetes, Docker)…")
        .tab_index(0)
        .with_buffer_font()
        .display_clear_button()
        .display_confirm_button()
        .clear_on_confirm()
        .on_confirm(move |input, _window, cx| {
            let Some(input) = input else {
                return;
            };
            let terms = split_terms(&input);
            if terms.is_empty() {
                return;
            }
            let glossary = glossary_with_terms(&current_glossary(cx), &terms);
            update_agent_settings(cx, move |agent| set_glossary(agent, glossary));
            settings_window.update(cx, |_, cx| cx.notify()).log_err();
        })
        .into_any_element()
'''),
('''    #[test]
    fn removing_a_term_removes_only_that_term() {''', '''    #[test]
    fn a_comma_separated_list_becomes_one_term_per_part() {
        assert_eq!(
            split_terms("TypeScript, Docker, Kubernetes"),
            glossary(&["TypeScript", "Docker", "Kubernetes"])
        );
        assert_eq!(
            split_terms("  Rust ,, Docker,\\nRust, \\n , GitHub "),
            glossary(&["Rust", "Docker", "GitHub"])
        );
        assert_eq!(split_terms("Kubernetes"), glossary(&["Kubernetes"]));
        assert!(split_terms(" , , ").is_empty());
        assert!(split_terms("").is_empty());
    }

    #[test]
    fn pasted_terms_are_added_once_each_in_order() {
        let base = glossary(&["Rust", "Docker"]);
        assert_eq!(
            glossary_with_terms(&base, &glossary(&["Docker", "GitHub", "Rust", "Podman"])),
            glossary(&["Rust", "Docker", "GitHub", "Podman"])
        );
    }

    #[test]
    fn removing_a_term_removes_only_that_term() {'''),
])
print('ok')
