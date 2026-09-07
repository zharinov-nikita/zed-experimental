//! Local voice dictation engine: microphone capture, Whisper recognition via
//! transcribe.cpp and the live-transcript loop that turns speech into
//! Confirmed Text and Pending Text (see CONTEXT.md at the repository root).
//!
//! This crate has no GPUI dependency. The agent panel drives it from a
//! background task and receives `DictationEvent`s over a channel.

use std::num::NonZero;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use audio::RodioExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use transcribe_cpp::{
    Backend, Model, ModelOptions, RunExtension, RunOptions, Session, SessionOptions, TimestampKind,
    WhisperRunOptions,
};

pub use audio::OpenedInputDevice;
pub use cpal::DeviceId;

pub mod engine_download;
pub mod playback;
pub mod session_audio;
pub mod speech_gate;

pub use session_audio::{SessionAudioSink, SessionAudioStore};
pub use speech_gate::{GateEvent, SpeechGate};

/// Where everything the dictation feature stores lives: `dictation` in the
/// Zed data directory. Engine Assets go into its `models` and `backends`
/// subfolders, Session Audio into `audio`.
pub fn data_dir() -> PathBuf {
    paths::data_dir().join("dictation")
}

pub fn session_audio_dir() -> PathBuf {
    data_dir().join("audio")
}

/// Puts `from` in place of `to`, keeping the old `to` until the new one is
/// in place: Windows refuses to rename over an existing file, and deleting
/// first would lose both copies when the rename then fails.
pub(crate) fn replace_file(from: &Path, to: &Path) -> Result<()> {
    let previous = to.with_extension("old");
    if to.exists() {
        std::fs::rename(to, &previous)
            .with_context(|| format!("setting aside {}", to.display()))?;
    }
    if let Err(error) = std::fs::rename(from, to) {
        if previous.exists() {
            std::fs::rename(&previous, to)
                .with_context(|| format!("restoring {}", to.display()))?;
        }
        return Err(error)
            .with_context(|| format!("moving {} to {}", from.display(), to.display()));
    }
    if previous.exists() {
        std::fs::remove_file(&previous)
            .with_context(|| format!("removing {}", previous.display()))?;
    }
    Ok(())
}

/// Whisper models are trained on 16 kHz mono audio.
pub const ENGINE_SAMPLE_RATE: u32 = 16_000;

fn engine_sample_rate() -> NonZero<u32> {
    NonZero::new(ENGINE_SAMPLE_RATE).expect("engine sample rate is a non-zero constant")
}

/// Whisper looks at 30 s of audio at a time; the buffer since the commit point never exceeds it.
const WINDOW: Duration = Duration::from_secs(30);
/// New audio required before the buffer is recognized again.
const STEP: Duration = Duration::from_secs(1);
/// A segment ending closer than this to the buffer end may still change and stays Pending Text.
const SETTLE: Duration = Duration::from_secs(1);
/// Whisper returns nothing for audio shorter than a second; shorter buffers are padded.
const MIN_AUDIO: Duration = Duration::from_secs(1);
/// How often the microphone source checks for new audio while waiting.
const POLL: Duration = Duration::from_millis(50);
/// Whisper places segment boundaries near pauses, not in them; the commit
/// point is moved to the quietest spot this far around the boundary.
const BOUNDARY_SEARCH: Duration = Duration::from_millis(250);
/// Frame used to compare loudness when looking for the quietest spot.
const BOUNDARY_FRAME: Duration = Duration::from_millis(20);
/// How much of the Confirmed Text is handed to Whisper as context for the next buffer.
const CONTEXT_CHARS: usize = 200;

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

/// One recognized phrase with its position in the audio that was recognized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
}

/// A loaded model plus a decoding session. One recognizer serves one dictation at a time.
pub struct Transcriber {
    model: Model,
    session: Session,
    run_options: RunOptions,
    glossary_prompt: Option<String>,
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

