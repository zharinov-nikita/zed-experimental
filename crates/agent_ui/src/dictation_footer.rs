//! Local: what the footer of the Dictation Window shows, computed from the
//! phase of the session and the tasks still running. The render code only
//! draws the result, so every rule about which hint is enabled when lives
//! here and is tested here.

use dictation::OpenedInputDevice;

/// Where the Dictation Session is, as far as the footer is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterPhase {
    /// Model Loading and opening the microphone.
    Starting,
    Recording,
    /// Recording stopped, the tail is being recognized.
    Recognizing,
    /// The transcript is shown but Post-processing is still rewriting it.
    PostProcessing,
    Review,
    Failed,
}

/// Everything the footer needs to know that is not the phase itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FooterInput {
    pub phase: FooterPhase,
    /// The model that rewrote the text; `None` when Post-processing was
    /// off, failed or returned nothing.
    pub processed_by: Option<ProcessedBy>,
    /// Whether the raw text is shown instead of the processed one.
    pub show_raw: bool,
    /// Session Audio for this block exists and can be played.
    pub session_audio_available: bool,
    pub playing: bool,
    /// The input the session records from; `None` outside recording.
    pub microphone: Option<OpenedInputDevice>,
    /// Post-processing is waiting for the Ollama server Zed has started.
    pub model_server_starting: bool,
}

/// The model that actually rewrote the text, including the fallback to
/// the agent's default model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedBy {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterAction {
    Accept,
    Cancel,
    ToggleRaw,
    TogglePlayback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub action: FooterAction,
    pub label: &'static str,
    pub enabled: bool,
}

/// The label on the left of the footer, after the timer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FooterLabel {
    /// Recording into this device; the full text goes into the tooltip.
    Microphone(String),
    Raw,
    Processed(ProcessedBy),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FooterState {
    pub recording_indicator: bool,
    /// Text next to a spinner while the user has to wait.
    pub spinner: Option<&'static str>,
    pub label: Option<FooterLabel>,
    pub hints: Vec<Hint>,
}

impl FooterState {
    pub fn is_enabled(&self, action: FooterAction) -> bool {
        self.hints
            .iter()
            .any(|hint| hint.action == action && hint.enabled)
    }
}

const CONFIGURED_DEVICE_MISSING: &str = "configured device not found";

pub fn microphone_label(microphone: &OpenedInputDevice) -> String {
    if microphone.configured_device_missing {
        format!("{} · {CONFIGURED_DEVICE_MISSING}", microphone.name)
    } else {
        microphone.name.clone()
    }
}

fn hint(action: FooterAction, label: &'static str, enabled: bool) -> Hint {
    Hint {
        action,
        label,
        enabled,
    }
}

