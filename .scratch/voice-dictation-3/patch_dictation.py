import sys

p = 'crates/dictation/src/dictation.rs'
s = open(p, encoding='utf-8').read()

def rep(old, new):
    global s
    if old not in s:
        print('NOT FOUND:\n' + old[:200])
        sys.exit(1)
    s = s.replace(old, new, 1)

rep('''pub use cpal::DeviceId;
''', '''pub use audio::OpenedInputDevice;
pub use cpal::DeviceId;

pub mod engine_download;
pub mod playback;
pub mod session_audio;

pub use session_audio::{SessionAudioSink, SessionAudioStore};
''')

rep('''    pub fn segments(&mut self, pcm: &[f32]) -> Result<Vec<Segment>> {
        self.segments_after(pcm, "")
    }
''', '''    pub fn segments(&mut self, pcm: &[f32]) -> Result<Vec<Segment>> {
        self.segments_after(pcm, "", false)
    }
''')

rep('''    /// previous iteration's tokens over. Short buffers cut out of a sentence
    /// are decoded far more consistently with the sentence in front of them.
    fn segments_after(&mut self, pcm: &[f32], preceding_text: &str) -> Result<Vec<Segment>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
''', '''    /// previous iteration's tokens over. Short buffers cut out of a sentence
    /// are decoded far more consistently with the sentence in front of them.
    ///
    /// With `gate_no_speech` the decoder's own no-speech verdict is trusted
    /// outright: whisper.cpp drops a window only when its no-speech
    /// probability is above the threshold *and* the average log-probability
    /// is below `logprob_thold`, so raising that bound to zero (log
    /// probabilities never exceed it) leaves the probability alone in charge.
    /// Used for the tail on stop, where a silent window otherwise turns into
    /// a Recognizer Artifact (ADR 0001).
    fn segments_after(
        &mut self,
        pcm: &[f32],
        preceding_text: &str,
        gate_no_speech: bool,
    ) -> Result<Vec<Segment>> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
''')

rep('''        let context = tail_chars(preceding_text, CONTEXT_CHARS);
        let with_context;
        let run_options = if context.is_empty() {
            &self.run_options
        } else {
            let mut options = self.run_options.clone();
            let prompt = match &self.glossary_prompt {
                Some(glossary) => format!("{glossary}\\n{context}"),
                None => context.to_string(),
            };
            if let Some(RunExtension::Whisper(whisper)) = &mut options.family {
                whisper.initial_prompt = Some(prompt);
            }
            with_context = options;
            &with_context
        };
''', '''        let context = tail_chars(preceding_text, CONTEXT_CHARS);
        let adjusted;
        let run_options = if context.is_empty() && !gate_no_speech {
            &self.run_options
        } else {
            let mut options = self.run_options.clone();
            if let Some(RunExtension::Whisper(whisper)) = &mut options.family {
                if !context.is_empty() {
                    whisper.initial_prompt = Some(match &self.glossary_prompt {
                        Some(glossary) => format!("{glossary}\\n{context}"),
                        None => context.to_string(),
                    });
                }
                if gate_no_speech {
                    whisper.logprob_thold = Some(0.0);
                }
            }
            adjusted = options;
            &adjusted
        };
''')

rep('''pub struct Recorder {
    stop: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    thread: Option<JoinHandle<()>>,
}
''', '''pub struct Recorder {
    stop: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    thread: Option<JoinHandle<()>>,
    device: OpenedInputDevice,
}
''')
rep('''        let (opened_tx, opened_rx) = mpsc::channel::<Result<()>>();''',
    '''        let (opened_tx, opened_rx) = mpsc::channel::<Result<OpenedInputDevice>>();''')
rep('''                    let source = match audio::open_input_stream(device) {
                        Ok(source) => source,
                        Err(error) => {
                            opened_tx.send(Err(error)).ok();
                            ended.store(true, Ordering::Relaxed);
                            return;
                        }
                    };
                    let mut source = source
                        .possibly_disconnected_channels_to_mono()
                        .constant_samplerate(engine_sample_rate());
                    opened_tx.send(Ok(())).ok();
''', '''                    let (source, opened) = match audio::open_input_stream_reporting(device) {
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
''')
rep('''        opened_rx
            .recv()
            .context("capture thread exited before opening the microphone")?
            .context("opening microphone")?;

        Ok(Self {
            stop,
            ended,
            samples,
            thread: Some(thread),
        })
    }
''', '''        let device = opened_rx
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
''')

rep('''    /// existing Dictation Block: it is kept verbatim and new phrases are
    /// appended. With `save_recording_to` set, the whole session is written
    /// there as a WAV once it ends (see [`last_recording_path`]).
    pub fn start(
        transcriber: Transcriber,
        recorder: Recorder,
        confirmed_prefix: String,
        save_recording_to: Option<PathBuf>,
    ) -> Result<(Self, UnboundedReceiver<DictationEvent>)> {
        Self::start_with_source(
            transcriber,
            Box::new(recorder),
            confirmed_prefix,
            save_recording_to,
        )
    }
''', '''    /// existing Dictation Block: it is kept verbatim and new phrases are
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
''')
rep('''    fn start_with_source(
        transcriber: Transcriber,
        source: Box<dyn AudioSource>,
        confirmed_prefix: String,
        save_recording_to: Option<PathBuf>,
    ) -> Result<(Self, UnboundedReceiver<DictationEvent>)> {''', '''    fn start_with_source(
        transcriber: Transcriber,
        source: Box<dyn AudioSource>,
        confirmed_prefix: String,
        session_audio: Option<SessionAudioSink>,
    ) -> Result<(Self, UnboundedReceiver<DictationEvent>)> {''')
