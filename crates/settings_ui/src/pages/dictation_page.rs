//! Local: the Settings > AI > Dictation sub-page. Every widget here edits
//! `agent.dictation` in settings.json (the microphone lives under
//! `audio.experimental.input_audio_device`); nothing is stored elsewhere.
//! Engine Download state lives in `dictation_downloads` and outlives the
//! window.

use std::sync::Arc;
use std::time::Duration;

use agent_settings::AgentSettings;
use dictation::engine_download::{AssetSpec, WHISPER_MODELS};
use editor::{Editor, EditorEvent};
use gpui::{
    Entity, Focusable as _, ReadGlobal as _, ScrollHandle, Subscription, Task, TextStyleRefinement,
    WeakEntity, prelude::*,
};
use language::language_settings::SoftWrap;
use language_model::{LanguageModelProvider, LanguageModelProviderId, LanguageModelRegistry};
use project::agent_server_store::AllAgentServersSettings;
use settings::{
    AgentConfigOptionValue, AgentSettingsContent, AudioInputDeviceName,
    DictationAgentOptionContent, DictationAgentOptionKindContent,
    DictationPostProcessingSettingsContent, DictationSettingsContent, LanguageModelProviderSetting,
    LanguageModelSelection, Settings as _, SettingsContent, SettingsStore,
};
use theme_settings::ThemeSettings;
use ui::{
    Banner, ContextMenu, Disableable as _, Divider, DropdownMenu, DropdownStyle, IconButtonShape,
    IconPosition, PopoverMenu, SwitchField, ToggleState, Tooltip, prelude::*,
};
use util::ResultExt as _;

use super::audio_test_window::open_audio_test_window;
use super::dictation_downloads::{
    DownloadStatus, DownloadTarget, cancel_download, dismiss_error, download_status, start_download,
};
use crate::{
    ActionLink, SettingField, SettingItem, SettingsPageItem, SettingsUiFile, SettingsWindow, USER,
    components::{SettingsInputField, SettingsSectionHeader},
    render_settings_item_layout,
};

const DEFAULT_AUDIO_INPUT: AudioInputDeviceName = AudioInputDeviceName(None);
const DEFAULT_EMPTY_AUDIO_INPUT: Option<&AudioInputDeviceName> = Some(&DEFAULT_AUDIO_INPUT);

const AGENT_DEFAULT_MODEL_LABEL: &str = "Agent default model";
const REWRITER_LANGUAGE_MODEL_LABEL: &str = "Zed language model";
const REWRITER_EXTERNAL_AGENT_LABEL: &str = "External Agent";
const AGENT_OPTION_DEFAULT_LABEL: &str = "Agent default";
const MODELS_UNKNOWN_LABEL: &str = "Not known yet";
const MODELS_UNKNOWN_DESCRIPTION: &str =
    "The agent has not been contacted yet. Its models are listed here after the first rewrite.";
const REWRITER_CONFLICT: &str = "Both a language model and an External Agent are set in settings.json. Post-processing is off until you choose one of them here.";
const PROMPT_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

const GLOSSARY_DESCRIPTION: &str = "Terms the recognizer and post-processing should spell exactly as written. Paste a comma-separated list to add several at once.";
const MODEL_PATH_DESCRIPTION: &str = "Path to the Whisper model file (ggml `.bin` or `.gguf`). Dictation is unavailable until this is set.";
const BACKENDS_DIR_DESCRIPTION: &str = "Directory with the ggml backend modules (`ggml-vulkan.dll` etc.). When empty, backends are looked up next to the speech library.";
const PROMPT_DESCRIPTION: &str = "`${output}` is replaced with the raw transcript and `${glossary}` with the comma-separated glossary.";

fn dictation_settings(settings: &SettingsContent) -> Option<&DictationSettingsContent> {
    settings.agent.as_ref()?.dictation.as_ref()
}

fn post_processing_settings(
    settings: &SettingsContent,
) -> Option<&DictationPostProcessingSettingsContent> {
    dictation_settings(settings)?.post_processing.as_ref()
}

fn dictation_content(agent: &mut AgentSettingsContent) -> &mut DictationSettingsContent {
    agent.dictation.get_or_insert_default()
}

fn post_processing_content(
    agent: &mut AgentSettingsContent,
) -> &mut DictationPostProcessingSettingsContent {
    dictation_content(agent)
        .post_processing
        .get_or_insert_default()
}

/// Splits what the user typed or pasted into the add field into terms:
/// commas and line breaks separate them, whitespace around each is dropped,
/// empty parts and repeats are dropped, order is kept.
fn split_terms(input: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for part in input.split([',', '\n', '\r']) {
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
    terms.iter().fold(glossary.to_vec(), |glossary, term| {
        glossary_with_term(&glossary, term)
    })
}

fn glossary_without_term(glossary: &[String], term: &str) -> Vec<String> {
    glossary
        .iter()
        .filter(|existing| existing.as_str() != term)
        .cloned()
        .collect()
}

fn set_glossary(agent: &mut AgentSettingsContent, glossary: Vec<String>) {
    dictation_content(agent).glossary = Some(glossary);
}

/// `None` removes the selection so the agent's default model is used again.
fn set_post_processing_model(
    agent: &mut AgentSettingsContent,
    selection: Option<LanguageModelSelection>,
) {
    post_processing_content(agent).model = selection;
}

/// `None` removes the override so the default prompt applies again.
fn set_post_processing_prompt(agent: &mut AgentSettingsContent, prompt: Option<String>) {
    post_processing_content(agent).prompt = prompt;
}

/// What rewrites the transcript: the first level of the choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rewriter {
    LanguageModel,
    ExternalAgent,
}

/// Both keys set at once in settings.json.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RewriterConflict;

/// The branch the page shows. An `agent` block, even one without an id
/// yet, is the agent branch; `model` alone or nothing is the language
/// model branch, where nothing means the agent's default model.
fn rewriter_choice(
    post_processing: Option<&DictationPostProcessingSettingsContent>,
) -> Result<Rewriter, RewriterConflict> {
    let Some(post_processing) = post_processing else {
        return Ok(Rewriter::LanguageModel);
    };
    match (&post_processing.model, &post_processing.agent) {
        (Some(_), Some(_)) => Err(RewriterConflict),
        (None, Some(_)) => Ok(Rewriter::ExternalAgent),
        (_, None) => Ok(Rewriter::LanguageModel),
    }
}

/// Choosing a rewriter drops the other one's key, so the two never
/// coexist. Choosing the agent branch picks `first_agent` when no agent
/// was chosen before, so the settings are valid at once.
fn set_rewriter(agent: &mut AgentSettingsContent, rewriter: Rewriter, first_agent: Option<&str>) {
    let post_processing = post_processing_content(agent);
    match rewriter {
        Rewriter::LanguageModel => post_processing.agent = None,
        Rewriter::ExternalAgent => {
            post_processing.model = None;
            let chosen = post_processing.agent.get_or_insert_default();
            if chosen.id.is_none() {
                chosen.id = first_agent.map(str::to_string);
            }
        }
    }
}

