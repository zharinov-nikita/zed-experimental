//! Local: the decisions behind Post-processing of a dictated transcript:
//! which rewriter the settings ask for, what it is sent and what its answer
//! amounts to. The Dictation Window owns the state and the view and calls
//! in here; nothing in this module reaches a model, a process or the network
//! on its own. Everything external is handed in, the way the Ollama launcher
//! in `dictation_model_server` is given «is the server answering» and «start
//! it», so the decisions can be tested without any of it.

use std::future::Future;
use std::sync::Arc;

use agent_settings::{AgentSettings, DictationSettings};
use anyhow::{Result, anyhow};
use futures::StreamExt as _;
use gpui::{App, AsyncApp, Task};
use language_model::{
    CompletionIntent, LanguageModel, LanguageModelId, LanguageModelProviderId,
    LanguageModelRegistry, LanguageModelRequest, LanguageModelRequestMessage, Role, SelectedModel,
};
use settings::LanguageModelSelection;

use crate::dictation_footer::ProcessedBy;
use crate::dictation_model_server::ServerOutcome;

/// Which rewriter the resolved settings select.
#[derive(Clone, Debug, PartialEq)]
pub enum Backend {
    /// The model named in `agent.dictation.post_processing.model`. When it
    /// is missing, Post-processing fails rather than switching to another
    /// model.
    LanguageModel(LanguageModelSelection),
    /// No model is configured: the agent's default language model is used.
    DefaultLanguageModel,
}

pub fn select_backend(settings: &DictationSettings) -> Backend {
    match &settings.post_processing_model {
        Some(selection) => Backend::LanguageModel(selection.clone()),
        None => Backend::DefaultLanguageModel,
    }
}

/// The text the rewriter is sent: the prompt template with `${output}` and
/// `${glossary}` filled in.
pub fn prompt_text(template: &str, new_part: &str, glossary: &[String]) -> String {
    template
        .replace("${output}", new_part)
        .replace("${glossary}", &glossary.join(", "))
}

/// Why a run left the text raw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The rewriter answered, but with nothing.
    NoText,
    /// Zed started Ollama for Post-processing and it did not come up.
    ServerDidNotStart(String),
    /// The configured model is missing, or no model is configured at all.
    ModelUnavailable(String),
    /// The request to the model failed.
    RequestFailed(String),
}

/// What a run amounts to, decided in one place: the answer with the model's
/// thinking stripped, or the failure. An empty answer is a failure too.
pub fn classify_outcome(response: Result<String, Failure>) -> Result<String, Failure> {
    let text = strip_thinking(&response?);
    if text.is_empty() {
        Err(Failure::NoText)
    } else {
        Ok(text)
    }
}

/// Qwen-style models may wrap reasoning in `<think>` tags; only the answer is wanted.
fn strip_thinking(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start) = result.find("<think>") {
        match result[start..].find("</think>") {
            Some(end) => result.replace_range(start..start + end + "</think>".len(), ""),
            None => result.truncate(start),
        }
    }
    result.trim().to_string()
}

/// Rewrites through a language model: waits for the server Zed may have
/// started (`server`), resolves the model (`resolve`) and sends the prompt
/// (`complete`). When the server never came up the model is not even looked
/// up. The rewriter is named whenever the model was resolved, so the label
/// can show the model actually used, including the fallback to the agent's
/// default model.
pub async fn rewrite_with_language_model<Server, Resolve, Resolved, Model, Complete, Completed>(
    server: Option<Server>,
    resolve: Resolve,
    complete: Complete,
) -> (Option<ProcessedBy>, Result<String, Failure>)
where
    Server: Future<Output = ServerOutcome>,
    Resolve: FnOnce() -> Resolved,
    Resolved: Future<Output = Result<(ProcessedBy, Model)>>,
    Complete: FnOnce(Model) -> Completed,
    Completed: Future<Output = Result<String>>,
{
    if let Some(server) = server
        && let ServerOutcome::Failed(reason) = server.await
    {
        return (None, Err(Failure::ServerDidNotStart(reason)));
    }
    let (processed_by, model) = match resolve().await {
        Ok(resolved) => resolved,
        Err(error) => return (None, Err(Failure::ModelUnavailable(format!("{error:#}")))),
    };
    let response = complete(model)
        .await
        .map_err(|error| Failure::RequestFailed(format!("{error:#}")));
    (Some(processed_by), classify_outcome(response))
}

