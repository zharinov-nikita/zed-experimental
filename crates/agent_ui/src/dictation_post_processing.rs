//! Local: the decisions behind Post-processing of a dictated transcript:
//! which rewriter the settings ask for, what it is sent, what its session is
//! allowed to run with and what its answer amounts to. The Dictation Window
//! owns the state and the view and calls in here; nothing in this module
//! reaches a model, an agent, a process or the network on its own.
//! Everything external is handed in, the way the Ollama launcher in
//! `dictation_model_server` is given «is the server answering» and «start
//! it», so the decisions can be tested without any of it. The live
//! implementations sit next to the pure core: the language model path over
//! the registry, the External Agent path over the connections Zed already
//! holds (see `CONTEXT.md`, Post-processing Session).

use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use acp_thread::{AcpThread, AgentConnection as _, AuthRequired};
use agent_client_protocol::schema::v1 as acp;
use agent_servers::AcpConnection;
use agent_settings::{AgentSettings, DictationPostProcessingAgent, DictationSettings};
use anyhow::{Context as _, Result, anyhow};
use collections::{HashMap, HashSet};
use futures::future::{Either, LocalBoxFuture, select};
use futures::{FutureExt as _, StreamExt as _};
use gpui::{App, AsyncApp, Entity, Global, ReadGlobal as _, Task, WeakEntity};
use language_model::{
    CompletionIntent, LanguageModel, LanguageModelId, LanguageModelProviderId,
    LanguageModelRegistry, LanguageModelRequest, LanguageModelRequestMessage, Role, SelectedModel,
};
use project::{AgentId, Project};
use settings::{
    AgentConfigOptionValue, DictationAgentOptionContent, DictationAgentOptionKindContent,
    DictationAgentOptionValueContent, LanguageModelSelection, SettingsStore,
};
use util::ResultExt as _;
use util::path_list::PathList;
use workspace::Workspace;

use crate::agent_connection_store::AgentConnectionStore;
use crate::dictation_footer::{ProcessedBy, RewriterKind};
use crate::dictation_model_server::ServerOutcome;
use crate::{Agent, AgentPanel};

/// How long a Post-processing Session may take to answer; after that the
/// text stays raw. Lives next to the Ollama start-up limit.
pub const RESPONSE_LIMIT: Duration = Duration::from_secs(60);

/// Which rewriter the resolved settings select.
#[derive(Clone, Debug, PartialEq)]
pub enum Backend {
    /// The model named in `agent.dictation.post_processing.model`. When it
    /// is missing, Post-processing fails rather than switching to another
    /// model.
    LanguageModel(LanguageModelSelection),
    /// Neither a model nor an agent is configured: the agent's default
    /// language model is used.
    DefaultLanguageModel,
    /// The External Agent named in `agent.dictation.post_processing.agent`,
    /// reached through a Post-processing Session. Missing is an error here
    /// too, never a switch to another rewriter.
    ExternalAgent(DictationPostProcessingAgent),
}

/// A model and an agent configured at once is an error the user sees, not
/// a silent preference for one of them.
pub fn select_backend(settings: &DictationSettings) -> Result<Backend, Failure> {
    match (
        &settings.post_processing_model,
        &settings.post_processing_agent,
    ) {
        (Some(_), Some(_)) => Err(Failure::BothConfigured),
        (Some(selection), None) => Ok(Backend::LanguageModel(selection.clone())),
        (None, Some(agent)) => Ok(Backend::ExternalAgent(agent.clone())),
        (None, None) => Ok(Backend::DefaultLanguageModel),
    }
}

/// The text the rewriter is sent: the prompt template with `${output}` and
/// `${glossary}` filled in. The same for every rewriter.
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
    /// The request to the rewriter failed.
    RequestFailed(String),
    /// Both a language model and an External Agent are configured.
    BothConfigured,
    /// The agent could not be reached: not configured, not installed, not
    /// starting or, as far as Zed can tell, not signed in.
    AgentUnavailable { agent: String, reason: String },
    /// The agent asked to touch the machine or the user instead of
    /// answering with text; the request was refused (see `agent_servers`).
    AgentTriedToAct { agent: String },
    /// The agent declined to rewrite.
    AgentRefused { agent: String },
    /// No answer within [`RESPONSE_LIMIT`].
    TimedOut { agent: String },
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

fn processed_by_model(model: &Arc<dyn LanguageModel>) -> ProcessedBy {
    ProcessedBy {
        provider: model.provider_name().0.to_string(),
        model: model.name().0.to_string(),
        kind: RewriterKind::LanguageModel,
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
                    .map(|configured| (processed_by_model(&configured.model), configured.model))
                    .ok_or_else(|| anyhow!("No language model is configured for post-processing.")),
            );
        }
        Backend::LanguageModel(selection) => selection,
        Backend::ExternalAgent(agent) => {
            return Task::ready(Err(anyhow!(
                "{} is an External Agent, not a language model.",
                agent.id
            )));
        }
    };
    if let Some(model) = select_configured_model(&selection, cx) {
        return Task::ready(Ok((processed_by_model(&model), model)));
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
            .map(|model| (processed_by_model(&model), model))
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

// ---------------------------------------------------------------------------
// The External Agent path: a Post-processing Session on a connection Zed
// already holds.
// ---------------------------------------------------------------------------

/// What the agent announced when the Post-processing Session was created:
/// the modes it can run in and the session config options it accepts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Announcement {
    pub modes: Vec<acp::SessionMode>,
    pub config_options: Vec<acp::SessionConfigOption>,
}

/// How the Post-processing Session is set up: which options to set, which
/// mode to choose and which working directories to give it (none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionPolicy {
    /// Only options the agent announced, with values it announced; nothing
    /// from the settings stored for the user's own threads.
    pub options: Vec<(acp::SessionConfigId, acp::SessionConfigOptionValue)>,
    /// The strictest of the announced modes, when the agent has modes.
    pub mode: Option<acp::SessionModeId>,
    pub work_dirs: PathList,
}

fn is_mode_option(option: &acp::SessionConfigOption) -> bool {
    matches!(
        option.category,
        Some(acp::SessionConfigOptionCategory::Mode)
    )
}