fn set_post_processing_agent_id(agent: &mut AgentSettingsContent, id: String) {
    let post_processing = post_processing_content(agent);
    post_processing.model = None;
    post_processing.agent.get_or_insert_default().id = Some(id);
}

/// `None` removes the option so the agent's own default applies again.
fn set_agent_option(
    agent: &mut AgentSettingsContent,
    option_id: &str,
    value: Option<AgentConfigOptionValue>,
) {
    let options = post_processing_content(agent)
        .agent
        .get_or_insert_default()
        .options
        .get_or_insert_default();
    match value {
        Some(value) => {
            options.insert(option_id.to_string(), value);
        }
        None => {
            options.remove(option_id);
        }
    }
}

/// The agents the user has configured, by the ids they have under
/// `agent_servers`; the same list the agent panel offers.
fn configured_agents(settings: &AllAgentServersSettings) -> Vec<String> {
    let mut agents: Vec<String> = settings.keys().cloned().collect();
    agents.sort();
    agents
}

fn is_model_option(option: &DictationAgentOptionContent) -> bool {
    option.category.as_deref() == Some("model") || option.id == "model"
}

fn is_mode_option(option: &DictationAgentOptionContent) -> bool {
    option.category.as_deref() == Some("mode")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelChoice {
    id: String,
    name: String,
    /// The value the agent had selected when it announced the option.
    agent_default: bool,
}

/// The models an agent offers, as far as Zed knows them.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ModelChoices {
    /// The agent has not been contacted yet, or announced no model option.
    Unknown,
    Known {
        option_id: String,
        choices: Vec<ModelChoice>,
    },
}

/// Built from the cache the first rewrite filled; Zed cannot ask the
/// agent for its models without talking to it.
fn model_choices(cached: Option<&[DictationAgentOptionContent]>) -> ModelChoices {
    let Some(option) =
        cached.and_then(|cached| cached.iter().find(|option| is_model_option(option)))
    else {
        return ModelChoices::Unknown;
    };
    match &option.kind {
        DictationAgentOptionKindContent::Select { current, values } => ModelChoices::Known {
            option_id: option.id.clone(),
            choices: values
                .iter()
                .map(|value| ModelChoice {
                    id: value.id.clone(),
                    name: value.name.clone(),
                    agent_default: &value.id == current,
                })
                .collect(),
        },
        DictationAgentOptionKindContent::Boolean { .. } => ModelChoices::Unknown,
    }
}

/// What the Model dropdown says: the chosen model by its human name, or
/// that the agent decides.
fn model_label(choices: &ModelChoices, chosen: Option<&AgentConfigOptionValue>) -> String {
    let ModelChoices::Known { choices, .. } = choices else {
        return MODELS_UNKNOWN_LABEL.to_string();
    };
    let Some(chosen) = chosen.and_then(AgentConfigOptionValue::as_value_id) else {
        return AGENT_DEFAULT_MODEL_LABEL.to_string();
    };
    choices
        .iter()
        .find(|choice| choice.id == chosen)
        .map(|choice| choice.name.clone())
        .unwrap_or_else(|| chosen.to_string())
}

/// The announced options other than the model and the mode: the mode is
/// always the strictest one and cannot be chosen here.
fn other_agent_options(
    cached: &[DictationAgentOptionContent],
) -> Vec<&DictationAgentOptionContent> {
    cached
        .iter()
        .filter(|option| !is_model_option(option) && !is_mode_option(option))
        .collect()
}

fn selection_for_model(provider: &str, model: &str) -> LanguageModelSelection {
    LanguageModelSelection {
        provider: LanguageModelProviderSetting(provider.to_string()),
        model: model.to_string(),
        enable_thinking: false,
        effort: None,
        speed: None,
    }
}

/// The selection written when the user picks a provider: the current model is
/// kept if it belongs to that provider, otherwise the provider's preferred
/// (default) model, otherwise its first model. `None` when the provider has
/// no models yet, so an empty model name never reaches settings.json.
fn selection_for_provider(
    provider: &str,
    models: &[String],
    preferred: Option<&str>,
    current: Option<&LanguageModelSelection>,
) -> Option<LanguageModelSelection> {
    let model = current
        .filter(|current| current.provider.0 == provider && models.contains(&current.model))
        .map(|current| current.model.as_str())
        .or_else(|| preferred.filter(|preferred| models.iter().any(|model| model == preferred)))
        .or_else(|| models.first().map(String::as_str))?;
    Some(selection_for_model(provider, model))
}

fn update_agent_settings(
    cx: &mut App,
    update: impl 'static + Send + FnOnce(&mut AgentSettingsContent),
) {
    SettingsStore::global(cx).update_settings_file(<dyn fs::Fs>::global(cx), move |settings, _| {
        update(settings.agent.get_or_insert_default());
    });
}

