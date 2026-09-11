import sys

def patch(path, pairs):
    s = open(path, encoding='utf-8').read()
    for old, new in pairs:
        if old not in s:
            print('NOT FOUND in ' + path + ':\n' + old[:400]); sys.exit(1)
        s = s.replace(old, new, 1)
    open(path, 'w', encoding='utf-8').write(s)

# ---- session audio: Play hidden when Session Audio is off ----
patch('crates/dictation/src/session_audio.rs', [
('''    /// The Session Audio of `block_id`, if it is still there.
    pub fn existing(&self, block_id: &str) -> Option<PathBuf> {
        self.path_for(block_id).ok().filter(|path| path.is_file())
    }
''', '''    /// The Session Audio of `block_id`, if Session Audio is on and the file
    /// is still there. With `keep = 0` a file left over from earlier is not
    /// offered either: the user turned the feature off.
    pub fn existing(&self, block_id: &str) -> Option<PathBuf> {
        if !self.is_enabled() {
            return None;
        }
        self.path_for(block_id).ok().filter(|path| path.is_file())
    }
'''),
('''    #[test]
    fn keep_zero_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionAudioStore::new(dir.path().join("audio"), 0);

        assert!(!store.is_enabled());
        assert_eq!(store.append("block", &tone(1)).unwrap(), None);
        assert!(!dir.path().join("audio").exists());
    }
''', '''    #[test]
    fn keep_zero_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionAudioStore::new(dir.path().join("audio"), 0);

        assert!(!store.is_enabled());
        assert_eq!(store.append("block", &tone(1)).unwrap(), None);
        assert!(!dir.path().join("audio").exists());
    }

    #[test]
    fn keep_zero_offers_no_file_even_when_one_is_left_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = SessionAudioStore::new(dir.path().to_path_buf(), 5)
            .append("block", &tone(1))
            .unwrap()
            .unwrap();
        assert!(path.is_file());

        let disabled = SessionAudioStore::new(dir.path().to_path_buf(), 0);
        assert_eq!(disabled.existing("block"), None);
    }
'''),
])

# ---- decoder loop semantics ----
patch('crates/dictation/src/dictation.rs', [
('''/// Whether `text` is one n-gram repeated `LOOP_REPEATS` or more times in a
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
''', '''/// Whether `words` contain a run of one n-gram of at least `min_n` words
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
'''),
('''/// Removes Decoder Loops from a recognized buffer: segments that loop inside
/// themselves, and runs of `LOOP_REPEATS` or more consecutive segments that
/// say the same thing, which is how the same loop looks when Whisper splits
/// it at timestamps.
fn drop_decoder_loops(segments: Vec<Segment>) -> Vec<Segment> {''', '''/// Removes Decoder Loops from a recognized buffer: segments that loop inside
/// themselves, and runs of `LOOP_REPEATS` or more consecutive segments that
/// repeat the same phrase of two or more words, which is how the same loop
/// looks when Whisper splits it at timestamps. Single-word segments are
/// left alone for the same reason a single repeated word is.
fn drop_decoder_loops(segments: Vec<Segment>) -> Vec<Segment> {'''),
('''        if end - start >= LOOP_REPEATS && !words[start].is_empty() {
            keep[start..end].fill(false);
        }''', '''        if end - start >= LOOP_REPEATS && words[start].len() >= 2 {
            keep[start..end].fill(false);
        }'''),
('''            let start = duration_to_samples(last.start).min(pcm.len());
            let end = duration_to_samples(last.end + SETTLE).clamp(start, pcm.len());
            let alone = self.segments_after(&pcm[start..end], preceding_text, true)?;
            if alone.iter().all(|segment| segment.text.is_empty()) {''', '''            let start = duration_to_samples(last.start).min(pcm.len());
            let end = duration_to_samples(last.end + SETTLE).clamp(start, pcm.len());
            if end - start < duration_to_samples(BOUNDARY_FRAME) {
                // Timestamps outside the audio give the decoder nothing to
                // judge; without a verdict the words stay.
                break;
            }
            let alone = self.segments_after(&pcm[start..end], preceding_text, true)?;
            if alone.iter().all(|segment| segment.text.is_empty()) {'''),
('''        assert!(is_decoder_loop(
            "Продолжение следует. Продолжение следует. Продолжение следует."
        ));
    }
''', '''        assert!(is_decoder_loop(
            "Продолжение следует. Продолжение следует. Продолжение следует."
        ));
        assert!(is_decoder_loop(
            "Раз, два, git work tree, git work tree, git work tree."
        ));
    }
'''),
('''        assert!(!is_decoder_loop("нет, нет, нет, я имею в виду другое"));
    }
''', '''        assert!(!is_decoder_loop("нет, нет, нет, я имею в виду другое"));
        assert!(!is_decoder_loop("git work tree, git work tree, и всё"));
    }
'''),
('''            segment("четыре"),
            segment("четыре"),
        ]);
        let texts: Vec<&str> = kept.iter().map(|segment| segment.text.as_str()).collect();
        assert_eq!(texts, vec!["Раз, два, три.", "четыре", "четыре"]);
    }
''', '''            segment("четыре"),
            segment("четыре"),
        ]);
        let texts: Vec<&str> = kept.iter().map(|segment| segment.text.as_str()).collect();
        assert_eq!(texts, vec!["Раз, два, три.", "четыре", "четыре"]);

        let single_words = drop_decoder_loops(vec![
            segment("да."),
            segment("Да."),
            segment("да"),
        ]);
        assert_eq!(single_words.len(), 3, "single-word segments are speech");
    }
'''),
])

