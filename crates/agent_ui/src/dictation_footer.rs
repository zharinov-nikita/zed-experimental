//! Local: what the footer of the Dictation Window shows, computed from the
//! phase of the session and the tasks still running. The render code only
//! draws the result, so every rule about which hint is enabled when, what
//! the spinner says and how a failed rewrite is explained lives here and
//! is tested here.

use dictation::OpenedInputDevice;
use ui::SharedString;

use crate::dictation_post_processing::{Failure, RESPONSE_LIMIT};

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
    /// The rewriter that produced the processed text; `None` when
    /// Post-processing was off, failed or returned nothing.
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
    /// The External Agent the answer is awaited from, named in the spinner.
    pub rewriting_with_agent: Option<String>,
    /// Why the last Post-processing left the text raw.
    pub post_processing_failure: Option<Failure>,
}

/// Which kind of rewriter produced the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewriterKind {
    LanguageModel,
    ExternalAgent,
}

/// The rewriter that actually produced the text: a provider and model,
/// including the fallback to the agent's default model, or an External
/// Agent and the model it was asked to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedBy {
    pub provider: String,
    pub model: String,
    pub kind: RewriterKind,
}

impl ProcessedBy {
    /// The short form after «Processed ·»: the model alone for a language
    /// model, the agent and the model for an External Agent, so a Haiku
    /// cleanup can be told from a Sonnet one.
    pub fn label(&self) -> String {
        match self.kind {
            RewriterKind::LanguageModel => self.model.clone(),
            RewriterKind::ExternalAgent => format!("{} · {}", self.provider, self.model),
        }
    }

