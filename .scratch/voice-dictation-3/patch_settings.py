import sys

def patch(path, pairs):
    s = open(path, encoding='utf-8').read()
    for old, new in pairs:
        if old not in s:
            print('NOT FOUND in ' + path + ':\n' + old[:300])
            sys.exit(1)
        s = s.replace(old, new, 1)
    open(path, 'w', encoding='utf-8').write(s)

# --- settings_content ---
patch('crates/settings_content/src/agent.rs', [
('''    /// Save the audio of the last dictation session as a WAV file in the
    /// temporary directory for diagnosing recognition problems.
    ///
    /// Default: false
    pub save_last_recording: Option<bool>,
    pub post_processing: Option<DictationPostProcessingSettingsContent>,
}
''', '''    /// Session Audio: the sound of every dictation session, kept locally so
    /// it can be replayed or used to reproduce a recognition problem.
    pub session_audio: Option<DictationSessionAudioSettingsContent>,
    pub post_processing: Option<DictationPostProcessingSettingsContent>,
}

/// Local: Session Audio retention.
#[with_fallible_options]
#[derive(Clone, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom, Debug, Default)]
pub struct DictationSessionAudioSettingsContent {
    /// How many dictation sessions keep their audio (16 kHz mono WAV in the
    /// `dictation/audio` folder of the Zed data directory). The oldest files
    /// are deleted first; `0` turns Session Audio off.
    ///
    /// Default: 20
    pub keep: Option<u32>,
}
'''),
])

# --- agent_settings ---
patch('crates/agent_settings/src/agent_settings.rs', [
('''    pub keep_model_loaded: bool,
    pub save_last_recording: bool,
    pub post_processing_enabled: bool,''', '''    pub keep_model_loaded: bool,
    /// How many sessions keep their Session Audio; `0` turns it off.
    pub session_audio_keep: u32,
    pub post_processing_enabled: bool,'''),
('''        let dictation = agent.dictation.clone().unwrap_or_default();
        let post_processing = dictation.post_processing.clone().unwrap_or_default();
''', '''        let dictation = agent.dictation.clone().unwrap_or_default();
        let post_processing = dictation.post_processing.clone().unwrap_or_default();
        let session_audio = dictation.session_audio.clone().unwrap_or_default();
'''),
('''                keep_model_loaded: dictation.keep_model_loaded.unwrap_or(true),
                save_last_recording: dictation.save_last_recording.unwrap_or(false),
''', '''                keep_model_loaded: dictation.keep_model_loaded.unwrap_or(true),
                session_audio_keep: session_audio.keep.unwrap_or(20),
'''),
('''        assert!(dictation.keep_model_loaded);
        assert!(!dictation.save_last_recording);
        assert!(
            dictation.post_processing_prompt.contains("${glossary}"),
            "default prompt should mention the glossary placeholder"
        );
        assert!(
            dictation.post_processing_prompt.contains("${output}"),
            "default prompt should mention the output placeholder"
        );
''', '''        assert!(dictation.keep_model_loaded);
        assert_eq!(dictation.session_audio_keep, 20);
        assert!(
            dictation.post_processing_prompt.contains("${glossary}"),
            "default prompt should mention the glossary placeholder"
        );
        assert!(
            dictation.post_processing_prompt.contains("${output}"),
            "default prompt should mention the output placeholder"
        );
        assert!(
            dictation.post_processing_prompt.contains("Thank you"),
            "default prompt should list \\"Thank you\\" among the artifact examples"
        );
'''),
('''                            "dictation": {
                                "language": "ru",
                                "keep_model_loaded": false,
                                "save_last_recording": true
''', '''                            "dictation": {
                                "language": "ru",
                                "keep_model_loaded": false,
                                "session_audio": { "keep": 5 }
'''),
('''        assert_eq!(dictation.language.whisper_code(), Some("ru"));
        assert!(!dictation.keep_model_loaded);
        assert!(dictation.save_last_recording);
''', '''        assert_eq!(dictation.language.whisper_code(), Some("ru"));
        assert!(!dictation.keep_model_loaded);
        assert_eq!(dictation.session_audio_keep, 5);
'''),
('''                    r#"{ "agent": { "dictation": { "language": "klingon", "save_last_recording": true } } }"#,
''', '''                    r#"{ "agent": { "dictation": { "language": "klingon", "session_audio": { "keep": 0 } } } }"#,
'''),
('''        let dictation = AgentSettings::get_global(cx).dictation.clone();
        assert_eq!(dictation.language, DictationLanguage::English);
        assert!(dictation.save_last_recording);
    }
''', '''        let dictation = AgentSettings::get_global(cx).dictation.clone();
        assert_eq!(dictation.language, DictationLanguage::English);
        assert_eq!(dictation.session_audio_keep, 0);
    }

    #[gpui::test]
    fn test_dictation_old_save_last_recording_key_is_ignored(cx: &mut gpui::App) {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        project::DisableAiSettings::register(cx);
        AgentSettings::register(cx);

        SettingsStore::update_global(cx, |store, cx| {
            store
                .set_user_settings(
                    r#"{ "agent": { "dictation": { "save_last_recording": true, "language": "ru" } } }"#,
                    cx,
                )
                .result();
        });
        let dictation = AgentSettings::get_global(cx).dictation.clone();
        assert_eq!(dictation.language, DictationLanguage::Russian);
        assert_eq!(dictation.session_audio_keep, 20);
    }
'''),
])

# --- default.json ---
patch('assets/settings/default.json', [
('''      // Save the audio of the last session as a WAV file in the temp directory for diagnostics.
      "save_last_recording": false,
''', '''      // Session Audio: the sound of every dictation session, kept as 16 kHz mono WAV files in
      // the `dictation/audio` folder of the Zed data directory for replay and diagnostics.
      "session_audio": {
        // How many sessions keep their audio; the oldest files are deleted first. 0 turns it off.
        "keep": 20,
      },
'''),
('''remove Whisper artifacts like \\"Продолжение следует\\" or \\"Subtitles by\\". Keep''',
 '''remove Whisper artifacts like \\"Продолжение следует\\", \\"Subtitles by\\" or \\"Thank you\\". Keep'''),
])

print('ok')