patch('crates/dictation/tests/recognition_loop.rs', [
('''/// Whether any n-gram is repeated three or more times in a row anywhere in
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
''', '''/// The shape of a Decoder Loop, whatever the words: the whole text is one
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
'''),
('''    assert!(has_repeated_ngram("да да да"));
    assert!(!has_repeated_ngram("это очень очень важно"));''', '''    assert!(has_repeated_ngram("да да да"));
    assert!(!has_repeated_ngram("это очень очень важно"));
    assert!(!has_repeated_ngram("нет, нет, нет, я имею в виду другое"));'''),
])

# ---- device display name: friendly name first, short name as fallback ----
patch('crates/audio/src/audio_pipeline.rs', [
('''/// Local: the name a device is shown under everywhere in Zed. WASAPI reports
/// the short device description ("Microphone") as the name and puts the
/// friendly name Windows shows ("Microphone (fifine Microphone)") into the
/// extended lines, so the extended line that spells out the name is
/// preferred; without one the short name is used.
pub fn device_display_name(description: &DeviceDescription) -> String {
    let name = description.name();
    description
        .extended()
        .iter()
        .find(|line| {
            let line = line.trim();
            !line.is_empty() && line.to_lowercase().contains(&name.to_lowercase())
        })
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| name.to_string())
}
''', '''/// Local: the name a device is shown under everywhere in Zed. WASAPI reports
/// the short device description ("Microphone") as the name and puts the
/// friendly name Windows shows ("Microphone (fifine Microphone)") into the
/// extended lines, so the first extended line is preferred; without one the
/// short name is used.
pub fn device_display_name(description: &DeviceDescription) -> String {
    description
        .extended()
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| description.name())
        .to_string()
}
'''),
('''    fn without_a_friendly_name_the_short_name_is_used() {
        let description = DeviceDescriptionBuilder::new("Headset Microphone").build();
        assert_eq!(device_display_name(&description), "Headset Microphone");

        let unrelated = DeviceDescriptionBuilder::new("Microphone")
            .add_extended_line("USB Audio Class 2.0")
            .build();
        assert_eq!(device_display_name(&unrelated), "Microphone");
    }''', '''    fn without_a_friendly_name_the_short_name_is_used() {
        let description = DeviceDescriptionBuilder::new("Headset Microphone").build();
        assert_eq!(device_display_name(&description), "Headset Microphone");

        let blank = DeviceDescriptionBuilder::new("Microphone")
            .add_extended_line("   ")
            .build();
        assert_eq!(device_display_name(&blank), "Microphone");
    }'''),
])

# ---- tooltip quotes the prompt that was actually used ----
patch('crates/agent_ui/src/dictation_window.rs', [
('''    processed: Option<String>,
    processed_by: Option<ProcessedBy>,''', '''    processed: Option<String>,
    processed_by: Option<ProcessedBy>,
    /// The prompt template Post-processing ran with, for the label tooltip.
    processed_with_prompt: Option<String>,'''),
('''            processed: None,
            processed_by: None,
            post_processing_error: None,
            resume_error: None,
            playback_error: None,''', '''            processed: None,
            processed_by: None,
            processed_with_prompt: None,
            post_processing_error: None,
            resume_error: None,
            playback_error: None,'''),
('''        let model = post_processing_model(settings, cx);
        self.phase = Phase::Review;
        self.focus_review_editor(window, cx);
        cx.notify();
''', '''        let model = post_processing_model(settings, cx);
        self.processed_with_prompt = Some(settings.post_processing_prompt.clone());
        self.phase = Phase::Review;
        self.focus_review_editor(window, cx);
        cx.notify();
'''),
('''        let settings = &AgentSettings::get_global(cx).dictation;
        let prompt = prompt_preview(&settings.post_processing_prompt);
        let (text, tooltip_title, tooltip_meta): (SharedString, SharedString, SharedString) =
            match label {
                FooterLabel::Raw => (
                    "Raw".into(),
                    "Raw transcript from the Transcription Engine".into(),
                    format!("Post-processing prompt:\\n{prompt}").into(),
                ),
                FooterLabel::Processed(ProcessedBy { provider, model }) => (
                    format!("Processed · {model}").into(),
                    format!("Rewritten by {provider} · {model}").into(),
                    format!("Prompt:\\n{prompt}").into(),
                ),
                FooterLabel::Microphone(_) => return None,
            };''', '''        let current_prompt = &AgentSettings::get_global(cx).dictation.post_processing_prompt;
        let (text, tooltip_title, tooltip_meta): (SharedString, SharedString, SharedString) =
            match label {
                FooterLabel::Raw => (
                    "Raw".into(),
                    "Raw transcript from the Transcription Engine".into(),
                    format!(
                        "Post-processing prompt:\\n{}",
                        prompt_preview(current_prompt)
                    )
                    .into(),
                ),
                FooterLabel::Processed(ProcessedBy { provider, model }) => {
                    let prompt = self
                        .processed_with_prompt
                        .as_deref()
                        .unwrap_or(current_prompt);
                    (
                        format!("Processed · {model}").into(),
                        format!("Rewritten by {provider} · {model}").into(),
                        format!("Prompt:\\n{}", prompt_preview(prompt)).into(),
                    )
                }
                FooterLabel::Microphone(_) => return None,
            };'''),
])
print('ok')