    pub fn description(&self) -> String {
        format!("Rewritten by {} · {}", self.provider, self.model)
    }
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

/// A Callout shown in review: what went wrong and why the text is raw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: SharedString,
    pub description: SharedString,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FooterState {
    pub recording_indicator: bool,
    /// Text next to a spinner while the user has to wait.
    pub spinner: Option<SharedString>,
    pub label: Option<FooterLabel>,
    pub hints: Vec<Hint>,
    /// Why the text is raw, shown above it in review.
    pub notice: Option<Notice>,
}

impl FooterState {
    pub fn is_enabled(&self, action: FooterAction) -> bool {
        self.hints
            .iter()
            .any(|hint| hint.action == action && hint.enabled)
    }
}

const CONFIGURED_DEVICE_MISSING: &str = "configured device not found";
const TEXT_IS_RAW: &str = "The text is shown as recognized.";

pub fn microphone_label(microphone: &OpenedInputDevice) -> String {
    if microphone.configured_device_missing {
        format!("{} · {CONFIGURED_DEVICE_MISSING}", microphone.name)
    } else {
        microphone.name.clone()
    }
}

/// What the user is told about a rewrite that left the text raw. The
/// three agent failures are told apart by what was observed, not guessed
/// from the answer; what Zed cannot know (whether the agent is signed in)
/// is offered as a possible cause, not stated.
pub fn failure_notice(failure: &Failure) -> Notice {
    let notice = |title: &str, description: String| Notice {
        title: title.to_string().into(),
        description: description.into(),
    };
    match failure {
        Failure::NoText => notice(
            "Post-processing Unavailable",
            "Post-processing returned no text.".into(),
        ),
        Failure::ServerDidNotStart(reason) => {
            notice("Ollama did not start", format!("{reason} {TEXT_IS_RAW}"))
        }
        Failure::ModelUnavailable(reason) | Failure::RequestFailed(reason) => {
            notice("Post-processing Unavailable", reason.clone())
        }
        Failure::BothConfigured => notice(
            "Post-processing Misconfigured",
            format!(
                "Both `agent.dictation.post_processing.model` and `agent.dictation.post_processing.agent` are set; keep one of them. {TEXT_IS_RAW}"
            ),
        ),
        Failure::AgentUnavailable { agent, reason } => notice(
            "Agent Unavailable",
            format!(
                "{agent} could not be reached: it may not be installed, may have failed to start, or may not be signed in. {reason} {TEXT_IS_RAW}"
            ),
        ),
        Failure::AgentTriedToAct { agent } => notice(
            "Agent Tried to Act",
            format!(
                "{agent} asked to use a tool instead of answering with text, so the rewrite was refused. {TEXT_IS_RAW}"
            ),
        ),
        Failure::AgentRefused { agent } => notice(
            "Agent Refused",
            format!("{agent} declined to rewrite the text. {TEXT_IS_RAW}"),
        ),
        Failure::TimedOut { agent } => notice(
            "Post-processing Timed Out",
            format!(
                "{agent} did not answer within {} seconds. {TEXT_IS_RAW}",
                RESPONSE_LIMIT.as_secs()
            ),
        ),
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
    let post_processing_spinner = || -> SharedString {
        if input.model_server_starting {
            "Starting Ollama…".into()
        } else if let Some(agent) = &input.rewriting_with_agent {
            format!("Post-processing with {agent}…").into()
        } else {
            "Post-processing…".into()
        }
    };

    match input.phase {
        FooterPhase::Starting => FooterState {
            recording_indicator: false,
            spinner: Some("Loading Whisper model…".into()),
            label: None,
            hints: vec![hint(FooterAction::Cancel, "Cancel", true)],
            notice: None,
        },
        FooterPhase::Recording => FooterState {
            recording_indicator: true,
            spinner: None,
            label: input
                .microphone
                .as_ref()
                .map(|microphone| FooterLabel::Microphone(microphone_label(microphone))),
            hints: vec![hint(FooterAction::Cancel, "Review", true)],
            notice: None,
        },
        FooterPhase::Recognizing => FooterState {
            recording_indicator: false,
            spinner: Some("Recognizing…".into()),
            label: None,
            hints: vec![
                hint(FooterAction::Accept, "Accept", false),
                hint(FooterAction::Cancel, "Cancel", true),
            ],
            notice: None,
        },
        FooterPhase::PostProcessing => FooterState {
            recording_indicator: false,
            spinner: Some(post_processing_spinner()),
            label: Some(FooterLabel::Raw),
            hints: [
                Some(hint(FooterAction::Accept, "Accept", false)),
                Some(hint(FooterAction::ToggleRaw, "Show Processed", false)),
                Some(hint(FooterAction::Cancel, "Cancel", true)),
            ]
            .into_iter()
            .flatten()
            .collect(),
            notice: None,
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
            notice: input.post_processing_failure.as_ref().map(failure_notice),
        },
        FooterPhase::Failed => FooterState {
            recording_indicator: false,
            spinner: None,
            label: None,
            hints: vec![hint(FooterAction::Cancel, "Close", true)],
            notice: None,
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
            rewriting_with_agent: None,
            post_processing_failure: None,
        }
    }

    fn qwen() -> ProcessedBy {
        ProcessedBy {
            provider: "Ollama".into(),
            model: "qwen3:14b".into(),
            kind: RewriterKind::LanguageModel,
        }
    }

    fn claude_haiku() -> ProcessedBy {
        ProcessedBy {
            provider: "Claude Code".into(),
            model: "Haiku".into(),
            kind: RewriterKind::ExternalAgent,
        }
    }

    fn labels(state: &FooterState) -> Vec<(&'static str, bool)> {
        state
            .hints
            .iter()
            .map(|hint| (hint.label, hint.enabled))
            .collect()
    }

    fn spinner(state: &FooterState) -> Option<&str> {
        state.spinner.as_ref().map(|text| text.as_ref())
    }

    #[test]
    fn starting_offers_only_cancel_and_shows_the_model_loading() {
        let state = footer_state(&input(FooterPhase::Starting));
        assert_eq!(labels(&state), vec![("Cancel", true)]);
        assert_eq!(spinner(&state), Some("Loading Whisper model…"));
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
        assert_eq!(spinner(&state), Some("Recognizing…"));
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
        assert_eq!(spinner(&state), Some("Post-processing…"));
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
        assert_eq!(spinner(&state), Some("Starting Ollama…"));
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
                spinner(&footer_state(&waiting)),
                Some("Starting Ollama…"),
                "{phase:?}: the wait is only shown while Post-processing waits"
            );
        }
    }

