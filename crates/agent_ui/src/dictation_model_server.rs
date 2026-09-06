//! Local: brings up the Ollama server for Post-processing when a Dictation
//! Session starts and the server is not answering, so that after a reboot the
//! text is still processed by the local model instead of falling back or
//! failing. Only Ollama at its default local address is ever started; a
//! remote server or another provider is left alone.
//!
//! The launcher itself knows nothing about HTTP or processes: it is given
//! «is the server answering» and «start the server» and decides when to call
//! them, so the tests never touch a real server.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use gpui::BackgroundExecutor;
use http_client::{AsyncBody, HttpClient};

/// How long Post-processing waits for a server Zed has started.
pub const START_LIMIT: Duration = Duration::from_secs(20);
/// How often the server is asked whether it answers yet.
pub const POLL: Duration = Duration::from_millis(500);
/// A reachability probe that takes longer than this counts as no answer.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

pub const OLLAMA_PROVIDER_ID: &str = "ollama";
/// The desktop application next to the `ollama` executable on Windows. It
/// puts the server in the tray the way a manual start does.
#[cfg(target_os = "windows")]
const DESKTOP_APP: &str = "ollama app.exe";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerOutcome {
    /// The server answers; Post-processing can resolve its model.
    Ready,
    /// The server could not be started or did not answer within
    /// [`START_LIMIT`]; the reason is shown to the user.
    Failed(String),
}

/// Whether Zed should bring up a server for this Post-processing selection:
/// only Ollama, and only at its default local address.
pub fn should_start_ollama(provider: Option<&str>, api_url: &str) -> bool {
    provider == Some(OLLAMA_PROVIDER_ID) && is_default_local_url(api_url)
}

fn is_default_local_url(api_url: &str) -> bool {
    let url = api_url.trim().trim_end_matches('/');
    url == ollama::OLLAMA_API_URL || url == "http://127.0.0.1:11434"
}

/// The Ollama address as the provider resolves it: an empty setting means
/// the default.
pub fn ollama_api_url(configured: &str) -> String {
    if configured.trim().is_empty() {
        ollama::OLLAMA_API_URL.to_string()
    } else {
        configured.to_string()
    }
}

/// Makes sure the server answers: when `reachable` says it does not, `start`
/// is called once and the server is polled every [`POLL`] (sleeping with
/// `wait`) until it answers or [`START_LIMIT`] has passed. Nothing is started
/// when the server answers at once.
pub async fn ensure_server<Reachable, Answer, Wait, Sleep>(
    reachable: Reachable,
    start: impl FnOnce() -> Result<()>,
    wait: Wait,
) -> ServerOutcome
where
    Reachable: Fn() -> Answer,
    Answer: Future<Output = bool>,
    Wait: Fn(Duration) -> Sleep,
    Sleep: Future<Output = ()>,
{
    if reachable().await {
        return ServerOutcome::Ready;
    }
    if let Err(error) = start() {
        return ServerOutcome::Failed(format!("{error:#}"));
    }
    let mut waited = Duration::ZERO;
    while waited < START_LIMIT {
        wait(POLL).await;
        waited += POLL;
        if reachable().await {
            return ServerOutcome::Ready;
        }
    }
    ServerOutcome::Failed(format!(
        "Ollama did not answer within {} seconds of being started.",
        START_LIMIT.as_secs()
    ))
}

/// Asks the server for its version, which Ollama answers without loading a
/// model. A refused connection, an error status or a slow answer all count
/// as «not answering».
pub async fn ollama_answers(
    http_client: Arc<dyn HttpClient>,
    api_url: String,
    executor: BackgroundExecutor,
) -> bool {
    let request = http_client.get(
        &format!("{}/api/version", api_url.trim_end_matches('/')),
        AsyncBody::default(),
        false,
    );
    let timeout = executor.timer(PROBE_TIMEOUT);
    match futures::future::select(request, timeout).await {
        futures::future::Either::Left((response, _)) => {
            response.is_ok_and(|response| response.status().is_success())
        }
        futures::future::Either::Right(_) => false,
    }
}