fn select_configured_model(
    selection: &LanguageModelSelection,
    cx: &mut App,
) -> Option<Arc<dyn LanguageModel>> {
    let selected = SelectedModel {
        provider: LanguageModelProviderId(selection.provider.0.clone().into()),
        model: LanguageModelId(selection.model.clone().into()),
    };
    LanguageModelRegistry::global(cx)
        .update(cx, |registry, cx| registry.select_model(&selected, cx))
        .map(|configured| configured.model)
}

fn processed_by(model: &Arc<dyn LanguageModel>) -> ProcessedBy {
    ProcessedBy {
        provider: model.provider_name().0.to_string(),
        model: model.name().0.to_string(),
    }
}

/// Resolves the model for `backend` against the registry. Providers such
/// as Ollama list their models only after they have been asked to
/// authenticate, which nothing does in a fresh session, so the provider is
/// authenticated first and the lookup retried. A configured model that is
/// still missing is an error, never a silent switch to another model.
pub fn resolve_language_model(
    backend: Backend,
    cx: &mut App,
) -> Task<Result<(ProcessedBy, Arc<dyn LanguageModel>)>> {
    let selection = match backend {
        Backend::DefaultLanguageModel => {
            return Task::ready(
                LanguageModelRegistry::read_global(cx)
                    .default_model()
                    .map(|configured| (processed_by(&configured.model), configured.model))
                    .ok_or_else(|| anyhow!("No language model is configured for post-processing.")),
            );
        }
        Backend::LanguageModel(selection) => selection,
    };
    if let Some(model) = select_configured_model(&selection, cx) {
        return Task::ready(Ok((processed_by(&model), model)));
    }
    let provider = LanguageModelRegistry::read_global(cx).provider(&LanguageModelProviderId(
        selection.provider.0.clone().into(),
    ));
    let authenticate = provider.map(|provider| provider.authenticate(cx));
    cx.spawn(async move |cx| {
        if let Some(authenticate) = authenticate
            && let Err(error) = authenticate.await
        {
            log::warn!("dictation: post-processing provider is unavailable: {error}");
        }
        cx.update(|cx| select_configured_model(&selection, cx))
            .map(|model| (processed_by(&model), model))
            .ok_or_else(|| {
                anyhow!(
                    "The post-processing model {} ({}) is not available.",
                    selection.model,
                    selection.provider.0
                )
            })
    })
}

