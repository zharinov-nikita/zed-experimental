//! Local: the Settings > AI > Dictation sub-page. Every widget here edits
//! `agent.dictation` in settings.json (the microphone lives under
//! `audio.experimental.input_audio_device`); nothing is stored elsewhere.

use std::sync::Arc;
use std::time::Duration;

use agent_settings::AgentSettings;
use editor::{Editor, EditorEvent};
use gpui::{
    Entity, Focusable as _, ReadGlobal as _, ScrollHandle, Subscription, Task, TextStyleRefinement,
    WeakEntity, prelude::*,
};
use language::language_settings::SoftWrap;
use language_model::{LanguageModelProvider, LanguageModelProviderId, LanguageModelRegistry};
use settings::{
    AgentSettingsContent, AudioInputDeviceName, DictationPostProcessingSettingsContent,
    DictationSettingsContent, LanguageModelProviderSetting, LanguageModelSelection, Settings as _,
    SettingsContent, SettingsStore,
};
use theme_settings::ThemeSettings;
use ui::{
    ContextMenu, Disableable as _, Divider, DropdownMenu, DropdownStyle, IconPosition, Tooltip,
    prelude::*,
};
use util::ResultExt as _;

use crate::{
    SettingField, SettingItem, SettingsFieldMetadata, SettingsPageItem, SettingsWindow, USER,
    components::{SettingsInputField, SettingsSectionHeader},
    render_settings_item_layout,
};

const DEFAULT_STRING: String = String::new();
const DEFAULT_EMPTY_STRING: Option<&String> = Some(&DEFAULT_STRING);
const DEFAULT_AUDIO_INPUT: AudioInputDeviceName = AudioInputDeviceName(None);
const DEFAULT_EMPTY_AUDIO_INPUT: Option<&AudioInputDeviceName> = Some(&DEFAULT_AUDIO_INPUT);

const AGENT_DEFAULT_MODEL_LABEL: &str = "Agent default model";
const PROMPT_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

const GLOSSARY_DESCRIPTION: &str =
    "Terms the recognizer and post-processing should spell exactly as written, one per row.";
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

fn glossary_without_term(glossary: &[String], term: &str) -> Vec<String> {
    glossary
        .iter()
        .filter(|existing| existing.as_str() != term)
        .cloned()
        .collect()
}

/// Replaces `old` in place; an edit that produces an empty term or a term
/// already in the list drops the old row instead of duplicating it.
fn glossary_with_replaced_term(glossary: &[String], old: &str, new: &str) -> Vec<String> {
    let new = new.trim();
    if new.is_empty() || (new != old && glossary.iter().any(|existing| existing == new)) {
        return glossary_without_term(glossary, old);
    }
    glossary
        .iter()
        .map(|existing| {
            if existing == old {
                new.to_string()
            } else {
                existing.clone()
            }
        })
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
    ])
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
    let engine = render_items(&engine_items, window, cx);
    let flags = render_items(&flag_items, window, cx);
    let post_processing_enabled = render_items(&post_processing_items, window, cx);

    let glossary = render_glossary_section(cx);
    let model_rows = render_post_processing_model_rows(settings_window, window, cx);
    let prompt = render_prompt_section(window, cx);

    v_flex()
        .id("dictation-settings-page")
        .size_full()
        .pb_16()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
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
    let rows: Vec<AnyElement> = glossary
        .iter()
        .enumerate()
        .map(|(index, term)| render_glossary_row(index, term.clone(), cx))
        .collect();
    let add_input = render_add_glossary_input(cx);
    let is_empty = rows.is_empty();
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
                    this.child(v_flex().gap_1p5().children(rows))
                })
                .child(add_input),
        )
        .child(Divider::horizontal().mt_4())
        .into_any_element()
}

/// The row's element id includes the term because the input field caches
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

fn render_add_glossary_input(cx: &mut Context<SettingsWindow>) -> AnyElement {
    let settings_window = cx.entity().downgrade();

    SettingsInputField::new("dictation-glossary-new")
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
        .px_8()
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
    fn removing_a_term_removes_only_that_term() {
        let base = glossary(&["Rust", "Docker", "GitHub"]);
        assert_eq!(
            glossary_without_term(&base, "Docker"),
            glossary(&["Rust", "GitHub"])
        );
        assert_eq!(glossary_without_term(&base, "Ruby"), base);
    }

    #[test]
    fn replacing_a_term_keeps_its_row_in_place() {
        let base = glossary(&["Rust", "Docker", "GitHub"]);
        assert_eq!(
            glossary_with_replaced_term(&base, "Docker", " Podman "),
            glossary(&["Rust", "Podman", "GitHub"])
        );
        assert_eq!(glossary_with_replaced_term(&base, "Docker", "Docker"), base);
    }

    #[test]
    fn replacing_with_an_empty_or_duplicate_term_drops_the_row() {
        let base = glossary(&["Rust", "Docker", "GitHub"]);
        assert_eq!(
            glossary_with_replaced_term(&base, "Docker", ""),
            glossary(&["Rust", "GitHub"])
        );
        assert_eq!(
            glossary_with_replaced_term(&base, "Docker", "GitHub"),
            glossary(&["Rust", "GitHub"])
        );
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