fn is_model_option(option: &acp::SessionConfigOption) -> bool {
    matches!(
        option.category,
        Some(acp::SessionConfigOptionCategory::Model)
    ) || option.id.0.as_ref() == "model"
}

/// The values of a select option, grouped or not.
fn select_values(select: &acp::SessionConfigSelect) -> Vec<&acp::SessionConfigSelectOption> {
    match &select.options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect(),
        acp::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .collect(),
        _ => Vec::new(),
    }
}

/// How permissive a mode is, by what it calls itself; lower is stricter.
/// No announced mode means «no tools at all», so the ranking only decides
/// which one asks the most; the real guard is the refusal by session.
fn permissiveness(id: &str, name: &str) -> u8 {
    let text = format!("{id} {name}").to_lowercase();
    if text.contains("bypass") || text.contains("yolo") || text.contains("dangerous") {
        4
    } else if text.contains("auto") {
        3
    } else if text.contains("accept") || text.contains("edit") {
        2
    } else if text.contains("plan") || text.contains("read") {
        0
    } else if text.contains("default")
        || text.contains("manual")
        || text.contains("ask")
        || text.contains("normal")
    {
        1
    } else {
        2
    }
}

/// The strictest of `modes` as `(id, name)` pairs; the first one wins a tie.
fn strictest<'a>(modes: impl IntoIterator<Item = (&'a str, &'a str)>) -> Option<&'a str> {
    modes
        .into_iter()
        .map(|(id, name)| (permissiveness(id, name), id))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, id)| id)
}

/// Decides the session set-up from the options the user asked for and what
/// the agent announced. Options the agent did not announce, and values it
/// did not list, are not set at all; the mode is always the strictest one
/// announced, whether as a mode or as a mode option, whatever the user asked.
pub fn session_policy(
    requested: &HashMap<String, AgentConfigOptionValue>,
    announced: &Announcement,
) -> SessionPolicy {
    let mut options = Vec::new();
    for option in &announced.config_options {
        if is_mode_option(option) {
            if let acp::SessionConfigKind::Select(select) = &option.kind
                && let Some(mode) = strictest(
                    select_values(select)
                        .into_iter()
                        .map(|value| (value.value.0.as_ref(), value.name.as_str())),
                )
            {
                options.push((
                    option.id.clone(),
                    acp::SessionConfigOptionValue::value_id(mode.to_string()),
                ));
            }
            continue;
        }
        let Some(requested_value) = requested.get(option.id.0.as_ref()) else {
            continue;
        };
        let value = match (&option.kind, requested_value) {
            (acp::SessionConfigKind::Select(select), AgentConfigOptionValue::ValueId(id)) => {
                select_values(select)
                    .into_iter()
                    .any(|value| value.value.0.as_ref() == id.as_str())
                    .then(|| acp::SessionConfigOptionValue::value_id(id.clone()))
            }
            (acp::SessionConfigKind::Boolean(_), AgentConfigOptionValue::Boolean(value)) => {
                Some(acp::SessionConfigOptionValue::boolean(*value))
            }
            _ => None,
        };
        if let Some(value) = value {
            options.push((option.id.clone(), value));
        }
    }
    let mode = strictest(
        announced
            .modes
            .iter()
            .map(|mode| (mode.id.0.as_ref(), mode.name.as_str())),
    )
    .map(|id| acp::SessionModeId::new(id.to_string()));
    SessionPolicy {
        options,
        mode,
        work_dirs: PathList::default(),
    }
}

/// The model the label names: the human name of the model value the policy
/// sets, or the agent's own model when none is set.
pub fn model_label(policy: &SessionPolicy, announced: &Announcement) -> String {
    const OWN_MODEL: &str = "default model";
    let Some(option) = announced
        .config_options
        .iter()
        .find(|option| is_model_option(option))
    else {
        return OWN_MODEL.to_string();
    };
    let Some((_, acp::SessionConfigOptionValue::ValueId { value })) =
        policy.options.iter().find(|(id, _)| id == &option.id)
    else {
        return OWN_MODEL.to_string();
    };
    match &option.kind {
        acp::SessionConfigKind::Select(select) => select_values(select)
            .into_iter()
            .find(|candidate| candidate.value == *value)
            .map(|candidate| candidate.name.clone())
            .unwrap_or_else(|| value.0.to_string()),
        _ => value.0.to_string(),
    }
}

/// The announcement as the settings cache stores it, so the settings page
/// can list the agent's models without contacting it.
pub fn announcement_cache_entry(announced: &Announcement) -> Vec<DictationAgentOptionContent> {
    announced
        .config_options
        .iter()
        .filter_map(|option| {
            let kind = match &option.kind {
                acp::SessionConfigKind::Select(select) => DictationAgentOptionKindContent::Select {
                    current: select.current_value.0.to_string(),
                    values: select_values(select)
                        .into_iter()
                        .map(|value| DictationAgentOptionValueContent {
                            id: value.value.0.to_string(),
                            name: value.name.clone(),
                        })
                        .collect(),
                },
                acp::SessionConfigKind::Boolean(boolean) => {
                    DictationAgentOptionKindContent::Boolean {
                        current: boolean.current_value,
                    }
                }
                _ => return None,
            };
            Some(DictationAgentOptionContent {
                id: option.id.0.to_string(),
                name: option.name.clone(),
                category: option.category.as_ref().map(|category| match category {
                    acp::SessionConfigOptionCategory::Mode => "mode".to_string(),
                    acp::SessionConfigOptionCategory::Model => "model".to_string(),
                    acp::SessionConfigOptionCategory::ModelConfig => "model_config".to_string(),
                    acp::SessionConfigOptionCategory::ThoughtLevel => "thought_level".to_string(),
                    acp::SessionConfigOptionCategory::Other(other) => other.clone(),
                    _ => "other".to_string(),
                }),
                kind,
            })
        })
        .collect()
}

/// What a turn of the Post-processing Session came back with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observation {
    /// The agent's answer, text only.
    pub reply: String,
    /// How many of its requests to act were refused (see `agent_servers`).
    pub refused_actions: u32,
    pub stop_reason: Option<acp::StopReason>,
}