rep('''                    live_loop(
                        transcriber,
                        source,
                        confirmed_prefix,
                        save_recording_to,
                        stop,
                        events_tx,
                    )''', '''                    live_loop(
                        transcriber,
                        source,
                        confirmed_prefix,
                        session_audio,
                        stop,
                        events_tx,
                    )''')
rep('''fn live_loop(
    mut transcriber: Transcriber,
    mut source: Box<dyn AudioSource>,
    mut confirmed: String,
    save_recording_to: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    events: UnboundedSender<DictationEvent>,
) -> Result<(Transcriber, String)> {''', '''fn live_loop(
    mut transcriber: Transcriber,
    mut source: Box<dyn AudioSource>,
    mut confirmed: String,
    session_audio: Option<SessionAudioSink>,
    stop: Arc<AtomicBool>,
    events: UnboundedSender<DictationEvent>,
) -> Result<(Transcriber, String)> {''')
rep('''        let segments = match transcriber.segments_after(&buffer, &confirmed) {
            Ok(segments) => segments,
            Err(error) => {
                events
                    .unbounded_send(DictationEvent::Error(error.to_string()))
                    .ok();
                continue;
            }
        };
''', '''        let segments = match transcriber.segments_after(&buffer, &confirmed, false) {
            Ok(segments) => drop_decoder_loops(segments),
            Err(error) => {
                events
                    .unbounded_send(DictationEvent::Error(error.to_string()))
                    .ok();
                continue;
            }
        };
''')
rep('''    if let Some(path) = save_recording_to {
        match save_recording(&session_pcm, &path) {
            Ok(()) => log::info!("dictation: saved last recording to {}", path.display()),
            Err(error) => log::warn!("dictation: saving last recording failed: {error:#}"),
        }
    }
    let tail = session_pcm.get(commit_point..).unwrap_or(&[]);
    // Recognition may have fallen behind a slow backend, so the tail can be
    // longer than one Whisper window.
    for chunk in tail.chunks(window) {
        match transcriber.segments_after(chunk, &confirmed) {
            Ok(segments) => append_segments(&mut confirmed, &segments),
''', '''    if let Some(sink) = session_audio {
        match sink.append(&session_pcm) {
            Ok(Some(path)) => log::info!("dictation: session audio saved to {}", path.display()),
            Ok(None) => {}
            Err(error) => log::warn!("dictation: saving session audio failed: {error:#}"),
        }
    }
    let tail = session_pcm.get(commit_point..).unwrap_or(&[]);
    // Recognition may have fallen behind a slow backend, so the tail can be
    // longer than one Whisper window.
    for chunk in tail.chunks(window) {
        match transcriber.segments_after(chunk, &confirmed, true) {
            Ok(segments) => append_segments(&mut confirmed, &drop_decoder_loops(segments)),
''')

rep('''/// The single WAV the last Dictation Session is written to when
/// `save_last_recording` is on; every session overwrites it.
pub fn last_recording_path() -> PathBuf {
    std::env::temp_dir().join("zed-dictation-last-recording.wav")
}

''', '')

rep('''    #[test]
    fn last_recording_path_is_fixed_and_in_temp_dir() {
        let path = last_recording_path();
        assert_eq!(path, last_recording_path());
        assert!(path.starts_with(std::env::temp_dir()));
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("wav"));
    }
''', '''    fn segment(text: &str) -> Segment {
        Segment {
            start: Duration::ZERO,
            end: Duration::from_secs(1),
            text: text.to_string(),
        }
    }

    #[test]
    fn a_phrase_repeated_three_or_more_times_is_a_decoder_loop() {
        assert!(is_decoder_loop("git work tree, git work tree, git work tree"));
        assert!(is_decoder_loop(
            "Git work tree. Git work tree. Git work tree. Git work tree. Git work"
        ));
        assert!(is_decoder_loop("да да да"));
        assert!(is_decoder_loop(
            "Продолжение следует. Продолжение следует. Продолжение следует."
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
    }
''')

rep('''/// Picks the cut position for a segment boundary: the start of the quietest''',
'''/// The words of a segment as the Decoder Loop guard sees them: case and
/// punctuation do not make "Git work tree." differ from "git work tree".
fn loop_words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Minimum repetitions of one n-gram that make a segment a Decoder Loop.
const LOOP_REPEATS: usize = 3;

/// Whether `text` is one n-gram repeated `LOOP_REPEATS` or more times in a
/// row, optionally ending in an incomplete repetition. This is the shape a
/// Decoder Loop has on a near-empty buffer; the words themselves are never
/// consulted (ADR 0001), so "да да да" is dropped while "это очень очень
/// важно" stays.
pub fn is_decoder_loop(text: &str) -> bool {
    let words = loop_words(text);
    (1..=words.len() / LOOP_REPEATS).any(|n| {
        let unit = &words[..n];
        words.chunks(n).all(|chunk| unit.starts_with(chunk))
    })
}

/// Removes Decoder Loops from a recognized buffer: segments that loop inside
/// themselves, and runs of `LOOP_REPEATS` or more consecutive segments that
/// say the same thing, which is how the same loop looks when Whisper splits
/// it at timestamps.
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
        if end - start >= LOOP_REPEATS && !words[start].is_empty() {
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

/// Picks the cut position for a segment boundary: the start of the quietest''')

open(p, 'w', encoding='utf-8').write(s)
print('ok')