        let glossary_prompt = if config.glossary.is_empty() {
            None
        } else {
            Some(config.glossary.join(", "))
        };
        // Greedy decoding without temperature fallback and a strict no-speech
        // threshold: the fallback path is where Whisper invents subtitles-style
        // filler on silence.
        let run_options = RunOptions {
            language: config.language.clone(),
            timestamps: TimestampKind::Segment,
            family: Some(RunExtension::Whisper(WhisperRunOptions {
                initial_prompt: glossary_prompt.clone(),
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
            glossary_prompt,
        })
    }

    /// Recognizes 16 kHz mono PCM in `[-1, 1]` and returns the text as one string.
    pub fn transcribe(&mut self, pcm: &[f32]) -> Result<String> {
        let segments = self.segments(pcm)?;
        Ok(join_text(
            segments.iter().map(|segment| segment.text.as_str()),
        ))
    }

    /// Recognizes 16 kHz mono PCM in `[-1, 1]` and returns the phrases with
    /// their timestamps relative to the start of `pcm`.
    pub fn segments(&mut self, pcm: &[f32]) -> Result<Vec<Segment>> {
        Ok(drop_decoder_loops(self.segments_after(pcm, "")?))
    }

    /// Like `segments`, with the text spoken right before `pcm` given to the
    /// model as context, the way whisper.cpp's stream example carries the
    /// previous iteration's tokens over. Short buffers cut out of a sentence
    /// are decoded far more consistently with the sentence in front of them.
    fn segments_after(&mut self, pcm: &[f32], preceding_text: &str) -> Result<Vec<Segment>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
        let padded;
        let pcm = if pcm.len() < duration_to_samples(MIN_AUDIO) {
            padded = pad_with_silence(pcm, duration_to_samples(MIN_AUDIO));
            padded.as_slice()
        } else {
            pcm
        };
        let context = tail_chars(preceding_text, CONTEXT_CHARS);
        let adjusted;
        let run_options = if context.is_empty() {
            &self.run_options
        } else {
            let mut options = self.run_options.clone();
            if let Some(RunExtension::Whisper(whisper)) = &mut options.family {
                whisper.initial_prompt = Some(match &self.glossary_prompt {
                    Some(glossary) => format!("{glossary}\n{context}"),
                    None => context.to_string(),
                });
            }
            adjusted = options;
            &adjusted
        };
        let transcript = self
            .session
            .run(pcm, run_options)
            .context("recognition failed")?;
        Ok(transcript
            .segments
            .iter()
            .map(|segment| Segment {
                start: Duration::from_millis(segment.t0_ms.max(0) as u64),
                end: Duration::from_millis(segment.t1_ms.max(0) as u64),
                text: segment.text.trim().to_string(),
            })
            .filter(|segment| !segment.text.is_empty())
            .collect())
    }

