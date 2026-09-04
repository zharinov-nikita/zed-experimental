//! Local voice dictation engine: microphone capture, Whisper recognition via
//! transcribe.cpp and the live-transcript loop that turns speech into
//! Confirmed Text and Pending Text (see CONTEXT.md at the repository root).
//!
//! This crate has no GPUI dependency. The agent panel drives it from a
//! background task and receives `DictationEvent`s over a channel.

use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};
use audio::RodioExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use transcribe_cpp::{
    Backend, Model, ModelOptions, RunExtension, RunOptions, Session, SessionOptions, TimestampKind,
    WhisperRunOptions,
};

pub use cpal::DeviceId;

/// Whisper models are trained on 16 kHz mono audio.
pub const ENGINE_SAMPLE_RATE: u32 = 16_000;

/// How often the live loop looks at new audio.
const TICK: Duration = Duration::from_millis(400);
/// A pause at least this long ends the current phrase.
const SILENCE_TO_COMMIT: Duration = Duration::from_millis(700);
/// Do not bother recognizing phrases shorter than this; Whisper hallucinates on them.
const MIN_PHRASE: Duration = Duration::from_millis(900);
/// New audio required before re-recognizing the pending phrase.
const MIN_NEW_AUDIO_FOR_PENDING: Duration = Duration::from_millis(600);
/// Whisper works on 30 s windows; commit long phrases before they hit it.
const MAX_PHRASE: Duration = Duration::from_secs(24);
/// RMS below this is treated as silence. Microphone input is normalized to [-1, 1].
const SILENCE_RMS: f32 = 0.008;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    /// Whisper model file (ggml `.bin` or `.gguf`).
    pub model_path: PathBuf,
    /// Directory with ggml backend modules (`ggml-vulkan.dll` etc.). `None` looks next to the library.
    pub backends_dir: Option<PathBuf>,
    /// ISO language code, e.g. `ru`. `None` lets the model detect it.
    pub language: Option<String>,
    /// Terms passed to Whisper as the initial prompt so they are spelled correctly.
    pub glossary: Vec<String>,
    /// Decoder threads. `0` keeps the library default.
    pub threads: usize,
}

/// A loaded model plus a decoding session. One recognizer serves one dictation at a time.
pub struct Transcriber {
    model: Model,
    session: Session,
    run_options: RunOptions,
}

impl Transcriber {
    pub fn load(config: &EngineConfig) -> Result<Self> {
        init_backends(config.backends_dir.as_deref())?;
        if !config.model_path.is_file() {
            return Err(anyhow!(
                "dictation model not found: {}",
                config.model_path.display()
            ));
        }
        let model_options = ModelOptions {
            backend: Backend::Auto,
            ..Default::default()
        };
        let model = Model::load_with(&config.model_path, &model_options)
            .with_context(|| format!("loading {}", config.model_path.display()))?;
        let mut session_options = SessionOptions::default();
        if config.threads > 0 {
            session_options.n_threads = config.threads as i32;
        }
        let session = model
            .session_with(&session_options)
            .context("creating recognition session")?;

        let initial_prompt = if config.glossary.is_empty() {
            None
        } else {
            Some(config.glossary.join(", "))
        };
        // Greedy decoding without temperature fallback and a strict no-speech
        // threshold: the fallback path is where Whisper invents subtitles-style
        // filler on silence.
        let run_options = RunOptions {
            language: config.language.clone(),
            timestamps: TimestampKind::None,
            family: Some(RunExtension::Whisper(WhisperRunOptions {
                initial_prompt,
                condition_on_prev_tokens: Some(false),
                temperature: Some(0.0),
                temperature_inc: Some(0.0),
                no_speech_thold: Some(0.6),
                ..Default::default()
            })),
            ..Default::default()
        };
        log::info!(
            "dictation: loaded {} ({} / {}) on backend {}",
            config.model_path.display(),
            model.arch(),
            model.variant(),
            model.backend()
        );
        Ok(Self {
            model,
            session,
            run_options,
        })
    }

    /// Recognizes 16 kHz mono PCM in `[-1, 1]` and returns trimmed text with
    /// Whisper's well-known silence hallucinations removed.
    pub fn transcribe(&mut self, pcm: &[f32]) -> Result<String> {
        let pcm = trim_silence(pcm);
        if pcm.len() < seconds_to_samples(Duration::from_millis(300)) {
            return Ok(String::new());
        }
        let transcript = self
            .session
            .run(pcm, &self.run_options)
            .context("recognition failed")?;
        Ok(remove_hallucinations(&transcript.text))
    }

    pub fn backend(&self) -> String {
        self.model.backend()
    }
}

static BACKENDS: OnceLock<Result<(), String>> = OnceLock::new();

/// Backend registration is a process-wide, once-only operation in transcribe.cpp.
fn init_backends(dir: Option<&Path>) -> Result<()> {
    let result = BACKENDS.get_or_init(|| {
        let outcome = match dir {
            Some(dir) => transcribe_cpp::init_backends(dir),
            None => transcribe_cpp::init_backends_default(),
        };
        outcome.map_err(|error| error.to_string())
    });
    result
        .clone()
        .map_err(|error| anyhow!("initializing speech backends: {error}"))
}