pub fn footer_state(input: &FooterInput) -> FooterState {
    let review_label = || match &input.processed_by {
        Some(processed_by) if !input.show_raw => FooterLabel::Processed(processed_by.clone()),
        _ => FooterLabel::Raw,
    };
    let raw_toggle = |enabled: bool| {
        input.processed_by.as_ref().map(|_| {
            hint(
                FooterAction::ToggleRaw,
                if input.show_raw {
                    "Show Processed"
                } else {
                    "Show Raw"
                },
                enabled,
            )
        })
    };
    let playback = || {
        input.session_audio_available.then(|| {
            hint(
                FooterAction::TogglePlayback,
                if input.playing { "Stop" } else { "Play" },
                true,
            )
        })
    };

    match input.phase {
        FooterPhase::Starting => FooterState {
            recording_indicator: false,
            spinner: Some("Loading Whisper model…"),
            label: None,
            hints: vec![hint(FooterAction::Cancel, "Cancel", true)],
        },
        FooterPhase::Recording => FooterState {
            recording_indicator: true,
            spinner: None,
            label: input
                .microphone
                .as_ref()
                .map(|microphone| FooterLabel::Microphone(microphone_label(microphone))),
            hints: vec![hint(FooterAction::Cancel, "Review", true)],
        },
        FooterPhase::Recognizing => FooterState {
            recording_indicator: false,
            spinner: Some("Recognizing…"),
            label: None,
            hints: vec![
                hint(FooterAction::Accept, "Accept", false),
                hint(FooterAction::Cancel, "Cancel", true),
            ],
        },
        FooterPhase::PostProcessing => FooterState {
            recording_indicator: false,
            spinner: Some(if input.model_server_starting {
                "Starting Ollama…"
            } else {
                "Post-processing…"
            }),
            label: Some(FooterLabel::Raw),
            hints: [
                Some(hint(FooterAction::Accept, "Accept", false)),
                Some(hint(FooterAction::ToggleRaw, "Show Processed", false)),
                Some(hint(FooterAction::Cancel, "Cancel", true)),
            ]
            .into_iter()
            .flatten()
            .collect(),
        },
        FooterPhase::Review => FooterState {
            recording_indicator: false,
            spinner: None,
            label: Some(review_label()),
            hints: [
                Some(hint(FooterAction::Accept, "Accept", true)),
                raw_toggle(true),
                playback(),
                Some(hint(FooterAction::Cancel, "Cancel", true)),
            ]
            .into_iter()
            .flatten()
            .collect(),
        },
        FooterPhase::Failed => FooterState {
            recording_indicator: false,
            spinner: None,
            label: None,
            hints: vec![hint(FooterAction::Cancel, "Close", true)],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(phase: FooterPhase) -> FooterInput {
        FooterInput {
            phase,
            processed_by: None,
            show_raw: true,
            session_audio_available: false,
            playing: false,
            microphone: None,
            model_server_starting: false,
        }
    }

    fn qwen() -> ProcessedBy {
        ProcessedBy {
            provider: "Ollama".into(),
            model: "qwen3:14b".into(),
        }
    }

    fn labels(state: &FooterState) -> Vec<(&'static str, bool)> {
        state
            .hints
            .iter()
            .map(|hint| (hint.label, hint.enabled))
            .collect()
    }

    #[test]
    fn starting_offers_only_cancel_and_shows_the_model_loading() {
        let state = footer_state(&input(FooterPhase::Starting));
        assert_eq!(labels(&state), vec![("Cancel", true)]);
        assert_eq!(state.spinner, Some("Loading Whisper model…"));
        assert!(!state.recording_indicator);
        assert_eq!(state.label, None);
    }

    #[test]
    fn recording_shows_the_microphone_and_a_review_hint() {
        let mut input = input(FooterPhase::Recording);
        input.microphone = Some(OpenedInputDevice {
            name: "Microphone (fifine Microphone)".into(),
            configured_device_missing: false,
        });
        let state = footer_state(&input);
        assert_eq!(labels(&state), vec![("Review", true)]);
        assert!(state.recording_indicator);
        assert_eq!(state.spinner, None);
        assert_eq!(
            state.label,
            Some(FooterLabel::Microphone(
                "Microphone (fifine Microphone)".into()
            ))
        );
    }

    #[test]
    fn a_missing_configured_device_is_named_in_the_microphone_label() {
        let mut input = input(FooterPhase::Recording);
        input.microphone = Some(OpenedInputDevice {
            name: "Headset Microphone".into(),
            configured_device_missing: true,
        });
        let state = footer_state(&input);
        assert_eq!(
            state.label,
            Some(FooterLabel::Microphone(
                "Headset Microphone · configured device not found".into()
            ))
        );
    }

    #[test]
    fn the_microphone_label_is_absent_outside_recording() {
        for phase in [
            FooterPhase::Starting,
            FooterPhase::Recognizing,
            FooterPhase::PostProcessing,
            FooterPhase::Review,
            FooterPhase::Failed,
        ] {
            let mut input = input(phase);
            input.microphone = Some(OpenedInputDevice {
                name: "Mic".into(),
                configured_device_missing: false,
            });
            let state = footer_state(&input);
            assert!(
                !matches!(state.label, Some(FooterLabel::Microphone(_))),
                "{phase:?}"
            );
        }
    }

    #[test]
    fn recognizing_blocks_accept_and_keeps_cancel() {
        let state = footer_state(&input(FooterPhase::Recognizing));
        assert_eq!(labels(&state), vec![("Accept", false), ("Cancel", true)]);
        assert_eq!(state.spinner, Some("Recognizing…"));
        assert!(!state.is_enabled(FooterAction::Accept));
        assert!(state.is_enabled(FooterAction::Cancel));
        assert!(!state.is_enabled(FooterAction::ToggleRaw));
    }

    #[test]
    fn post_processing_blocks_accept_and_tab_and_keeps_cancel() {
        let state = footer_state(&input(FooterPhase::PostProcessing));
        assert_eq!(
            labels(&state),
            vec![
                ("Accept", false),
                ("Show Processed", false),
                ("Cancel", true)
            ]
        );
        assert_eq!(state.spinner, Some("Post-processing…"));
        assert_eq!(state.label, Some(FooterLabel::Raw));
        assert!(!state.is_enabled(FooterAction::Accept));
        assert!(!state.is_enabled(FooterAction::ToggleRaw));
        assert!(!state.is_enabled(FooterAction::TogglePlayback));
        assert!(state.is_enabled(FooterAction::Cancel));
    }

    #[test]
    fn waiting_for_ollama_names_the_wait_and_blocks_accept_and_tab_like_post_processing() {
        let mut waiting = input(FooterPhase::PostProcessing);
        waiting.model_server_starting = true;
        let state = footer_state(&waiting);
        assert_eq!(state.spinner, Some("Starting Ollama…"));
        assert_eq!(
            labels(&state),
            vec![
                ("Accept", false),
                ("Show Processed", false),
                ("Cancel", true)
            ]
        );
        assert!(!state.is_enabled(FooterAction::Accept));
        assert!(!state.is_enabled(FooterAction::ToggleRaw));
        assert!(state.is_enabled(FooterAction::Cancel));

        for phase in [
            FooterPhase::Starting,
            FooterPhase::Recording,
            FooterPhase::Recognizing,
            FooterPhase::Review,
            FooterPhase::Failed,
        ] {
            let mut waiting = input(phase);
            waiting.model_server_starting = true;
            assert_ne!(
                footer_state(&waiting).spinner,
                Some("Starting Ollama…"),
                "{phase:?}: the wait is only shown while Post-processing waits"
            );
        }
    }

    #[test]
    fn review_with_processed_text_names_the_model_and_offers_show_raw() {
        let mut input = input(FooterPhase::Review);
        input.processed_by = Some(qwen());
        input.show_raw = false;
        let state = footer_state(&input);
        assert_eq!(
            labels(&state),
            vec![("Accept", true), ("Show Raw", true), ("Cancel", true)]
        );
        assert_eq!(state.spinner, None);
        assert_eq!(state.label, Some(FooterLabel::Processed(qwen())));
        assert!(state.is_enabled(FooterAction::Accept));
        assert!(state.is_enabled(FooterAction::ToggleRaw));
    }

    #[test]
    fn review_showing_raw_text_after_tab_offers_show_processed() {
        let mut input = input(FooterPhase::Review);
        input.processed_by = Some(qwen());
        input.show_raw = true;
        let state = footer_state(&input);
        assert_eq!(
            labels(&state),
            vec![("Accept", true), ("Show Processed", true), ("Cancel", true)]
        );
        assert_eq!(state.label, Some(FooterLabel::Raw));
    }

    #[test]
    fn review_without_post_processing_is_raw_without_a_tab_hint() {
        let state = footer_state(&input(FooterPhase::Review));
        assert_eq!(labels(&state), vec![("Accept", true), ("Cancel", true)]);
        assert_eq!(state.label, Some(FooterLabel::Raw));
        assert!(!state.is_enabled(FooterAction::ToggleRaw));
    }

    #[test]
    fn play_is_offered_only_in_review_with_session_audio() {
        let mut review = input(FooterPhase::Review);
        review.session_audio_available = true;
        let state = footer_state(&review);
        assert_eq!(
            labels(&state),
            vec![("Accept", true), ("Play", true), ("Cancel", true)]
        );

        review.playing = true;
        let state = footer_state(&review);
        assert_eq!(
            labels(&state),
            vec![("Accept", true), ("Stop", true), ("Cancel", true)]
        );

        let mut without_audio = input(FooterPhase::Review);
        without_audio.session_audio_available = false;
        assert!(!footer_state(&without_audio).is_enabled(FooterAction::TogglePlayback));

        for phase in [
            FooterPhase::Starting,
            FooterPhase::Recording,
            FooterPhase::Recognizing,
            FooterPhase::PostProcessing,
            FooterPhase::Failed,
        ] {
            let mut input = input(phase);
            input.session_audio_available = true;
            assert!(
                !footer_state(&input).is_enabled(FooterAction::TogglePlayback),
                "{phase:?}"
            );
        }
    }

    #[test]
    fn failed_offers_only_close() {
        let state = footer_state(&input(FooterPhase::Failed));
        assert_eq!(labels(&state), vec![("Close", true)]);
        assert_eq!(state.spinner, None);
        assert_eq!(state.label, None);
    }
}