/// Why the prompt itself failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptError {
    pub message: String,
    /// The agent asked to be authenticated first.
    pub auth_required: bool,
}

/// What a turn amounts to. A refused action outranks whatever text came
/// with it: a rewrite that tried to act has failed, and the user keeps the
/// raw text.
pub fn classify_agent_outcome(
    agent: &str,
    observed: Result<Observation, PromptError>,
) -> Result<String, Failure> {
    let observed = match observed {
        Ok(observed) => observed,
        Err(error) if error.auth_required => {
            return Err(Failure::AgentUnavailable {
                agent: agent.to_string(),
                reason: error.message,
            });
        }
        Err(error) => return Err(Failure::RequestFailed(error.message)),
    };
    if observed.refused_actions > 0 {
        return Err(Failure::AgentTriedToAct {
            agent: agent.to_string(),
        });
    }
    match observed.stop_reason {
        Some(acp::StopReason::Refusal) => Err(Failure::AgentRefused {
            agent: agent.to_string(),
        }),
        Some(acp::StopReason::Cancelled) => Err(Failure::RequestFailed(format!(
            "{agent} cancelled the rewrite."
        ))),
        _ => classify_outcome(Ok(observed.reply)),
    }
}

/// What to do with a Post-processing Session when its Dictation Session is
/// over, given whether the agent can delete sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgetPlan {
    /// Delete it at the agent, so it never shows up in any list.
    Delete,
    /// Mark it hidden, so Zed's lists leave it out.
    Hide,
}

pub fn forget_plan(agent_supports_delete: bool) -> ForgetPlan {
    if agent_supports_delete {
        ForgetPlan::Delete
    } else {
        ForgetPlan::Hide
    }
}

/// A connection to an agent, as the rewriter needs it: the handle and the
/// name the user knows the agent by.
pub struct Connected<C> {
    pub connection: C,
    pub display_name: String,
}

/// Everything the rewriter needs from the outside world. The live
/// implementation is [`ZedAgents`]; tests use a fake.
pub trait RewriteAgent {
    type Connection;
    type Session;

    /// The connection Zed already holds for `agent_id`, connecting if it does
    /// not; `Err` when the agent is not configured, not installed, not
    /// starting or refuses the connection.
    fn connect(
        &self,
        agent_id: &str,
    ) -> LocalBoxFuture<'static, Result<Connected<Self::Connection>>>;

    /// Creates a text-only session on `connection` and reports what the
    /// agent announced for it.
    fn new_session(
        &self,
        connection: &Self::Connection,
    ) -> LocalBoxFuture<'static, Result<(Self::Session, Announcement)>>;

    fn apply_policy(
        &self,
        session: &Self::Session,
        policy: &SessionPolicy,
    ) -> LocalBoxFuture<'static, Result<()>>;

    /// Sends the text and waits for the whole answer.
    fn prompt(
        &self,
        session: &Self::Session,
        prompt: String,
    ) -> LocalBoxFuture<'static, Result<Observation, PromptError>>;

    /// Stops an answer that is taking too long.
    fn cancel(&self, session: &Self::Session);

    fn wait(&self, duration: Duration) -> LocalBoxFuture<'static, ()>;

    /// The session is no longer needed: deleted at the agent or hidden.
    fn forget(&self, session: &Self::Session);

    /// Keeps the announcement where the settings page can read it.
    fn remember_announcement(&self, agent_id: &str, announcement: &Announcement);
}

struct OpenSession<S> {
    session: S,
    processed_by: ProcessedBy,
}

struct RewriterState<A: RewriteAgent> {
    connection: Option<Rc<Connected<A::Connection>>>,
    session: Option<Rc<OpenSession<A::Session>>>,
}

/// The Post-processing Session of one Dictation Session: created on the
/// first rewrite, reused by every Resume, forgotten when dropped. One per
/// Dictation Window.
pub struct AgentRewriter<A: RewriteAgent> {
    agent: A,
    config: DictationPostProcessingAgent,
    state: RefCell<RewriterState<A>>,
}