/// Starts Ollama detached from Zed: the desktop application when it is
/// installed next to the `ollama` executable found on `PATH`, otherwise
/// `ollama serve` without a console window. The child handle is dropped
/// without `kill_on_drop`, so the server outlives the session and is reaped
/// by smol's background reaper.
pub fn start_ollama() -> Result<()> {
    let executable = which::which("ollama")
        .context("`ollama` was not found on PATH; install Ollama or start it yourself")?;
    let mut command = match desktop_app(&executable) {
        Some(app) => util::command::new_command(app),
        None => {
            let mut command = util::command::new_command(&executable);
            command.arg("serve");
            command
        }
    };
    command
        .stdin(util::command::Stdio::null())
        .stdout(util::command::Stdio::null())
        .stderr(util::command::Stdio::null());
    let program = command.get_program().to_string_lossy().into_owned();
    log::info!("dictation: starting Ollama with {program}");
    command
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn desktop_app(executable: &Path) -> Option<std::path::PathBuf> {
    let app = executable.parent()?.join(DESKTOP_APP);
    app.is_file().then_some(app)
}

#[cfg(not(target_os = "windows"))]
fn desktop_app(_executable: &Path) -> Option<std::path::PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    struct FakeServer {
        answers: RefCell<VecDeque<bool>>,
        starts: Cell<usize>,
        waited: Cell<Duration>,
    }

    impl FakeServer {
        /// Answers each probe with the next value, then with the last one forever.
        fn answering(sequence: &[bool]) -> Self {
            Self {
                answers: RefCell::new(sequence.iter().copied().collect()),
                starts: Cell::new(0),
                waited: Cell::new(Duration::ZERO),
            }
        }

        fn run(&self, start_result: impl FnOnce() -> Result<()>) -> ServerOutcome {
            futures::executor::block_on(ensure_server(
                || async {
                    let mut answers = self.answers.borrow_mut();
                    let answer = answers.front().copied().unwrap_or(false);
                    if answers.len() > 1 {
                        answers.pop_front();
                    }
                    answer
                },
                || {
                    self.starts.set(self.starts.get() + 1);
                    start_result()
                },
                |duration| {
                    self.waited.set(self.waited.get() + duration);
                    async {}
                },
            ))
        }
    }

    #[test]
    fn a_server_that_answers_at_once_is_not_started() {
        let server = FakeServer::answering(&[true]);
        assert_eq!(server.run(|| Ok(())), ServerOutcome::Ready);
        assert_eq!(server.starts.get(), 0);
        assert_eq!(server.waited.get(), Duration::ZERO);
    }

    #[test]
    fn a_server_that_answers_after_a_few_polls_is_started_once() {
        let server = FakeServer::answering(&[false, false, false, true]);
        assert_eq!(server.run(|| Ok(())), ServerOutcome::Ready);
        assert_eq!(server.starts.get(), 1);
        assert_eq!(server.waited.get(), POLL * 3);
    }

    #[test]
    fn a_server_that_never_answers_fails_after_the_limit_with_one_start() {
        let server = FakeServer::answering(&[false]);
        let outcome = server.run(|| Ok(()));
        assert!(
            matches!(&outcome, ServerOutcome::Failed(reason) if reason.contains("20 seconds")),
            "{outcome:?}"
        );
        assert_eq!(server.starts.get(), 1);
        assert_eq!(server.waited.get(), START_LIMIT);
    }

    #[test]
    fn a_start_that_fails_is_reported_without_polling() {
        let server = FakeServer::answering(&[false]);
        let outcome = server.run(|| Err(anyhow::anyhow!("`ollama` was not found on PATH")));
        assert_eq!(
            outcome,
            ServerOutcome::Failed("`ollama` was not found on PATH".into())
        );
        assert_eq!(server.starts.get(), 1);
        assert_eq!(server.waited.get(), Duration::ZERO);
    }

    #[test]
    fn only_ollama_at_the_default_local_address_is_started() {
        assert!(should_start_ollama(Some("ollama"), ollama::OLLAMA_API_URL));
        assert!(should_start_ollama(
            Some("ollama"),
            "http://localhost:11434/"
        ));
        assert!(should_start_ollama(
            Some("ollama"),
            "http://127.0.0.1:11434"
        ));
        assert!(!should_start_ollama(
            Some("ollama"),
            "http://192.168.1.20:11434"
        ));
        assert!(!should_start_ollama(
            Some("ollama"),
            "https://ollama.example.com"
        ));
        assert!(!should_start_ollama(
            Some("anthropic"),
            ollama::OLLAMA_API_URL
        ));
        assert!(!should_start_ollama(None, ollama::OLLAMA_API_URL));
    }

    #[test]
    fn an_empty_address_setting_means_the_default() {
        assert_eq!(ollama_api_url(""), ollama::OLLAMA_API_URL);
        assert_eq!(ollama_api_url("  "), ollama::OLLAMA_API_URL);
        assert_eq!(ollama_api_url("http://box:11434"), "http://box:11434");
    }
}