/// Captures the microphone on its own thread into a growing 16 kHz mono buffer.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn start(device: Option<DeviceId>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
            ENGINE_SAMPLE_RATE as usize * 60,
        )));
        let (opened_tx, opened_rx) = mpsc::channel::<Result<()>>();
        let thread = thread::Builder::new()
            .name("DictationCapture".into())
            .spawn({
                let stop = stop.clone();
                let samples = samples.clone();
                move || {
                    // cpal streams must be created and polled on the same thread.
                    let source = match audio::open_input_stream(device) {
                        Ok(source) => source,
                        Err(error) => {
                            opened_tx.send(Err(error)).ok();
                            return;
                        }
                    };
                    let sample_rate = NonZero::new(ENGINE_SAMPLE_RATE)
                        .expect("engine sample rate is a non-zero constant");
                    let mut source = source
                        .possibly_disconnected_channels_to_mono()
                        .constant_samplerate(sample_rate);
                    opened_tx.send(Ok(())).ok();

                    let mut chunk = Vec::with_capacity(ENGINE_SAMPLE_RATE as usize / 50);
                    while !stop.load(Ordering::Relaxed) {
                        let Some(sample) = source.next() else {
                            log::warn!("dictation: microphone stream ended");
                            break;
                        };
                        chunk.push(sample);
                        if chunk.len() >= chunk.capacity() {
                            if let Ok(mut samples) = samples.lock() {
                                samples.extend_from_slice(&chunk);
                            }
                            chunk.clear();
                        }
                    }
                    if let Ok(mut samples) = samples.lock() {
                        samples.extend_from_slice(&chunk);
                    }
                }
            })
            .context("spawning capture thread")?;

        opened_rx
            .recv()
            .context("capture thread exited before opening the microphone")?
            .context("opening microphone")?;

        Ok(Self {
            stop,
            samples,
            thread: Some(thread),
        })
    }

    pub fn len(&self) -> usize {
        self.samples.lock().map(|samples| samples.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn samples_from(&self, start: usize) -> Vec<f32> {
        self.samples
            .lock()
            .map(|samples| samples.get(start..).unwrap_or(&[]).to_vec())
            .unwrap_or_default()
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DictationUpdate {
    /// Text that will not change anymore.
    pub confirmed: String,
    /// Tail that may still be rewritten as more speech arrives.
    pub pending: String,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub enum DictationEvent {
    Update(DictationUpdate),
    Error(String),
}

/// One dictation session: microphone → phrases → Confirmed/Pending text.
pub struct LiveDictation {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(Transcriber, String)>>>,
}

impl LiveDictation {
    /// Starts capturing. `confirmed_prefix` is used when resuming an existing
    /// Dictation Block: it is kept verbatim and new phrases are appended.
    pub fn start(
        transcriber: Transcriber,
        device: Option<DeviceId>,
        confirmed_prefix: String,
    ) -> Result<(Self, UnboundedReceiver<DictationEvent>)> {
        let recorder = Recorder::start(device)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (events_tx, events_rx) = unbounded();
        let worker = thread::Builder::new()
            .name("DictationLive".into())
            .spawn({
                let stop = stop.clone();
                move || live_loop(transcriber, recorder, confirmed_prefix, stop, events_tx)
            })
            .context("spawning dictation thread")?;
        Ok((
            Self {
                stop,
                worker: Some(worker),
            },
            events_rx,
        ))
    }

    /// Stops capturing, recognizes the remaining tail and returns the full
    /// confirmed text together with the recognizer for reuse.
    pub fn finish(mut self) -> Result<(Transcriber, String)> {
        self.stop.store(true, Ordering::Relaxed);
        let worker = self
            .worker
            .take()
            .ok_or_else(|| anyhow!("dictation already finished"))?;
        worker
            .join()
            .map_err(|_| anyhow!("dictation thread panicked"))?
    }
}

impl Drop for LiveDictation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn seconds_to_samples(duration: Duration) -> usize {
    (duration.as_secs_f64() * ENGINE_SAMPLE_RATE as f64) as usize
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let energy: f32 = samples.iter().map(|sample| sample * sample).sum();
    (energy / samples.len() as f32).sqrt()
}

fn has_speech(samples: &[f32]) -> bool {
    let window = seconds_to_samples(Duration::from_millis(100)).max(1);
    samples
        .chunks(window)
        .any(|chunk| rms(chunk) > SILENCE_RMS * 2.0)
}

/// Keeps at most 200 ms of silence on each side of the speech.
fn trim_silence(pcm: &[f32]) -> &[f32] {
    let window = seconds_to_samples(Duration::from_millis(50)).max(1);
    let margin = seconds_to_samples(Duration::from_millis(200));
    let loud = |chunk: &[f32]| rms(chunk) > SILENCE_RMS;
    let Some(first) = pcm.chunks(window).position(loud) else {
        return &[];
    };
    let Some(last) = pcm.chunks(window).rposition(loud) else {
        return &[];
    };
    let start = (first * window).saturating_sub(margin);
    let end = ((last + 1) * window + margin).min(pcm.len());
    &pcm[start..end]
}

/// Phrases Whisper produces on silence or noise instead of admitting there is
/// no speech: subtitle credits, channel sign-offs and similar filler.
const HALLUCINATIONS: &[&str] = &[
    "продолжение следует",
    "субтитр",
    "реклама",
    "спасибо за просмотр",
    "подписывайтесь",
    "подпишитесь",
    "ставьте лайк",
    "до новых встреч",
    "редактор",
    "корректор",
    "thank you for watching",
    "thanks for watching",
    "subtitles by",
    "please subscribe",
];

fn remove_hallucinations(text: &str) -> String {
    let mut kept = Vec::new();
    for sentence in split_sentences(text) {
        let lowered = sentence.to_lowercase();
        if HALLUCINATIONS.iter().any(|phrase| lowered.contains(phrase)) {
            continue;
        }
        if !sentence.chars().any(char::is_alphanumeric) {
            continue;
        }
        kept.push(sentence.trim().to_string());
    }
    kept.join(" ")
}

/// Splits on sentence punctuation while keeping the punctuation attached.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        current.push(character);
        if matches!(character, '.' | '!' | '?' | '…') {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    sentences
}

fn append_phrase(confirmed: &mut String, phrase: &str) {
    if phrase.is_empty() {
        return;
    }
    if !confirmed.is_empty() && !confirmed.ends_with(char::is_whitespace) {
        confirmed.push(' ');
    }
    confirmed.push_str(phrase);
}

fn live_loop(
    mut transcriber: Transcriber,
    recorder: Recorder,
    mut confirmed: String,
    stop: Arc<AtomicBool>,
    events: UnboundedSender<DictationEvent>,
) -> Result<(Transcriber, String)> {
    let started = Instant::now();
    let mut phrase_start = 0usize;
    let mut last_pending_len = 0usize;
    let mut pending = String::new();
    let silence_samples = seconds_to_samples(SILENCE_TO_COMMIT);
    let min_phrase = seconds_to_samples(MIN_PHRASE);
    let min_new_audio = seconds_to_samples(MIN_NEW_AUDIO_FOR_PENDING);
    let max_phrase = seconds_to_samples(MAX_PHRASE);

    let send = |events: &UnboundedSender<DictationEvent>, confirmed: &str, pending: &str| {
        events
            .unbounded_send(DictationEvent::Update(DictationUpdate {
                confirmed: confirmed.to_string(),
                pending: pending.to_string(),
                elapsed: started.elapsed(),
            }))
            .ok();
    };
    send(&events, &confirmed, &pending);

    while !stop.load(Ordering::Relaxed) {
        thread::sleep(TICK);
        let phrase = recorder.samples_from(phrase_start);
        if phrase.len() < min_phrase {
            continue;
        }
        let tail_is_silent = phrase.len() >= silence_samples
            && rms(&phrase[phrase.len() - silence_samples..]) < SILENCE_RMS;
        if !has_speech(&phrase) {
            // Skip leading silence so it never counts against the phrase length.
            if tail_is_silent {
                phrase_start += phrase.len() - silence_samples;
                last_pending_len = 0;
            }
            continue;
        }

        if tail_is_silent || phrase.len() >= max_phrase {
            match transcriber.transcribe(&phrase) {
                Ok(text) => append_phrase(&mut confirmed, &text),
                Err(error) => {
                    events
                        .unbounded_send(DictationEvent::Error(error.to_string()))
                        .ok();
                }
            }
            phrase_start += phrase.len();
            last_pending_len = 0;
            pending.clear();
            send(&events, &confirmed, &pending);
        } else if phrase.len() >= last_pending_len + min_new_audio {
            last_pending_len = phrase.len();
            match transcriber.transcribe(&phrase) {
                Ok(text) => pending = text,
                Err(error) => log::warn!("dictation: pending recognition failed: {error}"),
            }
            send(&events, &confirmed, &pending);
        }
    }

    let tail = recorder.samples_from(phrase_start);
    recorder.stop();
    if has_speech(&tail) && tail.len() >= seconds_to_samples(Duration::from_millis(300)) {
        let text = transcriber.transcribe(&tail)?;
        append_phrase(&mut confirmed, &text);
    }
    send(&events, &confirmed, "");
    Ok((transcriber, confirmed))
}

/// Decodes an audio file into 16 kHz mono PCM. Used by tools and tests.
pub fn load_audio_file(path: &Path) -> Result<Vec<f32>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let decoder = rodio::Decoder::new(std::io::BufReader::new(file))
        .with_context(|| format!("decoding {}", path.display()))?;
    let sample_rate =
        NonZero::new(ENGINE_SAMPLE_RATE).expect("engine sample rate is a non-zero constant");
    let samples: Vec<f32> = decoder
        .possibly_disconnected_channels_to_mono()
        .constant_samplerate(sample_rate)
        .collect();
    Ok(samples)
}
