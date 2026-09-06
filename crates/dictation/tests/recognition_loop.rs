//! Runs the live recognition loop over real recordings and checks what comes
//! out through the public API: the order in which text is confirmed and the
//! final text. Needs a Whisper model, so the tests return early unless
//! `ZED_DICTATION_MODEL` and `ZED_DICTATION_RECORDINGS` are set:
//!
//! ```text
//! ZED_DICTATION_MODEL=<model.bin> ZED_DICTATION_BACKENDS=<backends_dir> \
//! ZED_DICTATION_RECORDINGS=<dir with .wav> cargo test -p dictation
//! ```

use std::env;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use dictation::{
    DictationEvent, DictationUpdate, EngineConfig, LiveDictation, Transcriber, load_audio_file,
};
use futures::StreamExt as _;

const MODEL_VAR: &str = "ZED_DICTATION_MODEL";
const BACKENDS_VAR: &str = "ZED_DICTATION_BACKENDS";
const RECORDINGS_VAR: &str = "ZED_DICTATION_RECORDINGS";

/// Words that must appear in the final text of the Handy recordings the loop
/// was tuned on. Other recordings in the directory still go through the
/// monotonicity and whole-file comparisons.
const EXPECTED_WORDS: &[(&str, &[&str])] = &[
    ("handy-1788483884.wav", &["prompt", "подойдет"]),
    ("handy-1788483932.wav", &["вопрос", "понял", "имеешь"]),
    ("handy-1788483948.wav", &["вопрос", "аудио"]),
    (
        "handy-1788484168.wav",
        &["модели", "загружать", "несколько"],
    ),
    ("handy-1788484196.wav", &["да"]),
];

/// Spoken digits followed by silence, synthesized with the Windows speech
/// engine (see `LOCAL_DEV.md`). Whisper tends to loop on the silent tail and
/// to invent a closing phrase on stop; neither may reach the text.
const DIGITS_FIXTURE: &str = "zed-digits-with-silence.wav";

/// One model per test binary: loading takes seconds and a gigabyte of VRAM.
/// Holding the guard for the whole test also keeps the tests sequential.
static ENGINE: Mutex<Option<Transcriber>> = Mutex::new(None);

struct Fixture {
    engine: MutexGuard<'static, Option<Transcriber>>,
    recordings: Vec<PathBuf>,
}