fn engine_items() -> Box<[SettingsPageItem]> {
    Box::new([
        SettingsPageItem::SettingItem(SettingItem {
            title: "Language",
            description: "Spoken language for recognition. Auto lets the model detect it.",
            field: Box::new(SettingField {
                organization_override: None,
                json_path: Some("agent.dictation.language"),
                pick: |settings| dictation_settings(settings)?.language.as_ref(),
                write: |settings, value, _| {
                    dictation_content(settings.agent.get_or_insert_default()).language = value;
                },
            }),
            metadata: None,
            files: USER,
        }),
        SettingsPageItem::SettingItem(SettingItem {
            title: "Microphone",
            description: "Input device used for dictation. Shared with the Audio page.",
            field: Box::new(SettingField {
                organization_override: None,
                json_path: Some("audio.experimental.input_audio_device"),
                pick: |settings| {
                    settings
                        .audio
                        .as_ref()?
                        .input_audio_device
                        .as_ref()
                        .or(DEFAULT_EMPTY_AUDIO_INPUT)
                },
                write: |settings, value, _| {
                    settings.audio.get_or_insert_default().input_audio_device = value;
                },
            }),
            metadata: None,
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
                        .child(
                            Label::new(format!("Download failed: {error}")).size(LabelSize::Small),
                        )
                        .action_slot(
                            IconButton::new(
                                ("dictation-download-dismiss", target_index(target)),
                                IconName::Close,
                            )
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
                    IconButton::new(
                        ("dictation-download-cancel", target_index(target)),
                        IconName::Close,
                    )
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
        .anchor(gpui::Anchor::TopRight)
        .into_any_element()
}

fn flag_items() -> Box<[SettingsPageItem]> {
    Box::new([
        SettingsPageItem::SettingItem(SettingItem {
            title: "Sounds",
            description: "Play a sound when dictation starts and stops.",
            field: Box::new(SettingField {
                organization_override: None,
                json_path: Some("agent.dictation.sounds"),
                pick: |settings| dictation_settings(settings)?.sounds.as_ref(),
                write: |settings, value, _| {
                    dictation_content(settings.agent.get_or_insert_default()).sounds = value;
                },
            }),
            metadata: None,
            files: USER,
        }),
        SettingsPageItem::SettingItem(SettingItem {
            title: "Keep Model Loaded",
            description: "Keep the Whisper model in memory between sessions so the next one starts instantly. Turn off to free VRAM after each session.",
            field: Box::new(SettingField {
                organization_override: None,
                json_path: Some("agent.dictation.keep_model_loaded"),
                pick: |settings| dictation_settings(settings)?.keep_model_loaded.as_ref(),
                write: |settings, value, _| {
                    dictation_content(settings.agent.get_or_insert_default()).keep_model_loaded =
                        value;
                },
            }),
            metadata: None,
            files: USER,
        }),
        SettingsPageItem::SettingItem(SettingItem {
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

fn post_processing_items() -> Box<[SettingsPageItem]> {
    Box::new([SettingsPageItem::SettingItem(SettingItem {
        title: "Enabled",
        description: "Rewrite the raw transcript with a language model before accepting it.",
        field: Box::new(SettingField {
            organization_override: None,
            json_path: Some("agent.dictation.post_processing.enabled"),
            pick: |settings| post_processing_settings(settings)?.enabled.as_ref(),
            write: |settings, value, _| {
                post_processing_content(settings.agent.get_or_insert_default()).enabled = value;
            },
        }),
        metadata: None,
        files: USER,
    })])
}

pub(crate) fn render_dictation_page(
    settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let engine_items = engine_items();
    let flag_items = flag_items();
    let post_processing_items = post_processing_items();

    // Item indices key element state on the page, so each block continues
    // the numbering of the previous one.
    let mut next_index = 0;
    let mut render_items =
        |items: &[SettingsPageItem], window: &mut Window, cx: &mut Context<SettingsWindow>| {
            let first_index = next_index;
            next_index += items.len();
            settings_window
                .render_sub_page_items_section(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, item)| (first_index + index, item)),
                    false,
                    window,
                    cx,
                )
                .into_any_element()
        };
    let engine_assets = render_engine_asset_rows(settings_window, cx);
    let engine = render_items(&engine_items, window, cx);
    let flags = render_items(&flag_items, window, cx);
    let post_processing_enabled = render_items(&post_processing_items, window, cx);

    let glossary = render_glossary_section(cx);
    let model_rows = render_post_processing_rewriter_rows(settings_window, window, cx);
    let prompt = render_prompt_section(window, cx);

    v_flex()
        .id("dictation-settings-page")
        .size_full()
        .pb_16()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(engine_assets)
        .child(engine)
        .child(glossary)
        .child(flags)
        .child(
            v_flex()
                .px_8()
                .pt_2()
                .child(SettingsSectionHeader::new("Post-processing").no_padding(true)),
        )
        .child(post_processing_enabled)
        .child(model_rows)
        .child(prompt)
        .into_any_element()
}

fn current_glossary(cx: &App) -> Vec<String> {
    AgentSettings::get_global(cx).dictation.glossary.clone()
}

fn render_glossary_section(cx: &mut Context<SettingsWindow>) -> AnyElement {
    let glossary = current_glossary(cx);
    let chips: Vec<AnyElement> = glossary
        .iter()
        .enumerate()
        .map(|(index, term)| render_glossary_chip(index, term.clone(), cx))
        .collect();
    let add_input = render_add_glossary_input(cx);
    let is_empty = chips.is_empty();
    let empty_border = cx.theme().colors().border_variant;

    v_flex()
        .px_8()
        .pt_4()
        .pb_4()
        .gap_0p5()
        .child(Label::new("Glossary"))
        .child(
            Label::new(GLOSSARY_DESCRIPTION)
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            v_flex()
                .mt_2()
                .w_full()
                .gap_1p5()
                .when(is_empty, |this| {
                    this.child(
                        h_flex()
                            .p_2()
                            .rounded_md()
                            .border_1()
                            .border_dashed()
                            .border_color(empty_border)
                            .child(
                                Label::new("No terms")
                                    .size(LabelSize::Small)
                                    .color(Color::Disabled),
                            ),
                    )
                })
                .when(!is_empty, |this| {
                    this.child(h_flex().flex_wrap().gap_1().children(chips))
                })
                .child(add_input),
        )
        .child(Divider::horizontal().mt_4())
        .into_any_element()
}

/// One term as a chip with a remove cross. Editing a term is remove and
/// add, so the chip itself is not editable.
fn render_glossary_chip(
    index: usize,
    term: String,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
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

fn render_add_glossary_input(cx: &mut Context<SettingsWindow>) -> AnyElement {
    let settings_window = cx.entity().downgrade();

    SettingsInputField::new("dictation-glossary-new")
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
}

fn write_post_processing_model(selection: Option<LanguageModelSelection>, cx: &mut App) {
    update_agent_settings(cx, move |agent| set_post_processing_model(agent, selection));
}

fn model_ids(provider: &dyn LanguageModelProvider, cx: &App) -> Vec<String> {
    provider
        .provided_models(cx)
        .iter()
        .map(|model| model.id().0.to_string())
        .collect()
}

/// Providers such as Ollama list their models only after they have been
/// asked to authenticate, which nothing does in a fresh settings window, so
/// the chosen provider is authenticated once and the page re-rendered. A
/// failed attempt is not retried while the window is open: retrying on
/// every render would hammer an unreachable provider.
fn ensure_provider_authenticated(
    provider: &Arc<dyn LanguageModelProvider>,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) {
    let key = gpui::ElementId::Name(format!("dictation-provider-auth-{}", provider.id().0).into());
    let state = window.use_keyed_state(key, cx, |_, _| None::<Task<()>>);
    if state.read(cx).is_some() || provider.is_authenticated(cx) {
        return;
    }
    let authenticate = provider.authenticate(cx);
    let provider_id = provider.id();
    let task = cx.spawn(async move |this, cx| {
        if let Err(error) = authenticate.await {
            log::warn!(
                "dictation settings: provider {} is unavailable: {error}",
                provider_id.0
            );
        }
        this.update(cx, |_, cx| cx.notify()).ok();
    });
    *state.as_mut(cx) = Some(task);
}

/// A provider chosen while it has no models yet. It is not written to
/// settings.json until a model is picked, but the dropdowns show it so the
/// user can finish the two-step choice.
type PendingProvider = Option<String>;

fn set_pending_provider(
    pending: &Entity<PendingProvider>,
    settings_window: &WeakEntity<SettingsWindow>,
    provider: PendingProvider,
    cx: &mut App,
) {
    *pending.as_mut(cx) = provider;
    settings_window.update(cx, |_, cx| cx.notify()).log_err();
}

/// The two-level choice of the rewriter: first whether a Zed language
/// model or an External Agent rewrites, then the specifics of that branch.
fn render_post_processing_rewriter_rows(
    settings_window: &SettingsWindow,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let (_, post_processing) = SettingsStore::global(cx)
        .get_value_from_file(SettingsUiFile::User.to_settings(), post_processing_settings);
    let post_processing = post_processing.cloned();
    let choice = rewriter_choice(post_processing.as_ref());
    let agents = configured_agents(AllAgentServersSettings::get_global(cx));

    let rewriter_dropdown = render_rewriter_dropdown(choice.ok(), agents.clone(), window, cx);
    let rewriter_row = render_settings_item_layout(
        settings_window,
        "Rewriter",
        "What rewrites the transcript: a language model from Zed's providers, or an External Agent through a session of its own.",
        rewriter_dropdown,
        None,
        None,
        Some("agent.dictation.post_processing"),
        false,
        cx,
    )
    .pt_4()
    .pb_4();

    let mut rows = v_flex().px_8();
    if choice.is_err() {
        rows = rows.child(
            div().pt_4().child(
                Banner::new()
                    .severity(Severity::Warning)
                    .wrap_content(true)
                    .child(Label::new(REWRITER_CONFLICT).size(LabelSize::Small)),
            ),
        );
    }
    rows = rows.child(rewriter_row).child(Divider::horizontal());
    let branch = match choice {
        Ok(Rewriter::LanguageModel) => {
            render_post_processing_model_rows(settings_window, window, cx)
        }
        Ok(Rewriter::ExternalAgent) => render_post_processing_agent_rows(
            settings_window,
            post_processing.and_then(|post_processing| post_processing.agent),
            agents,
            window,
            cx,
        ),
        Err(RewriterConflict) => div().into_any_element(),
    };
    rows.child(branch).into_any_element()
}

fn render_rewriter_dropdown(
    current: Option<Rewriter>,
    agents: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let label: SharedString = match current {
        Some(Rewriter::LanguageModel) => REWRITER_LANGUAGE_MODEL_LABEL.into(),
        Some(Rewriter::ExternalAgent) => REWRITER_EXTERNAL_AGENT_LABEL.into(),
        None => "Choose…".into(),
    };
    let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
        for (rewriter, label) in [
            (Rewriter::LanguageModel, REWRITER_LANGUAGE_MODEL_LABEL),
            (Rewriter::ExternalAgent, REWRITER_EXTERNAL_AGENT_LABEL),
        ] {
            let first_agent = agents.first().cloned();
            menu = menu.toggleable_entry(
                label,
                current == Some(rewriter),
                IconPosition::Start,
                None,
                move |_, cx| {
                    let first_agent = first_agent.clone();
                    update_agent_settings(cx, move |agent| {
                        set_rewriter(agent, rewriter, first_agent.as_deref())
                    });
                },
            );
        }
        menu
    });
    DropdownMenu::new("dictation-post-processing-rewriter", label, menu)
        .style(DropdownStyle::Outlined)
        .into_any_element()
}

/// The agent branch: which agent, which of its models, and its other
/// announced options. Models and options come from the cache the first
/// rewrite filled; the page never contacts the agent.
fn render_post_processing_agent_rows(
    settings_window: &SettingsWindow,
    chosen: Option<settings::DictationPostProcessingAgentSettingsContent>,
    agents: Vec<String>,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let chosen_id = chosen
        .as_ref()
        .and_then(|chosen| chosen.id.clone())
        .filter(|id| !id.trim().is_empty());
    let options = chosen.and_then(|chosen| chosen.options).unwrap_or_default();
    let cached = chosen_id.as_ref().and_then(|id| {
        AgentSettings::get_global(cx)
            .dictation
            .post_processing_agent_options_cache
            .get(id)
            .cloned()
    });
    let choices = model_choices(cached.as_deref());

    let agent_dropdown = render_agent_dropdown(chosen_id, agents, window, cx);
    let agent_row = render_settings_item_layout(
        settings_window,
        "Agent",
        "One of the agents configured under `agent_servers`. The rewrite runs in a session of its own on the connection Zed already holds for it.",
        agent_dropdown,
        None,
        None,
        Some("agent.dictation.post_processing.agent"),
        false,
        cx,
    )
    .pt_4()
    .pb_4();

    let model_dropdown = render_agent_model_dropdown(&choices, options.get("model"), window, cx);
    let model_row = render_settings_item_layout(
        settings_window,
        "Model",
        "The agent's model for post-processing, independent of the model your own threads use. Pick the agent's default explicitly to say so on purpose.",
        model_dropdown,
        None,
        None,
        Some("agent.dictation.post_processing.agent"),
        false,
        cx,
    )
    .pt_4()
    .pb_4();

    let mut rows = v_flex()
        .child(agent_row)
        .child(Divider::horizontal())
        .child(model_row);
    if matches!(choices, ModelChoices::Unknown) {
        rows = rows.child(
            div().pb_4().child(
                Label::new(MODELS_UNKNOWN_DESCRIPTION)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            ),
        );
    }
    rows = rows.child(Divider::horizontal());
    for option in other_agent_options(cached.as_deref().unwrap_or_default()) {
        rows = rows
            .child(render_agent_option_row(
                option,
                options.get(&option.id),
                window,
                cx,
            ))
            .child(Divider::horizontal());
    }
    rows.into_any_element()
}

fn render_agent_dropdown(
    chosen_id: Option<String>,
    agents: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let label: SharedString = match &chosen_id {
        Some(id) => id.clone().into(),
        None if agents.is_empty() => "No agents configured".into(),
        None => "Select an agent…".into(),
    };
    let disabled = agents.is_empty();
    let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
        for id in &agents {
            let is_current = chosen_id.as_deref() == Some(id.as_str());
            let id_for_write = id.clone();
            menu = menu.toggleable_entry(
                id.clone(),
                is_current,
                IconPosition::Start,
                None,
                move |_, cx| {
                    let id = id_for_write.clone();
                    update_agent_settings(cx, move |agent| set_post_processing_agent_id(agent, id));
                },
            );
        }
        menu
    });
    DropdownMenu::new("dictation-post-processing-agent", label, menu)
        .style(DropdownStyle::Outlined)
        .disabled(disabled)
        .into_any_element()
}

fn render_agent_model_dropdown(
    choices: &ModelChoices,
    chosen: Option<&AgentConfigOptionValue>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let label: SharedString = model_label(choices, chosen).into();
    let chosen_id = chosen
        .and_then(AgentConfigOptionValue::as_value_id)
        .map(str::to_string);
    let known = match choices {
        ModelChoices::Known { option_id, choices } => Some((option_id.clone(), choices.clone())),
        ModelChoices::Unknown => None,
    };
    let disabled = known.is_none();
    let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
        let Some((option_id, choices)) = known.clone() else {
            return menu;
        };
        for choice in choices {
            let name = if choice.agent_default {
                format!("{} (agent default)", choice.name)
            } else {
                choice.name.clone()
            };
            let is_current = chosen_id.as_deref() == Some(choice.id.as_str());
            let option_id = option_id.clone();
            let value_id = choice.id.clone();
            menu =
                menu.toggleable_entry(name, is_current, IconPosition::Start, None, move |_, cx| {
                    let option_id = option_id.clone();
                    let value = AgentConfigOptionValue::ValueId(value_id.clone());
                    update_agent_settings(cx, move |agent| {
                        set_agent_option(agent, &option_id, Some(value))
                    });
                });
        }
        menu
    });
    DropdownMenu::new("dictation-post-processing-agent-model", label, menu)
        .style(DropdownStyle::Outlined)
        .disabled(disabled)
        .into_any_element()
}

/// One announced option, edited as the agent declared it: a select as a
/// list of its values with «Agent default» to unset it, a boolean as a
/// switch. Rendered by hand because the title is the agent's, not a
/// static string.
fn render_agent_option_row(
    option: &DictationAgentOptionContent,
    chosen: Option<&AgentConfigOptionValue>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let option_id = option.id.clone();
    let control: AnyElement = match &option.kind {
        DictationAgentOptionKindContent::Select { values, .. } => {
            let chosen_id = chosen
                .and_then(AgentConfigOptionValue::as_value_id)
                .map(str::to_string);
            let label: SharedString = chosen_id
                .as_ref()
                .and_then(|id| values.iter().find(|value| &value.id == id))
                .map(|value| value.name.clone())
                .or_else(|| chosen_id.clone())
                .unwrap_or_else(|| AGENT_OPTION_DEFAULT_LABEL.to_string())
                .into();
            let values = values.clone();
            let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
                menu = menu.toggleable_entry(
                    AGENT_OPTION_DEFAULT_LABEL,
                    chosen_id.is_none(),
                    IconPosition::Start,
                    None,
                    {
                        let option_id = option_id.clone();
                        move |_, cx| {
                            let option_id = option_id.clone();
                            update_agent_settings(cx, move |agent| {
                                set_agent_option(agent, &option_id, None)
                            });
                        }
                    },
                );
                for value in &values {
                    let is_current = chosen_id.as_deref() == Some(value.id.as_str());
                    let option_id = option_id.clone();
                    let value_id = value.id.clone();
                    menu = menu.toggleable_entry(
                        value.name.clone(),
                        is_current,
                        IconPosition::Start,
                        None,
                        move |_, cx| {
                            let option_id = option_id.clone();
                            let value = AgentConfigOptionValue::ValueId(value_id.clone());
                            update_agent_settings(cx, move |agent| {
                                set_agent_option(agent, &option_id, Some(value))
                            });
                        },
                    );
                }
                menu
            });
            DropdownMenu::new(
                gpui::ElementId::Name(format!("dictation-agent-option-{}", option.id).into()),
                label,
                menu,
            )
            .style(DropdownStyle::Outlined)
            .into_any_element()
        }
        DictationAgentOptionKindContent::Boolean { current } => {
            let state = chosen
                .and_then(AgentConfigOptionValue::as_bool)
                .unwrap_or(*current);
            SwitchField::new(
                gpui::ElementId::Name(format!("dictation-agent-option-{}", option.id).into()),
                None::<SharedString>,
                None,
                if state {
                    ToggleState::Selected
                } else {
                    ToggleState::Unselected
                },
                move |state, _, cx| {
                    let option_id = option_id.clone();
                    let value = AgentConfigOptionValue::Boolean(*state == ToggleState::Selected);
                    update_agent_settings(cx, move |agent| {
                        set_agent_option(agent, &option_id, Some(value))
                    });
                },
            )
            .into_any_element()
        }
    };
    h_flex()
        .w_full()
        .pt_4()
        .pb_4()
        .gap_4()
        .justify_between()
        .child(
            v_flex()
                .gap_0p5()
                .child(Label::new(option.name.clone()))
                .child(
                    Label::new(format!(
                        "Session option `{}` as the agent announced it.",
                        option.id
                    ))
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .render_code_spans(),
                ),
        )
        .child(control)
        .into_any_element()
}

fn render_post_processing_model_rows(
    settings_window: &SettingsWindow,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let selection = AgentSettings::get_global(cx)
        .dictation
        .post_processing_model
        .clone();
    let pending = window.use_keyed_state("dictation-pending-provider", cx, |_, _| {
        PendingProvider::None
    });
    let shown_provider_id = selection
        .as_ref()
        .map(|selection| selection.provider.0.clone())
        .or_else(|| pending.read(cx).clone());
    let shown_provider = shown_provider_id.as_ref().and_then(|id| {
        LanguageModelRegistry::read_global(cx).provider(&LanguageModelProviderId(id.clone().into()))
    });
    if let Some(provider) = &shown_provider {
        ensure_provider_authenticated(provider, window, cx);
    }

    let provider_dropdown = render_provider_dropdown(
        selection.clone(),
        shown_provider_id,
        pending.clone(),
        cx.weak_entity(),
        window,
        cx,
    );
    let model_dropdown = render_model_dropdown(
        selection,
        shown_provider,
        pending,
        cx.weak_entity(),
        window,
        cx,
    );

    let provider_row = render_settings_item_layout(
        settings_window,
        "Provider",
        "Language model provider for post-processing. The agent's default model is used when none is chosen.",
        provider_dropdown,
        None,
        None,
        Some("agent.dictation.post_processing.model"),
        false,
        cx,
    )
    .pt_4()
    .pb_4();
    let model_row = render_settings_item_layout(
        settings_window,
        "Model",
        "Model of the chosen provider used for post-processing.",
        model_dropdown,
        None,
        None,
        Some("agent.dictation.post_processing.model"),
        false,
        cx,
    )
    .pt_4()
    .pb_4();

    v_flex()
        .child(provider_row)
        .child(Divider::horizontal())
        .child(model_row)
        .child(Divider::horizontal())
        .into_any_element()
}

fn render_provider_dropdown(
    selection: Option<LanguageModelSelection>,
    shown_provider_id: Option<String>,
    pending: Entity<PendingProvider>,
    settings_window: WeakEntity<SettingsWindow>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let providers = LanguageModelRegistry::read_global(cx).visible_providers();
    let label = shown_provider_id
        .as_ref()
        .map(|id| {
            providers
                .iter()
                .find(|provider| provider.id().0.as_ref() == id.as_str())
                .map(|provider| provider.name().0)
                .unwrap_or_else(|| id.clone().into())
        })
        .unwrap_or_else(|| AGENT_DEFAULT_MODEL_LABEL.into());

    let menu = ContextMenu::build(window, cx, move |mut menu, _, cx| {
        menu = menu.toggleable_entry(
            AGENT_DEFAULT_MODEL_LABEL,
            shown_provider_id.is_none(),
            IconPosition::Start,
            None,
            {
                let pending = pending.clone();
                let settings_window = settings_window.clone();
                move |_, cx| {
                    set_pending_provider(&pending, &settings_window, None, cx);
                    write_post_processing_model(None, cx);
                }
            },
        );
        for provider in &providers {
            let id = provider.id().0.to_string();
            let is_current = shown_provider_id.as_deref() == Some(id.as_str());
            let models = model_ids(provider.as_ref(), cx);
            let preferred = provider
                .default_model(cx)
                .map(|model| model.id().0.to_string());
            let selection = selection.clone();
            let pending = pending.clone();
            let settings_window = settings_window.clone();
            menu = menu.toggleable_entry(
                provider.name().0,
                is_current,
                IconPosition::Start,
                None,
                move |_, cx| {
                    let new_selection = selection_for_provider(
                        &id,
                        &models,
                        preferred.as_deref(),
                        selection.as_ref(),
                    );
                    match new_selection {
                        Some(new_selection) => {
                            set_pending_provider(&pending, &settings_window, None, cx);
                            if selection.as_ref() != Some(&new_selection) {
                                write_post_processing_model(Some(new_selection), cx);
                            }
                        }
                        None => {
                            set_pending_provider(&pending, &settings_window, Some(id.clone()), cx);
                            if selection.is_some() {
                                write_post_processing_model(None, cx);
                            }
                        }
                    }
                },
            );
        }
        menu
    });

    DropdownMenu::new("dictation-post-processing-provider", label, menu)
        .style(DropdownStyle::Outlined)
        .into_any_element()
}

fn render_model_dropdown(
    selection: Option<LanguageModelSelection>,
    provider: Option<Arc<dyn LanguageModelProvider>>,
    pending: Entity<PendingProvider>,
    settings_window: WeakEntity<SettingsWindow>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let models = provider
        .as_ref()
        .map(|provider| provider.provided_models(cx))
        .unwrap_or_default();
    let current_model = selection.map(|selection| selection.model);
    let provider_id = provider
        .as_ref()
        .map(|provider| provider.id().0.to_string());

    let label: SharedString = match (&provider_id, &current_model) {
        (None, _) => AGENT_DEFAULT_MODEL_LABEL.into(),
        (Some(_), _) if models.is_empty() => "No models available".into(),
        (Some(_), Some(model)) => models
            .iter()
            .find(|candidate| candidate.id().0.as_ref() == model.as_str())
            .map(|candidate| candidate.name().0)
            .unwrap_or_else(|| model.clone().into()),
        (Some(_), None) => "Select a model…".into(),
    };
    let disabled = provider_id.is_none() || models.is_empty();

    let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
        let Some(provider_id) = provider_id else {
            return menu;
        };
        for model in &models {
            let model_id = model.id().0.to_string();
            let is_current = current_model.as_deref() == Some(model_id.as_str());
            let provider_id = provider_id.clone();
            let pending = pending.clone();
            let settings_window = settings_window.clone();
            menu = menu.toggleable_entry(
                model.name().0,
                is_current,
                IconPosition::Start,
                None,
                move |_, cx| {
                    set_pending_provider(&pending, &settings_window, None, cx);
                    write_post_processing_model(
                        Some(selection_for_model(&provider_id, &model_id)),
                        cx,
                    );
                },
            );
        }
        menu
    });

    DropdownMenu::new("dictation-post-processing-model", label, menu)
        .style(DropdownStyle::Outlined)
        .disabled(disabled)
        .into_any_element()
}