    #[test]
    fn waiting_for_an_agent_names_the_agent() {
        let mut waiting = input(FooterPhase::PostProcessing);
        waiting.rewriting_with_agent = Some("Claude Code".into());
        let state = footer_state(&waiting);
        assert_eq!(spinner(&state), Some("Post-processing with Claude Code…"));
        assert_eq!(state.label, Some(FooterLabel::Raw));
        assert!(!state.is_enabled(FooterAction::Accept));

        for phase in [
            FooterPhase::Starting,
            FooterPhase::Recording,
            FooterPhase::Recognizing,
            FooterPhase::Review,
            FooterPhase::Failed,
        ] {
            let mut waiting = input(phase);
            waiting.rewriting_with_agent = Some("Claude Code".into());
            assert_ne!(
                spinner(&footer_state(&waiting)),
                Some("Post-processing with Claude Code…"),
                "{phase:?}"
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
        assert_eq!(state.notice, None);
    }

    #[test]
    fn the_processed_label_names_the_model_and_for_an_agent_the_agent_too() {
        assert_eq!(qwen().label(), "qwen3:14b");
        assert_eq!(qwen().description(), "Rewritten by Ollama · qwen3:14b");
        assert_eq!(claude_haiku().label(), "Claude Code · Haiku");
        assert_eq!(
            claude_haiku().description(),
            "Rewritten by Claude Code · Haiku"
        );

        let own_model = ProcessedBy {
            model: "default model".into(),
            ..claude_haiku()
        };
        assert_eq!(own_model.label(), "Claude Code · default model");
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

    fn notice_in_review(failure: Failure) -> Notice {
        let mut review = input(FooterPhase::Review);
        review.post_processing_failure = Some(failure);
        footer_state(&review)
            .notice
            .expect("a failed rewrite is explained in review")
    }

    #[test]
    fn an_unavailable_agent_an_attempt_to_act_and_a_timeout_are_told_apart() {
        let unavailable = notice_in_review(Failure::AgentUnavailable {
            agent: "Claude Code".into(),
            reason: "Server exited with status 1".into(),
        });
        assert_eq!(unavailable.title, "Agent Unavailable");
        assert!(
            unavailable
                .description
                .contains("Claude Code could not be reached")
        );
        assert!(
            unavailable.description.contains("may not be signed in"),
            "signing in is offered as a possible cause: {}",
            unavailable.description
        );
        assert!(
            unavailable
                .description
                .contains("Server exited with status 1")
        );

        let acted = notice_in_review(Failure::AgentTriedToAct {
            agent: "Claude Code".into(),
        });
        assert_eq!(acted.title, "Agent Tried to Act");
        assert!(
            acted
                .description
                .contains("Claude Code asked to use a tool")
        );

        let timed_out = notice_in_review(Failure::TimedOut {
            agent: "Claude Code".into(),
        });
        assert_eq!(timed_out.title, "Post-processing Timed Out");
        assert!(timed_out.description.contains("within 60 seconds"));

        let refused = notice_in_review(Failure::AgentRefused {
            agent: "Claude Code".into(),
        });
        assert_eq!(refused.title, "Agent Refused");

        for notice in [&unavailable, &acted, &timed_out, &refused] {
            assert!(
                notice
                    .description
                    .ends_with("The text is shown as recognized."),
                "{}",
                notice.description
            );
        }
        let titles: std::collections::HashSet<_> = [&unavailable, &acted, &timed_out, &refused]
            .into_iter()
            .map(|notice| notice.title.clone())
            .collect();
        assert_eq!(titles.len(), 4, "every cause has its own title");
    }

    #[test]
    fn the_language_model_failures_keep_their_texts_and_a_double_configuration_is_named() {
        let no_text = notice_in_review(Failure::NoText);
        assert_eq!(no_text.title, "Post-processing Unavailable");
        assert_eq!(no_text.description, "Post-processing returned no text.");

        let ollama = notice_in_review(Failure::ServerDidNotStart("`ollama` was not found.".into()));
        assert_eq!(ollama.title, "Ollama did not start");
        assert_eq!(
            ollama.description,
            "`ollama` was not found. The text is shown as recognized."
        );

        let missing = notice_in_review(Failure::ModelUnavailable("qwen is missing".into()));
        assert_eq!(missing.title, "Post-processing Unavailable");
        assert_eq!(missing.description, "qwen is missing");

        let both = notice_in_review(Failure::BothConfigured);
        assert_eq!(both.title, "Post-processing Misconfigured");
        assert!(both.description.contains("post_processing.model"));
        assert!(both.description.contains("post_processing.agent"));
    }

    #[test]
    fn a_failure_is_explained_only_in_review() {
        for phase in [
            FooterPhase::Starting,
            FooterPhase::Recording,
            FooterPhase::Recognizing,
            FooterPhase::PostProcessing,
            FooterPhase::Failed,
        ] {
            let mut input = input(phase);
            input.post_processing_failure = Some(Failure::NoText);
            assert_eq!(footer_state(&input).notice, None, "{phase:?}");
        }
    }
}