fn fixture() -> Option<Fixture> {
    let (Some(model_path), Some(recordings_dir)) =
        (env::var_os(MODEL_VAR), env::var_os(RECORDINGS_VAR))
    else {
        eprintln!("skipping: set {MODEL_VAR} and {RECORDINGS_VAR} to run recognition tests");
        return None;
    };
    let mut engine = ENGINE.lock().unwrap_or_else(PoisonError::into_inner);
    if engine.is_none() {
        let config = EngineConfig {
            model_path: PathBuf::from(model_path),
            backends_dir: env::var_os(BACKENDS_VAR).map(PathBuf::from),
            language: Some("ru".into()),
            glossary: [
                "TypeScript",
                "JavaScript",
                "GitHub",
                "Docker",
                "Kubernetes",
                "API",
                "Rust",
                "PowerShell",
                "Claude Code",
                "Ollama",
                "frontend",
                "backend",
                "deploy",
                "commit",
                "pull request",
                "merge",
                "refactoring",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            threads: 0,
        };
        *engine = Some(Transcriber::load(&config).expect("loading the dictation model"));
    }
    let mut recordings: Vec<PathBuf> = std::fs::read_dir(&recordings_dir)
        .expect("reading the recordings directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "wav"))
        .collect();
    recordings.sort();
    assert!(
        !recordings.is_empty(),
        "no .wav files in {}",
        recordings_dir.to_string_lossy()
    );
    Some(Fixture { engine, recordings })
}

impl Fixture {
    fn take_engine(&mut self) -> Transcriber {
        self.engine
            .take()
            .expect("the engine was not returned by a previous test")
    }

    fn return_engine(&mut self, transcriber: Transcriber) {
        *self.engine = Some(transcriber);
    }

    /// Runs the loop over the whole buffer and collects every update plus the final text.
    fn run_loop(&mut self, pcm: Vec<f32>) -> (Vec<DictationUpdate>, String) {
        let transcriber = self.take_engine();
        let (live, mut events) =
            LiveDictation::start_from_pcm(transcriber, pcm, String::new()).expect("starting");
        let mut updates = Vec::new();
        futures::executor::block_on(async {
            while let Some(event) = events.next().await {
                match event {
                    DictationEvent::Update(update) => {
                        eprintln!(
                            "{:5.1}s confirmed={:?} pending={:?}",
                            update.elapsed.as_secs_f32(),
                            update.confirmed,
                            update.pending
                        );
                        updates.push(update);
                    }
                    DictationEvent::Error(error) => panic!("recognition error: {error}"),
                }
            }
        });
        let (transcriber, text) = live.finish().expect("finishing");
        self.return_engine(transcriber);
        (updates, text)
    }

    fn transcribe_whole(&mut self, pcm: &[f32]) -> String {
        let mut transcriber = self.take_engine();
        let text = transcriber.transcribe(pcm).expect("whole-file recognition");
        self.return_engine(transcriber);
        text
    }
}

/// Lowercased words without punctuation, so two transcripts can be compared
/// up to casing and punctuation.
fn words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Letters only, so that spacing choices like "ввиду" / "в виду" do not count.
fn letters(text: &str) -> Vec<char> {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn edit_distance(left: &[char], right: &[char]) -> usize {
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (row, left_char) in left.iter().enumerate() {
        let mut current = vec![row + 1];
        for (column, right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != right_char);
            let insertion = current[column] + 1;
            let deletion = previous[column + 1] + 1;
            current.push(substitution.min(insertion).min(deletion));
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(0)
}

/// Whisper does not decode exactly the same text on every window, so the
/// streaming text may differ from a single pass over the whole file in a
/// short word or a spelling ("Виспер" against "vizper"). Anything beyond a
/// fifth of the text means the loop lost or duplicated a phrase.
fn assert_same_speech(name: &str, actual: &str, reference: &str) {
    let actual_letters = letters(actual);
    let reference_letters = letters(reference);
    let allowed = (reference_letters.len() / 5).max(1);
    let distance = edit_distance(&actual_letters, &reference_letters);
    assert!(
        distance <= allowed,
        "{name}: {actual:?} differs from {reference:?} by {distance} letters, allowed {allowed}"
    );
}

/// The shape of a Decoder Loop, whatever the words: the whole text is one
/// word repeated three or more times, or a phrase of two or more words is
/// repeated that often anywhere in `text`. A single word repeated inside a
/// sentence is speech.
fn has_repeated_ngram(text: &str) -> bool {
    let words = words(text);
    let whole_single_word = words.len() >= 3 && words.iter().all(|word| word == &words[0]);
    whole_single_word
        || (2..=words.len() / 3).any(|n| {
            (0..=words.len() - 3 * n).any(|start| {
                let unit = &words[start..start + n];
                (1..3).all(|repeat| {
                    let from = start + repeat * n;
                    words.get(from..from + n) == Some(unit)
                })
            })
        })
}

fn is_digit_word(word: &str) -> bool {
    matches!(
        word,
        "1" | "2" | "3" | "4" | "5" | "один" | "раз" | "два" | "три" | "четыре" | "пять"
    )
}

fn file_name(path: &PathBuf) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[test]
fn confirmed_text_only_grows_by_prefix() {
    let Some(mut fixture) = fixture() else {
        return;
    };
    for path in fixture.recordings.clone() {
        let pcm = load_audio_file(&path).expect("decoding");
        let (updates, final_text) = fixture.run_loop(pcm);
        let mut previous = String::new();
        for update in &updates {
            assert!(
                update.confirmed.starts_with(&previous),
                "{}: confirmed text changed from {previous:?} to {:?}",
                file_name(&path),
                update.confirmed
            );
            previous = update.confirmed.clone();
        }
        assert!(
            final_text.starts_with(&previous),
            "{}: final text {final_text:?} does not extend the last confirmed text {previous:?}",
            file_name(&path)
        );
    }
}

#[test]
fn final_text_matches_whole_file_recognition() {
    let Some(mut fixture) = fixture() else {
        return;
    };
    for path in fixture.recordings.clone() {
        let pcm = load_audio_file(&path).expect("decoding");
        let whole = fixture.transcribe_whole(&pcm);
        let (_, final_text) = fixture.run_loop(pcm);
        let name = file_name(&path);
        assert_same_speech(&name, &final_text, &whole);
        let expected = EXPECTED_WORDS
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, words)| *words)
            .unwrap_or_default();
        let final_words = words(&final_text);
        for word in expected {
            assert!(
                final_words.iter().any(|candidate| candidate == word),
                "{name}: {word:?} missing from {final_text:?}"
            );
        }
    }
}

#[test]
fn digits_with_trailing_silence_yield_only_the_digits() {
    let Some(mut fixture) = fixture() else {
        return;
    };
    let Some(path) = fixture
        .recordings
        .iter()
        .find(|path| file_name(path) == DIGITS_FIXTURE)
        .cloned()
    else {
        eprintln!("skipping: {DIGITS_FIXTURE} is not among the recordings");
        return;
    };
    let pcm = load_audio_file(&path).expect("decoding");
    let (updates, final_text) = fixture.run_loop(pcm);

    for update in &updates {
        assert!(
            !has_repeated_ngram(&update.pending),
            "Pending Text contains a Decoder Loop: {:?}",
            update.pending
        );
        assert!(
            !has_repeated_ngram(&update.confirmed),
            "Confirmed Text contains a Decoder Loop: {:?}",
            update.confirmed
        );
    }
    assert!(!has_repeated_ngram(&final_text), "{final_text:?}");
    let final_words = words(&final_text);
    let last_digit = final_words
        .iter()
        .rposition(|word| is_digit_word(word))
        .unwrap_or_else(|| panic!("no digits recognized in {final_text:?}"));
    assert_eq!(
        last_digit + 1,
        final_words.len(),
        "text after the last digit in {final_text:?}"
    );
    assert!(
        final_words.iter().any(|word| word == "5" || word == "пять"),
        "the last digit is missing from {final_text:?}"
    );
}

#[test]
fn repeated_ngram_detector_matches_loops_only() {
    assert!(has_repeated_ngram(
        "раз, git work tree, git work tree, git work tree"
    ));
    assert!(has_repeated_ngram("да да да"));
    assert!(!has_repeated_ngram("это очень очень важно"));
    assert!(!has_repeated_ngram("нет, нет, нет, я имею в виду другое"));
    assert!(!has_repeated_ngram("1, 2, 3, 4, 5"));
    assert!(!has_repeated_ngram(""));
}

fn longest_recording(fixture: &Fixture) -> PathBuf {
    fixture
        .recordings
        .iter()
        .max_by_key(|path| {
            std::fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
        })
        .cloned()
        .expect("at least one recording")
}

/// The source ends in the middle of a phrase: whatever was not confirmed yet
/// must still come out through the stop path.
#[test]
fn source_ending_mid_file_keeps_the_tail() {
    let Some(mut fixture) = fixture() else {
        return;
    };
    let path = longest_recording(&fixture);
    let pcm = load_audio_file(&path).expect("decoding");
    let mut cut = pcm;
    cut.truncate(cut.len() * 6 / 10);

    let whole = fixture.transcribe_whole(&cut);
    let (updates, final_text) = fixture.run_loop(cut);
    let name = file_name(&path);
    assert_same_speech(&name, &final_text, &whole);
    let before_stop = updates
        .iter()
        .rev()
        .nth(1)
        .map(|update| update.confirmed.clone())
        .unwrap_or_default();
    assert!(
        final_text.len() > before_stop.len(),
        "{name}: stopping added nothing to the confirmed text {before_stop:?}"
    );
}

/// `finish` is called while the loop is still running, as when the user
/// stops dictating: the text recognized after the stop must extend what was
/// confirmed before it.
#[test]
fn stopping_mid_file_keeps_the_tail() {
    let Some(mut fixture) = fixture() else {
        return;
    };
    let path = longest_recording(&fixture);
    let pcm = load_audio_file(&path).expect("decoding");
    let transcriber = fixture.take_engine();
    let (live, mut events) =
        LiveDictation::start_from_pcm(transcriber, pcm, String::new()).expect("starting");
    let confirmed_before_stop = futures::executor::block_on(async {
        while let Some(event) = events.next().await {
            if let DictationEvent::Update(update) = event {
                if !update.confirmed.is_empty() {
                    return update.confirmed;
                }
            }
        }
        panic!("the loop ended without confirming anything");
    });
    let (transcriber, final_text) = live.finish().expect("finishing");
    fixture.return_engine(transcriber);
    let name = file_name(&path);
    assert!(
        final_text.starts_with(&confirmed_before_stop),
        "{name}: final text {final_text:?} does not extend {confirmed_before_stop:?}"
    );
    assert!(
        final_text.len() > confirmed_before_stop.len(),
        "{name}: stopping added nothing after {confirmed_before_stop:?}"
    );
}