impl<A: RewriteAgent> AgentRewriter<A> {
    pub fn new(agent: A, config: DictationPostProcessingAgent) -> Self {
        Self {
            agent,
            config,
            state: RefCell::new(RewriterState {
                connection: None,
                session: None,
            }),
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.config.id
    }

    pub fn agent(&self) -> &A {
        &self.agent
    }

    async fn connection(&self) -> Result<Rc<Connected<A::Connection>>> {
        if let Some(connection) = self.state.borrow().connection.clone() {
            return Ok(connection);
        }
        let connected = Rc::new(self.agent.connect(&self.config.id).await?);
        self.state.borrow_mut().connection = Some(connected.clone());
        Ok(connected)
    }

    async fn session(&self) -> Result<Rc<OpenSession<A::Session>>> {
        if let Some(session) = self.state.borrow().session.clone() {
            return Ok(session);
        }
        let connected = self.connection().await?;
        let (session, announcement) = self.agent.new_session(&connected.connection).await?;
        self.agent
            .remember_announcement(&self.config.id, &announcement);
        let policy = session_policy(&self.config.options, &announcement);
        let processed_by = ProcessedBy {
            provider: connected.display_name.clone(),
            model: model_label(&policy, &announcement),
            kind: RewriterKind::ExternalAgent,
        };
        if let Err(error) = self.agent.apply_policy(&session, &policy).await {
            self.agent.forget(&session);
            return Err(error);
        }
        let open = Rc::new(OpenSession {
            session,
            processed_by,
        });
        self.state.borrow_mut().session = Some(open.clone());
        Ok(open)
    }

    /// The name the failure messages use: the agent as the user knows it
    /// once connected, its id before that.
    fn agent_name(&self) -> String {
        self.state
            .borrow()
            .connection
            .as_ref()
            .map(|connected| connected.display_name.clone())
            .unwrap_or_else(|| self.config.id.clone())
    }

    /// Rewrites `prompt` in the Post-processing Session, creating it on the
    /// first call. Waits [`RESPONSE_LIMIT`] at most.
    pub async fn rewrite(&self, prompt: String) -> (Option<ProcessedBy>, Result<String, Failure>) {
        let open = match self.session().await {
            Ok(open) => open,
            Err(error) => {
                return (
                    None,
                    Err(Failure::AgentUnavailable {
                        agent: self.agent_name(),
                        reason: format!("{error:#}"),
                    }),
                );
            }
        };
        let agent = self.agent_name();
        let answer = self.agent.prompt(&open.session, prompt);
        let limit = self.agent.wait(RESPONSE_LIMIT);
        let result = match select(answer, limit).await {
            Either::Left((observed, _)) => classify_agent_outcome(&agent, observed),
            Either::Right(_) => {
                self.agent.cancel(&open.session);
                Err(Failure::TimedOut { agent })
            }
        };
        (Some(open.processed_by.clone()), result)
    }
}

impl<A: RewriteAgent> Drop for AgentRewriter<A> {
    fn drop(&mut self) {
        if let Some(open) = self.state.borrow_mut().session.take() {
            self.agent.forget(&open.session);
        }
    }
}

/// Post-processing Sessions of agents that cannot delete sessions; Zed's
/// lists of agent sessions leave them out.
#[derive(Default)]
pub struct HiddenSessions(HashSet<acp::SessionId>);

impl Global for HiddenSessions {}

pub fn hide_session(session_id: acp::SessionId, cx: &mut App) {
    cx.default_global::<HiddenSessions>().0.insert(session_id);
}

pub fn is_hidden_session(session_id: &acp::SessionId, cx: &App) -> bool {
    cx.try_global::<HiddenSessions>()
        .is_some_and(|hidden| hidden.0.contains(session_id))
}

/// The directory a Post-processing Session is told it works in: empty, so
/// that nothing of the user's is in reach even before the refusals.
fn post_processing_cwd() -> PathBuf {
    dictation::data_dir().join("post-processing")
}

/// The live [`RewriteAgent`]: connections come from the agent panel's
/// store, so a rewrite reuses the process the user is already talking to
/// and never starts a second one.
pub struct ZedAgents {
    store: Entity<AgentConnectionStore>,
    project: Entity<Project>,
    cx: AsyncApp,
}

pub struct LiveSession {
    connection: Rc<AcpConnection>,
    /// Keeps the session registered with the connection; dropping it closes
    /// the session for agents that support closing.
    _thread: Entity<AcpThread>,
    session_id: acp::SessionId,
}

impl ZedAgents {
    /// `None` when the workspace has no agent panel, which cannot happen
    /// while a Dictation Window is open in it.
    pub fn new(workspace: &WeakEntity<Workspace>, cx: &mut App) -> Option<Self> {
        let workspace = workspace.upgrade()?;
        let workspace = workspace.read(cx);
        let store = workspace
            .panel::<AgentPanel>(cx)?
            .read(cx)
            .connection_store()
            .clone();
        Some(Self {
            store,
            project: workspace.project().clone(),
            cx: cx.to_async(),
        })
    }

    /// The name the user knows `agent_id` by, for the footer while the
    /// answer is awaited.
    pub fn display_name(&self, agent_id: &str, cx: &App) -> String {
        self.project
            .read(cx)
            .agent_server_store()
            .read(cx)
            .agent_display_name(&AgentId::new(agent_id.to_string()))
            .map(|name| name.to_string())
            .unwrap_or_else(|| agent_id.to_string())
    }
}

impl RewriteAgent for ZedAgents {
    type Connection = Rc<AcpConnection>;
    type Session = LiveSession;

    fn connect(
        &self,
        agent_id: &str,
    ) -> LocalBoxFuture<'static, Result<Connected<Self::Connection>>> {
        let cx = self.cx.clone();
        let store = self.store.clone();
        let project = self.project.clone();
        let agent_id = agent_id.to_string();
        async move {
            let id = AgentId::new(agent_id.clone());
            let (configured, display_name) = cx.update(|cx| {
                let agents = project.read(cx).agent_server_store().read(cx);
                (
                    agents.external_agents().any(|known| known == &id),
                    agents.agent_display_name(&id),
                )
            });
            if !configured {
                anyhow::bail!("{agent_id} is not configured under `agent_servers`.");
            }
            let wait_for_connection = cx.update(|cx| {
                let agent = Agent::Custom { id };
                let server = agent.server(<dyn fs::Fs>::global(cx), agent::ThreadStore::global(cx));
                store.update(cx, |store, cx| {
                    store
                        .request_connection(agent, server, cx)
                        .read(cx)
                        .wait_for_connection()
                })
            });
            let connected = wait_for_connection
                .await
                .map_err(|error| anyhow!("{error}"))?;
            let connection = connected
                .connection
                .downcast::<AcpConnection>()
                .ok_or_else(|| anyhow!("{agent_id} is not an ACP agent."))?;
            Ok(Connected {
                connection,
                display_name: display_name
                    .map(|name| name.to_string())
                    .unwrap_or(agent_id),
            })
        }
        .boxed_local()
    }

