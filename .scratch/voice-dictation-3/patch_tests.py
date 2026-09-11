import sys

def patch(path, pairs):
    s = open(path, encoding='utf-8').read()
    for old, new in pairs:
        if old not in s:
            print('NOT FOUND in ' + path + ':\n' + old[:300])
            sys.exit(1)
        s = s.replace(old, new, 1)
    open(path, 'w', encoding='utf-8').write(s)

patch('crates/dictation/src/dictation.rs', [
('''    pub fn segments(&mut self, pcm: &[f32]) -> Result<Vec<Segment>> {
        self.segments_after(pcm, "", false)
    }
''', '''    pub fn segments(&mut self, pcm: &[f32]) -> Result<Vec<Segment>> {
        Ok(drop_decoder_loops(self.segments_after(pcm, "", false)?))
    }
'''),
])

patch('crates/dictation/tests/recognition_loop.rs', [
('''/// One model per test binary: loading takes seconds and a gigabyte of VRAM.''',
'''/// Spoken digits followed by silence, synthesized with the Windows speech
/// engine (see `LOCAL_DEV.md`). Whisper tends to loop on the silent tail and
/// to invent a closing phrase on stop; neither may reach the text.
const DIGITS_FIXTURE: &str = "zed-digits-with-silence.wav";

/// One model per test binary: loading takes seconds and a gigabyte of VRAM.'''),
('''fn file_name(path: &PathBuf) -> String {''',
'''/// Whether any n-gram is repeated three or more times in a row anywhere in
/// `text`: the shape of a Decoder Loop, whatever the words.
fn has_repeated_ngram(text: &str) -> bool {
    let words = words(text);
    (1..=words.len() / 3).any(|n| {
        (0..words.len().saturating_sub(3 * n - 1)).any(|start| {
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

fn file_name(path: &PathBuf) -> String {'''),
('''fn longest_recording(fixture: &Fixture) -> PathBuf {''',
'''#[test]
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
    assert!(has_repeated_ngram("раз, git work tree, git work tree, git work tree"));
    assert!(has_repeated_ngram("да да да"));
    assert!(!has_repeated_ngram("это очень очень важно"));
    assert!(!has_repeated_ngram("1, 2, 3, 4, 5"));
    assert!(!has_repeated_ngram(""));
}

fn longest_recording(fixture: &Fixture) -> PathBuf {'''),
])
print('ok')