/// The multi-line prompt editor. It outlives a single render through
/// `use_keyed_state`, so user edits are kept while the page re-renders.
struct PromptEditor {
    editor: Entity<Editor>,
    /// The prompt last taken from settings; the editor is only overwritten
    /// from settings when it still shows this text or is not focused.
    synced: String,
    save_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PromptEditor {
    fn new(prompt: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme_settings = ThemeSettings::get_global(cx);
        let text_style = TextStyleRefinement {
            font_family: Some(theme_settings.buffer_font.family.clone()),
            font_size: Some(rems(0.875).into()),
            ..Default::default()
        };
        let editor = cx.new(|cx| {
            let mut editor = Editor::auto_height(4, 20, window, cx);
            editor.set_soft_wrap_mode(SoftWrap::EditorWidth, cx);
            editor.set_show_gutter(false, cx);
            editor.set_show_wrap_guides(false, cx);
            editor.set_show_indent_guides(false, cx);
            editor.set_text_style_refinement(text_style);
            editor.set_text(prompt.clone(), window, cx);
            editor
        });
        let focus_handle = editor.focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &editor,
                window,
                |this, _, event: &EditorEvent, window, cx| {
                    if matches!(event, EditorEvent::BufferEdited) {
                        this.schedule_save(window, cx);
                    }
                },
            ),
            cx.on_focus_out(&focus_handle, window, |this, _, _, cx| {
                this.save(cx);
            }),
        ];
        Self {
            editor,
            synced: prompt,
            save_task: None,
            _subscriptions: subscriptions,
        }
    }

    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(PROMPT_SAVE_DEBOUNCE).await;
            this.update(cx, |this, cx| this.save(cx)).ok();
        }));
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        self.save_task = None;
        let text = self.editor.read(cx).text(cx);
        if text == self.synced {
            return;
        }
        self.synced = text.clone();
        update_agent_settings(cx, move |agent| {
            set_post_processing_prompt(agent, Some(text))
        });
    }

    /// Applies a prompt that changed in settings.json behind the editor's back.
    fn sync(&mut self, prompt: String, window: &mut Window, cx: &mut Context<Self>) {
        if prompt == self.synced {
            return;
        }
        let editor_text = self.editor.read(cx).text(cx);
        let has_unsaved_edits = editor_text != self.synced;
        if self.editor.read(cx).is_focused(window) && has_unsaved_edits {
            return;
        }
        self.set_text(prompt, window, cx);
    }

    fn set_text(&mut self, prompt: String, window: &mut Window, cx: &mut Context<Self>) {
        self.save_task = None;
        self.synced = prompt.clone();
        self.editor.update(cx, |editor, cx| {
            editor.set_text(prompt, window, cx);
        });
    }
}