    fn new_session(
        &self,
        connection: &Self::Connection,
    ) -> LocalBoxFuture<'static, Result<(Self::Session, Announcement)>> {
        let cx = self.cx.clone();
        let project = self.project.clone();
        let connection = connection.clone();
        async move {
            let cwd = post_processing_cwd();
            cx.background_executor()
                .spawn({
                    let cwd = cwd.clone();
                    async move { std::fs::create_dir_all(&cwd) }
                })
                .await?;
            let thread = cx
                .update(|cx| connection.clone().new_text_only_session(project, cwd, cx))
                .await?;
            let session_id = cx.update(|cx| thread.read(cx).session_id().clone());
            let announcement = cx.update(|cx| Announcement {
                modes: connection
                    .session_modes(&session_id, cx)
                    .map(|modes| modes.all_modes())
                    .unwrap_or_default(),
                config_options: connection
                    .session_config_options(&session_id, cx)
                    .map(|options| options.config_options())
                    .unwrap_or_default(),
            });
            Ok((
                LiveSession {
                    connection,
                    _thread: thread,
                    session_id,
                },
                announcement,
            ))
        }
        .boxed_local()
    }

    fn apply_policy(
        &self,
        session: &Self::Session,
        policy: &SessionPolicy,
    ) -> LocalBoxFuture<'static, Result<()>> {
        let cx = self.cx.clone();
        let connection = session.connection.clone();
        let session_id = session.session_id.clone();
        let policy = policy.clone();
        async move {
            for (option_id, value) in policy.options {
                let set = cx.update(|cx| {
                    connection
                        .session_config_options(&session_id, cx)
                        .map(|options| options.set_config_option(option_id.clone(), value, cx))
                });
                if let Some(set) = set {
                    set.await
                        .map_err(|error| anyhow!("setting option {}: {error:#}", option_id.0))?;
                }
            }
            if let Some(mode) = policy.mode {
                let set = cx.update(|cx| {
                    connection
                        .session_modes(&session_id, cx)
                        .map(|modes| modes.set_mode(mode.clone(), cx))
                });
                if let Some(set) = set {
                    set.await
                        .map_err(|error| anyhow!("setting mode {}: {error:#}", mode.0))?;
                }
            }
            Ok(())
        }
        .boxed_local()
    }

    fn prompt(
        &self,
        session: &Self::Session,
        prompt: String,
    ) -> LocalBoxFuture<'static, Result<Observation, PromptError>> {
        let cx = self.cx.clone();
        let connection = session.connection.clone();
        let session_id = session.session_id.clone();
        async move {
            connection.take_text_only_turn(&session_id);
            let response = cx
                .update(|cx| {
                    connection.prompt(
                        acp::PromptRequest::new(session_id.clone(), vec![prompt.into()]),
                        cx,
                    )
                })
                .await;
            let turn = connection
                .take_text_only_turn(&session_id)
                .unwrap_or_default();
            match response {
                Ok(response) => Ok(Observation {
                    reply: turn.reply,
                    refused_actions: turn.refused_actions,
                    stop_reason: Some(response.stop_reason),
                }),
                Err(error) => Err(PromptError {
                    auth_required: error.downcast_ref::<AuthRequired>().is_some(),
                    message: format!("{error:#}"),
                }),
            }
        }
        .boxed_local()
    }

    fn cancel(&self, session: &Self::Session) {
        let connection = session.connection.clone();
        let session_id = session.session_id.clone();
        self.cx
            .spawn(async move |cx| {
                cx.update(|cx| connection.cancel(&session_id, cx));
            })
            .detach();
    }

    fn wait(&self, duration: Duration) -> LocalBoxFuture<'static, ()> {
        self.cx.background_executor().timer(duration).boxed_local()
    }

    /// Runs in a task of its own: the rewriter may be dropped in the middle
    /// of an update, when the app must not be borrowed again.
    fn forget(&self, session: &Self::Session) {
        let connection = session.connection.clone();
        let session_id = session.session_id.clone();
        self.cx
            .spawn(async move |cx| {
                let delete = cx.update(|cx| {
                    let list = connection.session_list(cx);
                    match forget_plan(list.as_ref().is_some_and(|list| list.supports_delete())) {
                        ForgetPlan::Delete => list.map(|list| list.delete_session(&session_id, cx)),
                        ForgetPlan::Hide => {
                            hide_session(session_id.clone(), cx);
                            None
                        }
                    }
                });
                if let Some(delete) = delete {
                    delete
                        .await
                        .with_context(|| format!("deleting Post-processing Session {session_id}"))
                        .log_err();
                }
            })
            .detach();
    }

    fn remember_announcement(&self, agent_id: &str, announcement: &Announcement) {
        let entry = announcement_cache_entry(announcement);
        let agent_id = agent_id.to_string();
        self.cx
            .spawn(async move |cx| {
                cx.update(|cx| {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |content, _| {
                            content
                                .agent
                                .get_or_insert_default()
                                .dictation
                                .get_or_insert_default()
                                .post_processing
                                .get_or_insert_default()
                                .agent_options_cache
                                .get_or_insert_default()
                                .insert(agent_id, entry);
                        },
                    );
                });
            })
            .detach();
    }
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
            post_processing_agent: None,
            post_processing_agent_options_cache: HashMap::default(),
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

    fn claude(options: &[(&str, AgentConfigOptionValue)]) -> DictationPostProcessingAgent {
        DictationPostProcessingAgent {
            id: "claude-acp".into(),
            options: options
                .iter()
                .map(|(id, value)| (id.to_string(), value.clone()))
                .collect(),
        }
    }

    fn qwen() -> ProcessedBy {
        ProcessedBy {
            provider: "Ollama".into(),
            model: "qwen3:14b".into(),
            kind: RewriterKind::LanguageModel,
        }
    }

    fn value(id: &str) -> AgentConfigOptionValue {
        AgentConfigOptionValue::ValueId(id.into())
    }

    fn select_option(
        id: &str,
        category: Option<acp::SessionConfigOptionCategory>,
        current: &str,
        values: &[(&str, &str)],
    ) -> acp::SessionConfigOption {
        let mut option = acp::SessionConfigOption::new(
            id.to_string(),
            id.to_string(),
            acp::SessionConfigKind::Select(acp::SessionConfigSelect::new(
                current.to_string(),
                values
                    .iter()
                    .map(|(value, name)| {
                        acp::SessionConfigSelectOption::new(value.to_string(), name.to_string())
                    })
                    .collect::<Vec<_>>(),
            )),
        );
        option.category = category;
        option
    }

    fn boolean_option(id: &str, current: bool) -> acp::SessionConfigOption {
        acp::SessionConfigOption::new(
            id.to_string(),
            id.to_string(),
            acp::SessionConfigKind::Boolean(acp::SessionConfigBoolean::new(current)),
        )
    }

    /// What the installed agent announces, as recorded in the iteration spec.
    fn recorded_announcement() -> Announcement {
        Announcement {
            modes: vec![
                acp::SessionMode::new("default", "Default (always ask)"),
                acp::SessionMode::new("acceptEdits", "Accept Edits"),
                acp::SessionMode::new("plan", "Plan"),
                acp::SessionMode::new("auto", "Auto"),
                acp::SessionMode::new("bypassPermissions", "Bypass Permissions"),
            ],
            config_options: vec![
                select_option(
                    "model",
                    Some(acp::SessionConfigOptionCategory::Model),
                    "default",
                    &[
                        ("default", "Default (recommended)"),
                        ("opus", "Opus"),
                        ("sonnet", "Sonnet"),
                        ("haiku", "Haiku"),
                    ],
                ),
                boolean_option("fast", false),
            ],
        }
    }

    #[test]
    fn a_configured_model_is_selected_and_nothing_means_the_default_model() {
        let mut settings = settings();
        assert_eq!(select_backend(&settings), Ok(Backend::DefaultLanguageModel));

        settings.post_processing_model = Some(selection("ollama", "qwen3:14b"));
        assert_eq!(
            select_backend(&settings),
            Ok(Backend::LanguageModel(selection("ollama", "qwen3:14b")))
        );
    }

    #[test]
    fn a_configured_agent_is_selected_and_both_at_once_is_an_error() {
        let mut settings = settings();
        settings.post_processing_agent = Some(claude(&[("model", value("haiku"))]));
        assert_eq!(
            select_backend(&settings),
            Ok(Backend::ExternalAgent(claude(&[("model", value("haiku"))])))
        );

        settings.post_processing_model = Some(selection("ollama", "qwen3:14b"));
        assert_eq!(select_backend(&settings), Err(Failure::BothConfigured));
    }

    #[test]
    fn the_prompt_fills_in_the_transcript_and_the_glossary() {
        let settings = settings();
        assert_eq!(
            prompt_text(
                &settings.post_processing_prompt,
                "hello",
                &settings.glossary
            ),
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
        let (processed_by, result) = futures::executor::block_on(rewrite_with_language_model(
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
        let (processed_by, result) = futures::executor::block_on(rewrite_with_language_model(
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
            Err(Failure::ModelUnavailable(
                "qwen3:14b is not available".into()
            ))
        );
        assert!(!completed.get(), "nothing must be sent to a missing model");
    }

    #[test]
    fn a_ready_server_and_a_resolved_model_yield_the_cleaned_answer_and_the_model() {
        let (processed_by, result) = futures::executor::block_on(rewrite_with_language_model(
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
        let (processed_by, result) = futures::executor::block_on(rewrite_with_language_model(
            None::<std::future::Ready<ServerOutcome>>,
            || async { Ok((qwen(), ())) },
            |()| async { Err(anyhow!("connection refused")) },
        ));
        assert_eq!(processed_by, Some(qwen()));
        assert_eq!(
            result,
            Err(Failure::RequestFailed("connection refused".into()))
        );

        let (processed_by, result) = futures::executor::block_on(rewrite_with_language_model(
            None::<std::future::Ready<ServerOutcome>>,
            || async { Ok((qwen(), ())) },
            |()| async { Ok(String::new()) },
        ));
        assert_eq!(processed_by, Some(qwen()));
        assert_eq!(result, Err(Failure::NoText));
    }

    // -- Session policy --------------------------------------------------

    fn set_option(policy: &SessionPolicy, id: &str) -> Option<acp::SessionConfigOptionValue> {
        policy
            .options
            .iter()
            .find(|(option_id, _)| option_id.0.as_ref() == id)
            .map(|(_, value)| value.clone())
    }

    #[test]
    fn only_announced_options_with_announced_values_are_set() {
        let requested = claude(&[
            ("model", value("haiku")),
            ("fast", AgentConfigOptionValue::Boolean(true)),
            ("effort", value("high")),
            ("model_unknown", value("x")),
        ])
        .options;
        let policy = session_policy(&requested, &recorded_announcement());
        assert_eq!(
            set_option(&policy, "model"),
            Some(acp::SessionConfigOptionValue::value_id("haiku"))
        );
        assert_eq!(
            set_option(&policy, "fast"),
            Some(acp::SessionConfigOptionValue::boolean(true))
        );
        assert_eq!(set_option(&policy, "effort"), None, "not announced");
        assert_eq!(set_option(&policy, "model_unknown"), None, "not announced");
        assert_eq!(policy.options.len(), 2);
        assert!(policy.work_dirs.is_empty());
    }

    #[test]
    fn a_value_the_agent_did_not_list_and_a_value_of_the_wrong_kind_are_not_set() {
        let requested = claude(&[("model", value("gpt-5")), ("fast", value("yes"))]).options;
        let policy = session_policy(&requested, &recorded_announcement());
        assert!(policy.options.is_empty(), "{:?}", policy.options);
    }

    #[test]
    fn nothing_requested_sets_nothing_so_the_agent_keeps_its_own_model() {
        let policy = session_policy(&HashMap::default(), &recorded_announcement());
        assert!(policy.options.is_empty());
        assert_eq!(
            model_label(&policy, &recorded_announcement()),
            "default model"
        );
    }

    #[test]
    fn the_strictest_announced_mode_is_chosen() {
        let policy = session_policy(&HashMap::default(), &recorded_announcement());
        assert_eq!(policy.mode, Some(acp::SessionModeId::new("plan")));

        let without_plan = Announcement {
            modes: vec![
                acp::SessionMode::new("bypassPermissions", "Bypass Permissions"),
                acp::SessionMode::new("default", "Manual (always ask)"),
                acp::SessionMode::new("auto", "Auto"),
            ],
            config_options: Vec::new(),
        };
        let policy = session_policy(&HashMap::default(), &without_plan);
        assert_eq!(policy.mode, Some(acp::SessionModeId::new("default")));

        let none = Announcement::default();
        assert_eq!(session_policy(&HashMap::default(), &none).mode, None);
    }

    #[test]
    fn a_mode_announced_as_a_config_option_is_set_to_the_strictest_value_whatever_was_asked() {
        let announced = Announcement {
            modes: Vec::new(),
            config_options: vec![select_option(
                "permission-mode",
                Some(acp::SessionConfigOptionCategory::Mode),
                "default",
                &[
                    ("default", "Default"),
                    ("acceptEdits", "Accept Edits"),
                    ("plan", "Plan"),
                    ("bypassPermissions", "Bypass"),
                ],
            )],
        };
        let requested = claude(&[("permission-mode", value("bypassPermissions"))]).options;
        let policy = session_policy(&requested, &announced);
        assert_eq!(
            set_option(&policy, "permission-mode"),
            Some(acp::SessionConfigOptionValue::value_id("plan"))
        );
        assert_eq!(policy.mode, None);
    }

    #[test]
    fn the_label_names_the_chosen_model_by_its_human_name() {
        let requested = claude(&[("model", value("haiku"))]).options;
        let announced = recorded_announcement();
        let policy = session_policy(&requested, &announced);
        assert_eq!(model_label(&policy, &announced), "Haiku");

        let requested = claude(&[("model", value("default"))]).options;
        let policy = session_policy(&requested, &announced);
        assert_eq!(model_label(&policy, &announced), "Default (recommended)");
    }

    #[test]
    fn the_cache_entry_records_every_announced_option_as_declared() {
        let entry = announcement_cache_entry(&recorded_announcement());
        assert_eq!(entry.len(), 2);
        assert_eq!(entry[0].id, "model");
        assert_eq!(entry[0].category.as_deref(), Some("model"));
        assert_eq!(
            entry[0].kind,
            DictationAgentOptionKindContent::Select {
                current: "default".into(),
                values: vec![
                    DictationAgentOptionValueContent {
                        id: "default".into(),
                        name: "Default (recommended)".into()
                    },
                    DictationAgentOptionValueContent {
                        id: "opus".into(),
                        name: "Opus".into()
                    },
                    DictationAgentOptionValueContent {
                        id: "sonnet".into(),
                        name: "Sonnet".into()
                    },
                    DictationAgentOptionValueContent {
                        id: "haiku".into(),
                        name: "Haiku".into()
                    },
                ],
            }
        );
        assert_eq!(
            entry[1].kind,
            DictationAgentOptionKindContent::Boolean { current: false }
        );
        assert_eq!(entry[1].category, None);
    }

    // -- Outcome ---------------------------------------------------------

    fn observed(reply: &str, refused_actions: u32, stop_reason: acp::StopReason) -> Observation {
        Observation {
            reply: reply.into(),
            refused_actions,
            stop_reason: Some(stop_reason),
        }
    }

    #[test]
    fn a_text_answer_is_the_cleaned_text() {
        assert_eq!(
            classify_agent_outcome(
                "Claude Code",
                Ok(observed(
                    "<think>x</think> Clean ",
                    0,
                    acp::StopReason::EndTurn
                ))
            ),
            Ok("Clean".into())
        );
        assert_eq!(
            classify_agent_outcome("Claude Code", Ok(observed("", 0, acp::StopReason::EndTurn))),
            Err(Failure::NoText)
        );
    }

    #[test]
    fn a_refused_action_outranks_the_text_that_came_with_it() {
        assert_eq!(
            classify_agent_outcome(
                "Claude Code",
                Ok(observed("Clean", 1, acp::StopReason::EndTurn))
            ),
            Err(Failure::AgentTriedToAct {
                agent: "Claude Code".into()
            })
        );
    }

    #[test]
    fn a_refusal_and_a_prompt_error_are_told_apart_from_an_unavailable_agent() {
        assert_eq!(
            classify_agent_outcome("Claude Code", Ok(observed("", 0, acp::StopReason::Refusal))),
            Err(Failure::AgentRefused {
                agent: "Claude Code".into()
            })
        );
        assert_eq!(
            classify_agent_outcome(
                "Claude Code",
                Err(PromptError {
                    message: "Authentication required".into(),
                    auth_required: true,
                })
            ),
            Err(Failure::AgentUnavailable {
                agent: "Claude Code".into(),
                reason: "Authentication required".into(),
            })
        );
        assert_eq!(
            classify_agent_outcome(
                "Claude Code",
                Err(PromptError {
                    message: "connection closed".into(),
                    auth_required: false,
                })
            ),
            Err(Failure::RequestFailed("connection closed".into()))
        );
    }

    #[test]
    fn an_agent_that_can_delete_gets_the_session_deleted_and_one_that_cannot_hides_it() {
        assert_eq!(forget_plan(true), ForgetPlan::Delete);
        assert_eq!(forget_plan(false), ForgetPlan::Hide);
    }

    // -- Lifecycle with a fake agent --------------------------------------

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Reply {
        Text,
        Empty,
        Act,
        Hang,
    }

    #[derive(Default)]
    struct Log {
        connects: usize,
        sessions_created: usize,
        policies: Vec<SessionPolicy>,
        prompts: Vec<(usize, String)>,
        cancels: usize,
        forgotten: Vec<usize>,
        remembered: Vec<String>,
        waited: Duration,
    }

    /// A fake agent: connecting may fail, every session gets a number and
    /// answers as told. The log is shared so the test can read it after the
    /// rewriter, and the agent with it, are dropped.
    struct FakeAgent {
        connectable: bool,
        announcement: Announcement,
        reply: Cell<Reply>,
        log: Rc<RefCell<Log>>,
    }

    impl FakeAgent {
        fn new(reply: Reply) -> Self {
            Self {
                connectable: true,
                announcement: recorded_announcement(),
                reply: Cell::new(reply),
                log: Rc::default(),
            }
        }
    }

    impl RewriteAgent for FakeAgent {
        type Connection = ();
        type Session = usize;

        fn connect(&self, agent_id: &str) -> LocalBoxFuture<'static, Result<Connected<()>>> {
            self.log.borrow_mut().connects += 1;
            let result = if self.connectable {
                Ok(Connected {
                    connection: (),
                    display_name: format!("{agent_id} (display)"),
                })
            } else {
                Err(anyhow!(
                    "{agent_id} is not configured under `agent_servers`."
                ))
            };
            async move { result }.boxed_local()
        }

        fn new_session(&self, _: &()) -> LocalBoxFuture<'static, Result<(usize, Announcement)>> {
            let mut log = self.log.borrow_mut();
            log.sessions_created += 1;
            let session = log.sessions_created;
            let announcement = self.announcement.clone();
            async move { Ok((session, announcement)) }.boxed_local()
        }

        fn apply_policy(
            &self,
            _: &usize,
            policy: &SessionPolicy,
        ) -> LocalBoxFuture<'static, Result<()>> {
            self.log.borrow_mut().policies.push(policy.clone());
            async { Ok(()) }.boxed_local()
        }

        fn prompt(
            &self,
            session: &usize,
            prompt: String,
        ) -> LocalBoxFuture<'static, Result<Observation, PromptError>> {
            self.log.borrow_mut().prompts.push((*session, prompt));
            let reply = self.reply.get();
            async move {
                match reply {
                    Reply::Text => Ok(observed("Clean text", 0, acp::StopReason::EndTurn)),
                    Reply::Empty => Ok(observed("", 0, acp::StopReason::EndTurn)),
                    Reply::Act => Ok(observed("", 2, acp::StopReason::EndTurn)),
                    Reply::Hang => std::future::pending().await,
                }
            }
            .boxed_local()
        }

        fn cancel(&self, _: &usize) {
            self.log.borrow_mut().cancels += 1;
        }

        fn wait(&self, duration: Duration) -> LocalBoxFuture<'static, ()> {
            // The fake clock jumps: waiting completes at once, as if the
            // limit had passed, so a hanging answer times out immediately.
            self.log.borrow_mut().waited += duration;
            async {}.boxed_local()
        }

        fn forget(&self, session: &usize) {
            self.log.borrow_mut().forgotten.push(*session);
        }

        fn remember_announcement(&self, agent_id: &str, _: &Announcement) {
            self.log.borrow_mut().remembered.push(agent_id.to_string());
        }
    }

    fn rewriter(agent: FakeAgent) -> (AgentRewriter<FakeAgent>, Rc<RefCell<Log>>) {
        let log = agent.log.clone();
        (
            AgentRewriter::new(agent, claude(&[("model", value("haiku"))])),
            log,
        )
    }

    fn processed_by_claude() -> ProcessedBy {
        ProcessedBy {
            provider: "claude-acp (display)".into(),
            model: "Haiku".into(),
            kind: RewriterKind::ExternalAgent,
        }
    }

    #[test]
    fn one_session_serves_the_first_rewrite_and_the_resume_and_is_forgotten_at_the_end() {
        let (rewriter, log) = rewriter(FakeAgent::new(Reply::Text));
        let first = futures::executor::block_on(rewriter.rewrite("first".into()));
        assert_eq!(
            first,
            (Some(processed_by_claude()), Ok("Clean text".into()))
        );
        let resume = futures::executor::block_on(rewriter.rewrite("resume".into()));
        assert_eq!(
            resume,
            (Some(processed_by_claude()), Ok("Clean text".into()))
        );
        {
            let log = log.borrow();
            assert_eq!(log.connects, 1, "the connection is asked for once");
            assert_eq!(log.sessions_created, 1, "one session per Dictation Session");
            assert_eq!(log.prompts, vec![(1, "first".into()), (1, "resume".into())]);
            assert_eq!(log.remembered, vec!["claude-acp".to_string()]);
            assert_eq!(log.policies.len(), 1);
            assert_eq!(
                set_option(&log.policies[0], "model"),
                Some(acp::SessionConfigOptionValue::value_id("haiku"))
            );
            assert_eq!(log.policies[0].mode, Some(acp::SessionModeId::new("plan")));
            assert!(log.forgotten.is_empty(), "the session lives until the end");
        }
        drop(rewriter);
        assert_eq!(log.borrow().forgotten, vec![1]);
    }

    #[test]
    fn a_failed_run_still_leaves_no_session_behind() {
        let agent = FakeAgent::new(Reply::Act);
        let log = agent.log.clone();
        let rewriter = AgentRewriter::new(agent, claude(&[]));
        let (processed_by, result) = futures::executor::block_on(rewriter.rewrite("x".into()));
        assert_eq!(
            processed_by.as_ref().map(|by| by.model.as_str()),
            Some("default model")
        );
        assert_eq!(
            result,
            Err(Failure::AgentTriedToAct {
                agent: "claude-acp (display)".into()
            })
        );
        drop(rewriter);
        assert_eq!(log.borrow().forgotten, vec![1]);
    }

    #[test]
    fn an_unconnectable_agent_is_reported_and_no_session_is_created() {
        let mut agent = FakeAgent::new(Reply::Text);
        agent.connectable = false;
        let (rewriter, log) = rewriter(agent);
        let (processed_by, result) = futures::executor::block_on(rewriter.rewrite("x".into()));
        assert_eq!(processed_by, None);
        assert_eq!(
            result,
            Err(Failure::AgentUnavailable {
                agent: "claude-acp".into(),
                reason: "claude-acp is not configured under `agent_servers`.".into(),
            })
        );
        drop(rewriter);
        let log = log.borrow();
        assert_eq!(log.sessions_created, 0);
        assert!(log.prompts.is_empty());
        assert!(log.forgotten.is_empty());
    }

    #[test]
    fn an_answer_that_takes_too_long_is_cancelled_and_reported_as_a_timeout() {
        let (rewriter, log) = rewriter(FakeAgent::new(Reply::Hang));
        let (processed_by, result) = futures::executor::block_on(rewriter.rewrite("x".into()));
        assert_eq!(processed_by, Some(processed_by_claude()));
        assert_eq!(
            result,
            Err(Failure::TimedOut {
                agent: "claude-acp (display)".into()
            })
        );
        let log = log.borrow();
        assert_eq!(log.cancels, 1);
        assert_eq!(log.waited, RESPONSE_LIMIT);
    }

    #[test]
    fn an_empty_answer_leaves_the_text_raw_and_the_session_open_for_the_resume() {
        let (rewriter, log) = rewriter(FakeAgent::new(Reply::Empty));
        let (_, result) = futures::executor::block_on(rewriter.rewrite("x".into()));
        assert_eq!(result, Err(Failure::NoText));
        rewriter.agent.reply.set(Reply::Text);
        let (_, result) = futures::executor::block_on(rewriter.rewrite("y".into()));
        assert_eq!(result, Ok("Clean text".into()));
        assert_eq!(log.borrow().sessions_created, 1);
    }
}