    /// Recognizes audio that will not grow anymore (a phrase the Speech Gate
    /// has closed behind, or the tail on stop) and appends it to `confirmed`.
    /// Recognition may have fallen behind a slow backend, so the audio can
    /// be longer than one Whisper window. Errors are reported per window and
    /// do not stop the remaining windows from being recognized.
    fn recognize_final(
        &mut self,
        pcm: &[f32],
        confirmed: &mut String,
        events: &UnboundedSender<DictationEvent>,
    ) {
        for chunk in pcm.chunks(duration_to_samples(WINDOW)) {
            match self.segments_after(chunk, confirmed) {
                Ok(segments) => append_segments(confirmed, &drop_decoder_loops(segments)),
                Err(error) => {
                    log::warn!("dictation: recognizing a finished phrase failed: {error:#}");
                    events
                        .unbounded_send(DictationEvent::Error(error.to_string()))
                        .ok();
                }
            }
        }
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

/// Where the live loop gets its 16 kHz mono samples from: the microphone or a
/// pre-recorded buffer. Samples are addressed by their position since the
/// start of the session and never go away while the source lives.
trait AudioSource: Send {
    /// Blocks until at least `wanted` samples are available, the source ends
    /// or `stop` is raised. Returns `None` once no more audio will ever come.
    fn wait_for(&mut self, wanted: usize, stop: &AtomicBool) -> Option<usize>;
    /// Samples available so far.
    fn len(&self) -> usize;
    fn samples(&self, range: Range<usize>) -> Vec<f32>;
    /// Stops capturing; after this `len` and `samples` see everything the
    /// source ever produced.
    fn stop(&mut self);
}

/// Captures the microphone on its own thread into a growing 16 kHz mono buffer.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    thread: Option<JoinHandle<()>>,
    device: OpenedInputDevice,
}

impl Recorder {
    pub fn start(device: Option<DeviceId>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
            ENGINE_SAMPLE_RATE as usize * 60,
        )));
        let (opened_tx, opened_rx) = mpsc::channel::<Result<OpenedInputDevice>>();
        let thread = thread::Builder::new()
            .name("DictationCapture".into())
            .spawn({
                let stop = stop.clone();
                let ended = ended.clone();
                let samples = samples.clone();
                move || {
                    // cpal streams must be created and polled on the same thread.
                    let (source, opened) = match audio::open_input_stream_reporting(device) {
                        Ok(opened) => opened,
                        Err(error) => {
                            opened_tx.send(Err(error)).ok();
                            ended.store(true, Ordering::Relaxed);
                            return;
                        }
                    };
                    let mut source = source
                        .possibly_disconnected_channels_to_mono()
                        .constant_samplerate(engine_sample_rate());
                    opened_tx.send(Ok(opened)).ok();

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
                    ended.store(true, Ordering::Relaxed);
                }
            })
            .context("spawning capture thread")?;

        let device = opened_rx
            .recv()
            .context("capture thread exited before opening the microphone")?
            .context("opening microphone")?;

        Ok(Self {
            stop,
            ended,
            samples,
            thread: Some(thread),
            device,
        })
    }

    /// The input the session is actually captured from, and whether the
    /// configured device was missing so the default was opened instead.
    pub fn device(&self) -> &OpenedInputDevice {
        &self.device
    }

    pub fn len(&self) -> usize {
        self.samples
            .lock()
            .map(|samples| samples.len())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Recorder {
    /// Stops the capture thread and waits for it to flush its last chunk.
    fn stop_capture(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

/// Releases the microphone before the recorder is gone, so the next session
/// never finds the device still open.
impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop_capture();
    }
}

impl AudioSource for Recorder {
    fn wait_for(&mut self, wanted: usize, stop: &AtomicBool) -> Option<usize> {
        loop {
            let available = self.len();
            if available >= wanted || stop.load(Ordering::Relaxed) {
                return Some(available);
            }
            if self.ended.load(Ordering::Relaxed) {
                return None;
            }
            thread::sleep(POLL);
        }
    }

    fn len(&self) -> usize {
        Recorder::len(self)
    }

    fn samples(&self, range: Range<usize>) -> Vec<f32> {
        self.samples
            .lock()
            .map(|samples| samples.get(range).unwrap_or(&[]).to_vec())
            .unwrap_or_default()
    }

    fn stop(&mut self) {
        self.stop_capture();
    }
}

/// A pre-recorded buffer revealed to the loop as fast as it asks for it.
struct PcmSource {
    samples: Vec<f32>,
    revealed: usize,
}

impl AudioSource for PcmSource {
    fn wait_for(&mut self, wanted: usize, stop: &AtomicBool) -> Option<usize> {
        if stop.load(Ordering::Relaxed) {
            return Some(self.revealed);
        }
        if wanted > self.samples.len() && self.revealed == self.samples.len() {
            return None;
        }
        self.revealed = wanted.min(self.samples.len());
        Some(self.revealed)
    }

    fn len(&self) -> usize {
        self.revealed
    }

    fn samples(&self, range: Range<usize>) -> Vec<f32> {
        let range = range.start.min(self.revealed)..range.end.min(self.revealed);
        self.samples.get(range).unwrap_or(&[]).to_vec()
    }

    fn stop(&mut self) {}
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DictationUpdate {
    /// Text that will not change anymore.
    pub confirmed: String,
    /// Tail that may still be rewritten as more speech arrives.
    pub pending: String,
    /// Audio captured so far.
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub enum DictationEvent {
    Update(DictationUpdate),
    Error(String),
}

/// One dictation session: audio source → segments → Confirmed/Pending text.
pub struct LiveDictation {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(Transcriber, String)>>>,
}

impl LiveDictation {
    /// Recognizes an already opened microphone. The caller opens the
    /// [`Recorder`] only once the model is loaded so that nothing is captured
    /// during Model Loading. `confirmed_prefix` is used when resuming an
    /// existing Dictation Block: it is kept verbatim and new phrases are
    /// appended. With `session_audio` set, everything captured is appended
    /// to the block's Session Audio once the session ends.
    pub fn start(
        transcriber: Transcriber,
        recorder: Recorder,
        confirmed_prefix: String,
        session_audio: Option<SessionAudioSink>,
    ) -> Result<(Self, UnboundedReceiver<DictationEvent>)> {
        Self::start_with_source(
            transcriber,
            Box::new(recorder),
            confirmed_prefix,
            session_audio,
        )
    }

    /// Runs the same loop over pre-recorded 16 kHz mono PCM instead of the
    /// microphone, as fast as recognition allows. The event stream ends once
    /// the buffer is exhausted; `finish` then returns the full text.
    pub fn start_from_pcm(
        transcriber: Transcriber,
        pcm: Vec<f32>,
        confirmed_prefix: String,
    ) -> Result<(Self, UnboundedReceiver<DictationEvent>)> {
        let source = PcmSource {
            samples: pcm,
            revealed: 0,
        };
        Self::start_with_source(transcriber, Box::new(source), confirmed_prefix, None)
    }

    fn start_with_source(
        transcriber: Transcriber,
        source: Box<dyn AudioSource>,
        confirmed_prefix: String,
        session_audio: Option<SessionAudioSink>,
    ) -> Result<(Self, UnboundedReceiver<DictationEvent>)> {
        let stop = Arc::new(AtomicBool::new(false));
        let (events_tx, events_rx) = unbounded();
        let worker = thread::Builder::new()
            .name("DictationLive".into())
            .spawn({
                let stop = stop.clone();
                move || {
                    live_loop(
                        transcriber,
                        source,
                        confirmed_prefix,
                        session_audio,
                        stop,
                        events_tx,
                    )
                }
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

pub(crate) fn duration_to_samples(duration: Duration) -> usize {
    (duration.as_secs_f64() * ENGINE_SAMPLE_RATE as f64) as usize
}

fn samples_to_duration(samples: usize) -> Duration {
    Duration::from_secs_f64(samples as f64 / ENGINE_SAMPLE_RATE as f64)
}

fn pad_with_silence(pcm: &[f32], length: usize) -> Vec<f32> {
    let mut padded = pcm.to_vec();
    padded.resize(length.max(pcm.len()), 0.0);
    padded
}

/// The last `count` characters of `text`, starting at a word boundary.
fn tail_chars(text: &str, count: usize) -> &str {
    let text = text.trim();
    let start = text
        .char_indices()
        .rev()
        .nth(count.saturating_sub(1))
        .map(|(index, _)| index)
        .unwrap_or(0);
    let tail = &text[start..];
    match tail.find(char::is_whitespace) {
        Some(space) if start > 0 => tail[space..].trim_start(),
        _ => tail,
    }
}

fn join_text<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = String::new();
    for part in parts {
        append_text(&mut text, part);
    }
    text
}

fn append_segments(text: &mut String, segments: &[Segment]) {
    for segment in segments {
        append_text(text, &segment.text);
    }
}

fn append_text(text: &mut String, part: &str) {
    let part = part.trim();
    if part.is_empty() {
        return;
    }
    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
        text.push(' ');
    }
    text.push_str(part);
}

/// The words of a segment as the Decoder Loop guard sees them: case and
/// punctuation do not make "Git work tree." differ from "git work tree".
fn loop_words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Minimum repetitions of one n-gram that make a segment a Decoder Loop.
const LOOP_REPEATS: usize = 3;

/// Whether `words` contain a run of one n-gram of at least `min_n` words
/// repeated `LOOP_REPEATS` or more times in a row.
fn has_repeated_run(words: &[String], min_n: usize) -> bool {
    (min_n..=words.len() / LOOP_REPEATS).any(|n| {
        (0..=words.len() - LOOP_REPEATS * n).any(|start| {
            let unit = &words[start..start + n];
            (1..LOOP_REPEATS).all(|repeat| {
                let from = start + repeat * n;
                words.get(from..from + n) == Some(unit)
            })
        })
    })
}

/// Whether `text` has the shape of a Decoder Loop: the whole text is one
/// n-gram repeated `LOOP_REPEATS` or more times in a row (an incomplete last
/// repetition allowed), or a phrase of two or more words is repeated that
/// often anywhere inside it. The words themselves are never consulted
/// (ADR 0001): "да да да" and "раз, git work tree, git work tree, git work
/// tree" are dropped, while "нет, нет, нет, я имею в виду другое" stays,
/// because people do repeat a single word.
pub fn is_decoder_loop(text: &str) -> bool {
    let words = loop_words(text);
    let whole = (1..=words.len() / LOOP_REPEATS).any(|n| {
        let unit = &words[..n];
        words.chunks(n).all(|chunk| unit.starts_with(chunk))
    });
    whole || has_repeated_run(&words, 2)
}

/// Removes Decoder Loops from a recognized buffer: segments that loop inside
/// themselves, and runs of `LOOP_REPEATS` or more consecutive segments that
/// repeat the same phrase of two or more words, which is how the same loop
/// looks when Whisper splits it at timestamps. Single-word segments are
/// left alone for the same reason a single repeated word is.
fn drop_decoder_loops(segments: Vec<Segment>) -> Vec<Segment> {
    let segments: Vec<Segment> = segments
        .into_iter()
        .filter(|segment| !is_decoder_loop(&segment.text))
        .collect();
    let words: Vec<Vec<String>> = segments
        .iter()
        .map(|segment| loop_words(&segment.text))
        .collect();
    let mut keep = vec![true; segments.len()];
    let mut start = 0;
    while start < segments.len() {
        let mut end = start + 1;
        while end < segments.len() && words[end] == words[start] {
            end += 1;
        }
        if end - start >= LOOP_REPEATS && words[start].len() >= 2 {
            keep[start..end].fill(false);
        }
        start = end;
    }
    segments
        .into_iter()
        .zip(keep)
        .filter_map(|(segment, keep)| keep.then_some(segment))
        .collect()
}

/// Picks the cut position for a segment boundary: the start of the quietest
/// frame within `BOUNDARY_SEARCH` of `boundary`. Segment timestamps are
/// approximate and often land on the first syllable of the next phrase;
/// cutting there hands the next window a broken word to recognize.
fn quietest_point(pcm: &[f32], boundary: usize) -> usize {
    let radius = duration_to_samples(BOUNDARY_SEARCH);
    let frame = duration_to_samples(BOUNDARY_FRAME).max(1);
    let start = boundary.saturating_sub(radius);
    let end = (boundary + radius).min(pcm.len());
    if start >= end {
        return boundary.min(pcm.len());
    }
    let energy = |samples: &[f32]| samples.iter().map(|sample| sample * sample).sum::<f32>();
    pcm[start..end]
        .chunks(frame)
        .enumerate()
        .filter(|(_, chunk)| chunk.len() == frame)
        .map(|(index, chunk)| (index, energy(chunk)))
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| start + index * frame)
        .unwrap_or(boundary.min(pcm.len()))
}

/// How many leading segments of a recognized buffer become Confirmed Text.
///
/// The last segment always stays pending: its end is where new speech is still
/// arriving. Earlier segments are confirmed once they end at least `SETTLE`
/// before the buffer end. When the buffer has reached the Whisper window the
/// loop cannot wait any longer, so everything but the last segment is
/// confirmed; a lone segment filling the whole window is confirmed too,
/// otherwise the commit point could never move again.
fn segments_to_confirm(segments: &[Segment], buffer: Duration, at_window: bool) -> usize {
    let Some(candidates) = segments.len().checked_sub(1) else {
        return 0;
    };
    if at_window {
        return candidates.max(1);
    }
    segments[..candidates]
        .iter()
        .take_while(|segment| segment.end + SETTLE <= buffer)
        .count()
}

/// The text of the session and how far into the audio it reaches, as the
/// live loop keeps them between iterations.
struct Transcript {
    confirmed: String,
    pending: String,
    /// Audio before this point is either confirmed or silence; recognition
    /// starts here.
    commit_point: usize,
    /// Where the speech since the commit point began; `None` is silence
    /// since the commit point, and then nothing is recognized.
    speech_start: Option<usize>,
}

impl Transcript {
    /// Handles one Speech Gate transition. Speech beginning after silence
    /// moves the commit point to its start, so the silence never enters the
    /// recognition window; speech beginning while earlier speech is still
    /// being recognized simply extends it. The gate closing behind a phrase
    /// makes the phrase complete: it is recognized once without the silence
    /// after it and confirmed whole. `samples` reads session audio by
    /// position; `available` is how much of it exists.
    fn apply_gate_event(
        &mut self,
        event: GateEvent,
        available: usize,
        samples: impl FnOnce(Range<usize>) -> Vec<f32>,
        transcriber: &mut Transcriber,
        events: &UnboundedSender<DictationEvent>,
    ) {
        match event {
            GateEvent::Opened { start } => {
                if self.speech_start.is_none() {
                    self.commit_point = self.commit_point.max(start);
                    self.speech_start = Some(self.commit_point);
                }
            }
            GateEvent::Closed { end } => {
                let phrase_end = end.clamp(self.commit_point.min(available), available);
                let phrase = samples(self.commit_point..phrase_end);
                transcriber.recognize_final(&phrase, &mut self.confirmed, events);
                self.commit_point = phrase_end;
                self.pending.clear();
                self.speech_start = None;
            }
        }
    }
}

fn live_loop(
    mut transcriber: Transcriber,
    mut source: Box<dyn AudioSource>,
    confirmed: String,
    session_audio: Option<SessionAudioSink>,
    stop: Arc<AtomicBool>,
    events: UnboundedSender<DictationEvent>,
) -> Result<(Transcriber, String)> {
    let window = duration_to_samples(WINDOW);
    let step = duration_to_samples(STEP);
    let settle = duration_to_samples(SETTLE);
    let mut recognized_up_to = 0usize;
    let mut last_recognized_end = 0usize;
    let mut gate = SpeechGate::new();
    let mut transcript = Transcript {
        confirmed,
        pending: String::new(),
        commit_point: 0,
        speech_start: None,
    };

    let send = |confirmed: &str, pending: &str, captured: usize| {
        events
            .unbounded_send(DictationEvent::Update(DictationUpdate {
                confirmed: confirmed.to_string(),
                pending: pending.to_string(),
                elapsed: samples_to_duration(captured),
            }))
            .ok();
    };
    send(&transcript.confirmed, &transcript.pending, 0);

    while !stop.load(Ordering::Relaxed) {
        let Some(available) = source.wait_for(recognized_up_to + step, &stop) else {
            break;
        };
        if available < recognized_up_to + step {
            continue;
        }
        recognized_up_to = available;
        let new_audio = source.samples(gate.position()..available);
        for event in gate.feed(&new_audio) {
            transcript.apply_gate_event(
                event,
                available,
                |range| source.samples(range),
                &mut transcriber,
                &events,
            );
        }
        if transcript.speech_start.is_none() {
            // Silence since the commit point: the decoder does not run and
            // the Pending Text is empty.
            send(&transcript.confirmed, "", available);
            continue;
        }

        // The gate is open: recognize up to where speech was last heard,
        // never the silence after it, so a Recognizer Artifact has nothing to
        // grow from even before the gate closes. Silence adds no audio to
        // the buffer, so the same buffer is not recognized twice.
        let commit_point = transcript.commit_point;
        let speech_end = gate.speech_end().unwrap_or(available);
        let buffer_end = available.min(speech_end).min(commit_point + window);
        if buffer_end <= last_recognized_end {
            send(&transcript.confirmed, &transcript.pending, available);
            continue;
        }
        last_recognized_end = buffer_end;
        let at_window = buffer_end.saturating_sub(commit_point) >= window - settle;
        let buffer = source.samples(commit_point..buffer_end);
        let segments = match transcriber.segments_after(&buffer, &transcript.confirmed) {
            Ok(segments) => drop_decoder_loops(segments),
            Err(error) => {
                events
                    .unbounded_send(DictationEvent::Error(error.to_string()))
                    .ok();
                continue;
            }
        };
        let confirm = segments_to_confirm(&segments, samples_to_duration(buffer.len()), at_window);
        let (confirmed_now, still_pending) = segments.split_at(confirm);
        if let Some(last) = confirmed_now.last() {
            append_segments(&mut transcript.confirmed, confirmed_now);
            transcript.commit_point += quietest_point(&buffer, duration_to_samples(last.end));
        } else if at_window && segments.is_empty() {
            // A full window with nothing in it is silence; drop it so the
            // loop keeps looking at fresh audio.
            transcript.commit_point = buffer_end - settle;
        }
        transcript.pending = join_text(still_pending.iter().map(|segment| segment.text.as_str()));
        send(&transcript.confirmed, &transcript.pending, available);
    }

    source.stop();
    let session_pcm = source.samples(0..source.len());
    let captured = session_pcm.len();
    drop(source);
    if let Some(sink) = session_audio {
        match sink.append(&session_pcm) {
            Ok(Some(path)) => log::info!("dictation: session audio saved to {}", path.display()),
            Ok(None) => {}
            Err(error) => log::warn!("dictation: saving session audio failed: {error:#}"),
        }
    }
    // The gate sees the last audio; a phrase it closes behind is confirmed
    // like any other, and whatever is still open is the tail, cut where the
    // gate saw speech end so the silence before the stop is never decoded.
    let unseen = session_pcm.get(gate.position()..).unwrap_or(&[]);
    for event in gate.feed(unseen) {
        transcript.apply_gate_event(
            event,
            captured,
            |range| session_pcm.get(range).unwrap_or(&[]).to_vec(),
            &mut transcriber,
            &events,
        );
    }
    if transcript.speech_start.is_some() {
        let commit_point = transcript.commit_point.min(captured);
        let tail_end = gate
            .speech_end()
            .unwrap_or(captured)
            .clamp(commit_point, captured);
        let tail = session_pcm.get(commit_point..tail_end).unwrap_or(&[]);
        transcriber.recognize_final(tail, &mut transcript.confirmed, &events);
    }
    send(&transcript.confirmed, "", captured);
    Ok((transcriber, transcript.confirmed))
}

/// Writes 16 kHz mono PCM as a WAV that [`load_audio_file`] and the
/// `transcribe_wav` example read back unchanged.
pub fn save_recording(pcm: &[f32], path: &Path) -> Result<()> {
    let source =
        rodio::buffer::SamplesBuffer::new(rodio::nz!(1), engine_sample_rate(), pcm.to_vec());
    rodio::wav_to_file(source, path).with_context(|| format!("writing {}", path.display()))
}

/// Decodes an audio file into 16 kHz mono PCM. Used by tools and tests.
pub fn load_audio_file(path: &Path) -> Result<Vec<f32>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let decoder = rodio::Decoder::new(std::io::BufReader::new(file))
        .with_context(|| format!("decoding {}", path.display()))?;
    let samples: Vec<f32> = decoder
        .possibly_disconnected_channels_to_mono()
        .constant_samplerate(engine_sample_rate())
        .collect();
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_recording_loads_back_as_engine_pcm() {
        let pcm: Vec<f32> = (0..ENGINE_SAMPLE_RATE * 2)
            .map(|index| ((index % 100) as f32 / 100.0 - 0.5) * 0.4)
            .collect();
        let path =
            std::env::temp_dir().join(format!("zed-dictation-test-{}.wav", std::process::id()));

        save_recording(&pcm, &path).expect("saving");
        let loaded = load_audio_file(&path).expect("loading");
        std::fs::remove_file(&path).expect("removing the test file");

        assert_eq!(loaded.len(), pcm.len());
        let max_error = loaded
            .iter()
            .zip(&pcm)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0f32, f32::max);
        assert!(max_error < 1e-6, "samples changed by up to {max_error}");
    }

    fn segment(text: &str) -> Segment {
        Segment {
            start: Duration::ZERO,
            end: Duration::from_secs(1),
            text: text.to_string(),
        }
    }

    #[test]
    fn a_phrase_repeated_three_or_more_times_is_a_decoder_loop() {
        assert!(is_decoder_loop(
            "git work tree, git work tree, git work tree"
        ));
        assert!(is_decoder_loop(
            "Git work tree. Git work tree. Git work tree. Git work tree. Git work"
        ));
        assert!(is_decoder_loop("да да да"));
        assert!(is_decoder_loop(
            "Продолжение следует. Продолжение следует. Продолжение следует."
        ));
        assert!(is_decoder_loop(
            "Раз, два, git work tree, git work tree, git work tree."
        ));
    }

    #[test]
    fn speech_with_a_single_repeated_word_is_not_a_decoder_loop() {
        assert!(!is_decoder_loop("это очень очень важно"));
        assert!(!is_decoder_loop("1, 2, 3, 4, 5"));
        assert!(!is_decoder_loop("git work tree, git work tree"));
        assert!(!is_decoder_loop("да да"));
        assert!(!is_decoder_loop(""));
        assert!(!is_decoder_loop("нет, нет, нет, я имею в виду другое"));
        assert!(!is_decoder_loop("git work tree, git work tree, и всё"));
    }

    #[test]
    fn looping_segments_are_dropped_and_speech_is_kept() {
        let kept = drop_decoder_loops(vec![
            segment("Раз, два, три."),
            segment("git work tree, git work tree, git work tree, git work tree"),
            segment("четыре, пять."),
        ]);
        let texts: Vec<&str> = kept.iter().map(|segment| segment.text.as_str()).collect();
        assert_eq!(texts, vec!["Раз, два, три.", "четыре, пять."]);
    }

    #[test]
    fn a_run_of_identical_segments_is_a_decoder_loop_too() {
        let kept = drop_decoder_loops(vec![
            segment("Раз, два, три."),
            segment("git work tree"),
            segment("Git work tree."),
            segment("git work tree"),
            segment("четыре"),
            segment("четыре"),
        ]);
        let texts: Vec<&str> = kept.iter().map(|segment| segment.text.as_str()).collect();
        assert_eq!(texts, vec!["Раз, два, три.", "четыре", "четыре"]);

        let single_words = drop_decoder_loops(vec![segment("да."), segment("Да."), segment("да")]);
        assert_eq!(single_words.len(), 3, "single-word segments are speech");
    }
}