/// Sends `prompt` to `model` and collects the whole answer; nothing is
/// shown until it is complete.
pub async fn complete_with_language_model(
    model: Arc<dyn LanguageModel>,
    prompt: String,
    cx: AsyncApp,
) -> Result<String> {
    let temperature = cx.update(|cx| AgentSettings::temperature_for_model(&model, cx));
    let request = LanguageModelRequest {
        intent: Some(CompletionIntent::UserPrompt),
        messages: vec![LanguageModelRequestMessage {
            role: Role::User,
            content: vec![prompt.into()],
            cache: false,
            reasoning_details: None,
        }],
        temperature,
        thinking_allowed: false,
        ..Default::default()
    };
    let mut messages = model.stream_completion_text(request, &cx).await?;
    let mut text = String::new();
    while let Some(chunk) = messages.stream.next().await {
        text.push_str(&chunk?);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use settings::LanguageModelProviderSetting;
    use std::cell::Cell;

    fn settings() -> DictationSettings {
        DictationSettings {
            model_path: None,
            backends_dir: None,
            language: Default::default(),
            glossary: vec!["Zed".into(), "Rust".into()],
            sounds: false,
            keep_model_loaded: true,
            session_audio_keep: 20,
            post_processing_enabled: true,
            post_processing_model: None,
            post_processing_prompt: "Fix ${output} with ${glossary}".into(),
        }
    }

    fn selection(provider: &str, model: &str) -> LanguageModelSelection {
        LanguageModelSelection {
            provider: LanguageModelProviderSetting(provider.into()),
            model: model.into(),
            enable_thinking: false,
            effort: None,
            speed: None,
        }
    }

    fn qwen() -> ProcessedBy {
        ProcessedBy {
            provider: "Ollama".into(),
            model: "qwen3:14b".into(),
        }
    }

    #[test]
    fn a_configured_model_is_selected_and_nothing_means_the_default_model() {
        let mut settings = settings();
        assert_eq!(select_backend(&settings), Backend::DefaultLanguageModel);

        settings.post_processing_model = Some(selection("ollama", "qwen3:14b"));
        assert_eq!(
            select_backend(&settings),
            Backend::LanguageModel(selection("ollama", "qwen3:14b"))
        );
    }

    #[test]
    fn the_prompt_fills_in_the_transcript_and_the_glossary() {
        let settings = settings();
        assert_eq!(
            prompt_text(&settings.post_processing_prompt, "hello", &settings.glossary),
            "Fix hello with Zed, Rust"
        );
    }

    #[test]
    fn the_answer_is_the_text_without_the_models_thinking() {
        assert_eq!(
            classify_outcome(Ok("<think>hmm</think>\n Clean text ".into())),
            Ok("Clean text".into())
        );
        assert_eq!(
            classify_outcome(Ok("<think>never closed".into())),
            Err(Failure::NoText)
        );
        assert_eq!(classify_outcome(Ok("   ".into())), Err(Failure::NoText));
        assert_eq!(
            classify_outcome(Err(Failure::RequestFailed("boom".into()))),
            Err(Failure::RequestFailed("boom".into()))
        );
    }

    #[test]
    fn a_server_that_did_not_start_skips_the_model_entirely() {
        let resolved = Cell::new(false);
        let (processed_by, result) =
            futures::executor::block_on(rewrite_with_language_model(
                Some(async { ServerOutcome::Failed("no ollama".into()) }),
                || {
                    resolved.set(true);
                    async { Ok((qwen(), ())) }
                },
                |()| async { Ok("text".to_string()) },
            ));
        assert_eq!(processed_by, None);
        assert_eq!(result, Err(Failure::ServerDidNotStart("no ollama".into())));
        assert!(!resolved.get(), "the model must not be resolved");
    }

    #[test]
    fn a_missing_model_is_reported_without_a_request() {
        let completed = Cell::new(false);
        let (processed_by, result) =
            futures::executor::block_on(rewrite_with_language_model(
                None::<std::future::Ready<ServerOutcome>>,
                || async { Err::<(ProcessedBy, ()), _>(anyhow!("qwen3:14b is not available")) },
                |()| {
                    completed.set(true);
                    async { Ok("text".to_string()) }
                },
            ));
        assert_eq!(processed_by, None);
        assert_eq!(
            result,
            Err(Failure::ModelUnavailable("qwen3:14b is not available".into()))
        );
        assert!(!completed.get(), "nothing must be sent to a missing model");
    }

    #[test]
    fn a_ready_server_and_a_resolved_model_yield_the_cleaned_answer_and_the_model() {
        let (processed_by, result) =
            futures::executor::block_on(rewrite_with_language_model(
                Some(async { ServerOutcome::Ready }),
                || async { Ok((qwen(), "model")) },
                |model| async move {
                    assert_eq!(model, "model");
                    Ok("<think>x</think>Clean".to_string())
                },
            ));
        assert_eq!(processed_by, Some(qwen()));
        assert_eq!(result, Ok("Clean".into()));
    }

    #[test]
    fn a_failed_request_and_an_empty_answer_keep_the_model_name() {
        let (processed_by, result) =
            futures::executor::block_on(rewrite_with_language_model(
                None::<std::future::Ready<ServerOutcome>>,
                || async { Ok((qwen(), ())) },
                |()| async { Err(anyhow!("connection refused")) },
            ));
        assert_eq!(processed_by, Some(qwen()));
        assert_eq!(
            result,
            Err(Failure::RequestFailed("connection refused".into()))
        );

        let (processed_by, result) =
            futures::executor::block_on(rewrite_with_language_model(
                None::<std::future::Ready<ServerOutcome>>,
                || async { Ok((qwen(), ())) },
                |()| async { Ok(String::new()) },
            ));
        assert_eq!(processed_by, Some(qwen()));
        assert_eq!(result, Err(Failure::NoText));
    }
}