fn default_prompt(cx: &App) -> String {
    post_processing_settings(SettingsStore::global(cx).raw_default_settings())
        .and_then(|post_processing| post_processing.prompt.clone())
        .unwrap_or_default()
}

fn render_prompt_section(window: &mut Window, cx: &mut Context<SettingsWindow>) -> AnyElement {
    let prompt = AgentSettings::get_global(cx)
        .dictation
        .post_processing_prompt
        .clone();
    let state = window.use_keyed_state("dictation-post-processing-prompt", cx, {
        let prompt = prompt.clone();
        move |window, cx| PromptEditor::new(prompt, window, cx)
    });
    state.update(cx, |state, cx| state.sync(prompt.clone(), window, cx));

    let is_default = prompt == default_prompt(cx);
    let editor = state.read(cx).editor.clone();
    let focus_handle = editor.focus_handle(cx);
    let theme_colors = cx.theme().colors();

    let reset_button = IconButton::new("dictation-prompt-reset", IconName::Undo)
        .icon_color(Color::Muted)
        .icon_size(IconSize::Small)
        .disabled(is_default)
        .tooltip(Tooltip::text("Reset to Default"))
        .on_click({
            let state = state.clone();
            move |_, window, cx| {
                let default = default_prompt(cx);
                state.update(cx, |state, cx| state.set_text(default, window, cx));
                update_agent_settings(cx, |agent| set_post_processing_prompt(agent, None));
            }
        });

    v_flex()
        .px_8()
        .pt_4()
        .gap_0p5()
        .child(
            h_flex()
                .gap_1()
                .child(Label::new("Prompt"))
                .child(reset_button),
        )
        .child(
            Label::new(PROMPT_DESCRIPTION)
                .size(LabelSize::Small)
                .color(Color::Muted)
                .render_code_spans(),
        )
        .child(
            div()
                .id("dictation-post-processing-prompt-editor")
                .mt_2()
                .w_full()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(theme_colors.border)
                .bg(theme_colors.editor_background)
                .track_focus(&focus_handle.tab_index(0).tab_stop(true))
                .focus(|style| style.border_color(theme_colors.border_focused))
                .child(editor),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glossary(terms: &[&str]) -> Vec<String> {
        terms.iter().map(|term| term.to_string()).collect()
    }

    #[test]
    fn adding_a_term_appends_it_once_and_keeps_order() {
        let base = glossary(&["Rust", "Docker"]);
        assert_eq!(
            glossary_with_term(&base, "  GitHub "),
            glossary(&["Rust", "Docker", "GitHub"])
        );
        assert_eq!(glossary_with_term(&base, "Docker"), base);
        assert_eq!(glossary_with_term(&base, "   "), base);
    }

    #[test]
    fn a_comma_separated_list_becomes_one_term_per_part() {
        assert_eq!(
            split_terms("TypeScript, Docker, Kubernetes"),
            glossary(&["TypeScript", "Docker", "Kubernetes"])
        );
        assert_eq!(
            split_terms("  Rust ,, Docker,\nRust, \n , GitHub "),
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
    fn removing_a_term_removes_only_that_term() {
        let base = glossary(&["Rust", "Docker", "GitHub"]);
        assert_eq!(
            glossary_without_term(&base, "Docker"),
            glossary(&["Rust", "GitHub"])
        );
        assert_eq!(glossary_without_term(&base, "Ruby"), base);
    }

    #[test]
    fn glossary_is_written_under_agent_dictation() {
        let mut agent = AgentSettingsContent::default();
        set_glossary(&mut agent, glossary(&["Rust"]));
        let written = agent
            .dictation
            .as_ref()
            .and_then(|dictation| dictation.glossary.as_ref());
        assert_eq!(written, Some(&glossary(&["Rust"])));
    }

    #[test]
    fn post_processing_model_is_written_like_the_agent_default_model() {
        let mut agent = AgentSettingsContent::default();
        set_post_processing_model(&mut agent, Some(selection_for_model("ollama", "qwen3:14b")));
        let json = serde_json::to_value(&agent).unwrap();
        let model = &json["dictation"]["post_processing"]["model"];
        assert_eq!(model["provider"], serde_json::json!("ollama"));
        assert_eq!(model["model"], serde_json::json!("qwen3:14b"));
    }

    #[test]
    fn clearing_the_provider_removes_the_model_but_keeps_other_fields() {
        let mut agent = AgentSettingsContent::default();
        post_processing_content(&mut agent).enabled = Some(false);
        set_post_processing_model(&mut agent, Some(selection_for_model("ollama", "qwen3:14b")));
        set_post_processing_model(&mut agent, None);
        let post_processing = agent.dictation.unwrap().post_processing.unwrap();
        assert_eq!(post_processing.model, None);
        assert_eq!(post_processing.enabled, Some(false));
    }

    #[test]
    fn choosing_a_provider_keeps_the_model_only_when_it_belongs_to_that_provider() {
        let current = selection_for_model("ollama", "qwen3:14b");
        let models = glossary(&["llama3", "qwen3:14b"]);

        let same_provider =
            selection_for_provider("ollama", &models, Some("llama3"), Some(&current));
        assert_eq!(
            same_provider.map(|selection| selection.model),
            Some("qwen3:14b".to_string())
        );

        let other_provider =
            selection_for_provider("anthropic", &models, Some("qwen3:14b"), Some(&current))
                .unwrap();
        assert_eq!(other_provider.provider.0, "anthropic");
        assert_eq!(other_provider.model, "qwen3:14b");

        let no_preference = selection_for_provider("anthropic", &models, None, None);
        assert_eq!(
            no_preference.map(|selection| selection.model),
            Some("llama3".to_string())
        );

        let no_models = selection_for_provider("anthropic", &[], Some("claude"), None);
        assert_eq!(no_models, None);
    }

    fn cached(options: &[DictationAgentOptionContent]) -> Option<&[DictationAgentOptionContent]> {
        Some(options)
    }

    fn model_option() -> DictationAgentOptionContent {
        DictationAgentOptionContent {
            id: "model".into(),
            name: "Model".into(),
            category: Some("model".into()),
            kind: DictationAgentOptionKindContent::Select {
                current: "default".into(),
                values: vec![
                    settings::DictationAgentOptionValueContent {
                        id: "default".into(),
                        name: "Default (recommended)".into(),
                    },
                    settings::DictationAgentOptionValueContent {
                        id: "haiku".into(),
                        name: "Haiku".into(),
                    },
                ],
            },
        }
    }

    fn fast_option() -> DictationAgentOptionContent {
        DictationAgentOptionContent {
            id: "fast".into(),
            name: "Fast mode".into(),
            category: None,
            kind: DictationAgentOptionKindContent::Boolean { current: false },
        }
    }

    fn mode_option() -> DictationAgentOptionContent {
        DictationAgentOptionContent {
            id: "permission-mode".into(),
            name: "Permissions".into(),
            category: Some("mode".into()),
            kind: DictationAgentOptionKindContent::Select {
                current: "default".into(),
                values: vec![],
            },
        }
    }

    fn post_processing_of(agent: &AgentSettingsContent) -> &DictationPostProcessingSettingsContent {
        agent
            .dictation
            .as_ref()
            .and_then(|dictation| dictation.post_processing.as_ref())
            .expect("post_processing should be written")
    }

    #[test]
    fn the_branch_follows_the_keys_and_both_at_once_is_a_conflict() {
        assert_eq!(rewriter_choice(None), Ok(Rewriter::LanguageModel));
        let mut content = DictationPostProcessingSettingsContent::default();
        assert_eq!(rewriter_choice(Some(&content)), Ok(Rewriter::LanguageModel));

        content.model = Some(selection_for_model("ollama", "qwen3:14b"));
        assert_eq!(rewriter_choice(Some(&content)), Ok(Rewriter::LanguageModel));

        content.agent = Some(settings::DictationPostProcessingAgentSettingsContent::default());
        assert_eq!(rewriter_choice(Some(&content)), Err(RewriterConflict));

        content.model = None;
        assert_eq!(
            rewriter_choice(Some(&content)),
            Ok(Rewriter::ExternalAgent),
            "an agent block without an id is still the agent branch"
        );
    }

    #[test]
    fn choosing_a_rewriter_drops_the_other_ones_key() {
        let mut agent = AgentSettingsContent::default();
        set_post_processing_model(&mut agent, Some(selection_for_model("ollama", "qwen3:14b")));

        set_rewriter(&mut agent, Rewriter::ExternalAgent, Some("claude-acp"));
        let post_processing = post_processing_of(&agent);
        assert_eq!(post_processing.model, None);
        assert_eq!(
            post_processing
                .agent
                .as_ref()
                .and_then(|agent| agent.id.as_deref()),
            Some("claude-acp"),
            "the first configured agent is chosen so the settings are valid at once"
        );

        set_agent_option(
            &mut agent,
            "model",
            Some(AgentConfigOptionValue::ValueId("haiku".into())),
        );
        set_rewriter(&mut agent, Rewriter::ExternalAgent, Some("gemini"));
        assert_eq!(
            post_processing_of(&agent)
                .agent
                .as_ref()
                .and_then(|agent| agent.id.as_deref()),
            Some("claude-acp"),
            "choosing the agent branch again keeps the chosen agent"
        );

        set_post_processing_model(&mut agent, Some(selection_for_model("ollama", "qwen3:14b")));
        set_rewriter(&mut agent, Rewriter::LanguageModel, None);
        let post_processing = post_processing_of(&agent);
        assert_eq!(post_processing.agent, None);
        assert!(post_processing.model.is_some());

        let mut none_configured = AgentSettingsContent::default();
        set_rewriter(&mut none_configured, Rewriter::ExternalAgent, None);
        let chosen = post_processing_of(&none_configured)
            .agent
            .as_ref()
            .expect("the agent block is written even with no agents to choose from");
        assert_eq!(chosen.id, None);
    }

    #[test]
    fn picking_an_agent_writes_its_id_and_drops_the_model_key() {
        let mut agent = AgentSettingsContent::default();
        set_post_processing_model(&mut agent, Some(selection_for_model("ollama", "qwen3:14b")));
        set_post_processing_agent_id(&mut agent, "claude-acp".into());
        let json = serde_json::to_value(&agent).unwrap();
        assert_eq!(
            json["dictation"]["post_processing"]["agent"]["id"],
            serde_json::json!("claude-acp")
        );
        assert!(json["dictation"]["post_processing"].get("model").is_none());
    }

    #[test]
    fn agent_options_are_written_by_id_and_removed_by_none() {
        let mut agent = AgentSettingsContent::default();
        set_agent_option(
            &mut agent,
            "model",
            Some(AgentConfigOptionValue::ValueId("haiku".into())),
        );
        set_agent_option(
            &mut agent,
            "fast",
            Some(AgentConfigOptionValue::Boolean(true)),
        );
        let json = serde_json::to_value(&agent).unwrap();
        let options = &json["dictation"]["post_processing"]["agent"]["options"];
        assert_eq!(options["model"], serde_json::json!("haiku"));
        assert_eq!(options["fast"], serde_json::json!(true));

        set_agent_option(&mut agent, "fast", None);
        let json = serde_json::to_value(&agent).unwrap();
        let options = &json["dictation"]["post_processing"]["agent"]["options"];
        assert!(options.get("fast").is_none());
        assert_eq!(options["model"], serde_json::json!("haiku"));
    }

    #[test]
    fn the_model_list_comes_from_the_cache_with_the_agents_default_marked() {
        let options = [model_option(), fast_option()];
        assert_eq!(
            model_choices(cached(&options)),
            ModelChoices::Known {
                option_id: "model".into(),
                choices: vec![
                    ModelChoice {
                        id: "default".into(),
                        name: "Default (recommended)".into(),
                        agent_default: true,
                    },
                    ModelChoice {
                        id: "haiku".into(),
                        name: "Haiku".into(),
                        agent_default: false,
                    },
                ],
            }
        );
    }

    #[test]
    fn without_a_cache_the_models_are_not_known_rather_than_empty() {
        assert_eq!(model_choices(None), ModelChoices::Unknown);
        let without_model = [fast_option()];
        assert_eq!(model_choices(cached(&without_model)), ModelChoices::Unknown);
        assert_eq!(
            model_label(&ModelChoices::Unknown, None),
            MODELS_UNKNOWN_LABEL
        );
    }

    #[test]
    fn the_model_label_is_the_human_name_or_the_agents_default() {
        let options = [model_option()];
        let choices = model_choices(cached(&options));
        assert_eq!(model_label(&choices, None), AGENT_DEFAULT_MODEL_LABEL);
        assert_eq!(
            model_label(
                &choices,
                Some(&AgentConfigOptionValue::ValueId("haiku".into()))
            ),
            "Haiku"
        );
        assert_eq!(
            model_label(
                &choices,
                Some(&AgentConfigOptionValue::ValueId("opus[1m]".into()))
            ),
            "opus[1m]",
            "a value the cache does not know is shown as written"
        );
    }

    #[test]
    fn the_other_options_leave_out_the_model_and_the_mode() {
        let options = [model_option(), mode_option(), fast_option()];
        let other: Vec<&str> = other_agent_options(&options)
            .into_iter()
            .map(|option| option.id.as_str())
            .collect();
        assert_eq!(other, vec!["fast"]);
    }

    #[test]
    fn configured_agents_are_listed_in_a_stable_order() {
        let mut settings = AllAgentServersSettings::default();
        settings.insert(
            "gemini".into(),
            project::agent_server_store::CustomAgentServerSettings::Registry {
                env: Default::default(),
                default_mode: None,
                default_config_options: Default::default(),
                favorite_config_option_values: Default::default(),
            },
        );
        settings.insert(
            "claude-acp".into(),
            project::agent_server_store::CustomAgentServerSettings::Registry {
                env: Default::default(),
                default_mode: None,
                default_config_options: Default::default(),
                favorite_config_option_values: Default::default(),
            },
        );
        assert_eq!(configured_agents(&settings), vec!["claude-acp", "gemini"]);
    }

    #[test]
    fn resetting_the_prompt_removes_the_override() {
        let mut agent = AgentSettingsContent::default();
        set_post_processing_prompt(&mut agent, Some("custom".into()));
        assert_eq!(
            post_processing_content(&mut agent).prompt.as_deref(),
            Some("custom")
        );
        set_post_processing_prompt(&mut agent, None);
        assert_eq!(post_processing_content(&mut agent).prompt, None);
    }
}
