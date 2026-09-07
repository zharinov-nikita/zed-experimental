# Voice Dictation, итерация 2: секция, стабильное распознавание, настройки

Метка: `ready-for-agent`
Словарь: `CONTEXT.md` в корне репозитория. Термины ниже (Dictation Session, Dictation Window, Commit, Dictation Block, Live Transcript, Confirmed Text, Pending Text, Model Loading, Post-processing, Recognizer Artifact, Glossary, Composer) используются строго в его значениях.
Первичные источники: прототип `prototypes/voice-dictation/index.html` (вариант C «Section над композером»), `prototypes/voice-dictation/DECISION.md`, `LOCAL_DEV.md` (раздел про диктовку).

## Problem Statement

Первая итерация диктовки работает, но пользоваться ей неудобно:

1. После нажатия `ctrl-alt-space` запись начинается с задержкой в несколько секунд, а пользователь не видит, что происходит. Начало речи теряется. В середине речи пропадают слова и целые предложения: тихие фразы считаются тишиной, края фраз режутся, а фильтр Recognizer Artifact удаляет любое предложение со словами «редактор», «реклама», «корректор».
2. Подсказки клавиш в подвале Dictation Window занимают слишком много места: на Windows `ctrl-alt-space` рисуется тремя клавишами, и таких подсказок две.
3. Dictation Window плавает над Composer у курсора. Пользователь хочет, чтобы она разворачивалась как секция над Composer, как в варианте C прототипа.
4. Все настройки диктовки правятся только в `settings.json`. Пользователь хочет менять их в окне Settings: модель, язык, глоссарий, микрофон, промпт и модель Post-processing.

## Solution

1. Dictation Session стартует только после Model Loading, и Model Loading видна: в секции написано «Loading Whisper model…», таймер и индикатор записи появляются, когда модель готова. Модель остаётся в памяти до закрытия Zed (настраивается). Одна Dictation Session на процесс: попытка начать вторую в другом окне показывает сообщение и ничего не пишет. Цикл распознавания переписан по схеме whisper.cpp stream: раз в секунду распознаётся накопленный буфер, Confirmed Text растёт по временным меткам сегментов, Pending Text это последний сегмент. Порог громкости и фильтр Recognizer Artifact удалены; артефакты убирает Post-processing. Есть настройка сохранения WAV последней сессии для диагностики.
2. В подвале остаются только короткие подсказки: во время записи «esc Review», в просмотре «enter Accept», «tab Raw / Processed», «esc Cancel». Клавиши уменьшенного размера. Старт, остановка и Resume по `ctrl-alt-space` описаны в тултипе кнопки микрофона.
3. Dictation Window встраивается в панель агента над Composer на всю ширину, сдвигая Composer вниз. Высота растёт до 10 строк текста, дальше прокрутка, одинаково для записи и просмотра. Плавающего позиционирования больше нет.
4. В окне Settings на странице «AI» в секции «General» появляется ссылка на подстраницу «Dictation» со всеми настройками диктовки. Значения хранятся в `settings.json` под `agent.dictation` (микрофон под `audio.experimental.input_audio_device`), подстраница только редактирует их. Промпт Post-processing по умолчанию короткий и редактируемый.

## User Stories

1. As a Zed user, I want to see «Loading Whisper model…» when I start a Dictation Session for the first time, so that I know why nothing is recorded yet and can wait.
2. As a Zed user, I want the recording indicator and timer to appear only once the model is loaded, so that I know exactly when speaking starts to count.
3. As a Zed user, I want the Whisper model to stay loaded until I close Zed, so that every next Dictation Session starts instantly.
4. As a Zed user, I want a «Keep model loaded» setting, so that I can free VRAM after each session if I need it for something else.
5. As a Zed user, I want a start attempt in a second window to show «Dictation is already running in another window», so that two Dictation Sessions never run at once and a second model copy is never loaded.
6. As a Zed user, I want quiet speech to be recognized, so that I do not have to raise my voice for the microphone.
7. As a Zed user, I want words at the beginning and end of phrases to be kept, so that the Live Transcript does not lose syllables at pauses.
8. As a Zed user, I want Confirmed Text to grow steadily while I speak and Pending Text to be only the last phrase, so that I can follow the recognition in real time.
9. As a Zed user, I want sentences containing ordinary words like «редактор» to be kept, so that the recognizer never silently drops legitimate speech.
10. As a Zed user, I want Recognizer Artifacts such as «Продолжение следует» to be removed by Post-processing, so that they do not end up in my prompt.
11. As a Zed user, I want to see the raw transcript (with artifacts) via the «Raw» toggle, so that I can check what was actually recognized.
12. As a Zed user, I want a «Save last recording» setting that keeps the WAV of the last Dictation Session, so that I can reproduce recognition problems offline.
13. As a Zed user, I want the saved WAV to be playable by the `transcribe_wav` example, so that diagnosis needs no extra tools.
14. As a Zed user, I want the Dictation Window to unfold above the Composer, so that it never covers the text I am editing.
15. As a Zed user, I want the Dictation Window to span the full Composer width, so that long transcripts wrap naturally.
16. As a Zed user, I want the Dictation Window to grow with the text up to 10 lines and then scroll, so that a long dictation never pushes the Composer off screen.
17. As a Zed user, I want the recording and review states to have the same height rules, so that the panel does not jump when I stop.
18. As a Zed user, I want the Composer to stay editable while I dictate, so that I can type while speaking, with Enter still sending as usual.
19. As a Zed user, I want only short key hints in the footer, so that the footer fits in one row even in a narrow panel.
20. As a Zed user, I want the key hints to be clickable, so that I can use the mouse instead of the keyboard.
21. As a Zed user, I want the microphone button tooltip to name the hotkey for start, stop and Resume, so that I can learn it without hints in the window.
22. As a Zed user, I want to press Enter in review to Accept and Shift-Enter to insert a newline, so that editing the transcript feels like editing any text.
23. As a Zed user, I want a «Dictation» sub-page under Settings > AI, so that all dictation settings live in one place.
24. As a Zed user, I want to set the Whisper model path and the backends folder as text fields on the sub-page, so that I do not have to edit JSON.
25. As a Zed user, I want to pick the language from the full list of languages Whisper supports, so that I never mistype a language code.
26. As a Zed user, I want the default language to be English, so that a fresh install behaves like other English-first Zed features.
27. As a Zed user, I want my own configuration to explicitly set Russian, so that changing the default does not change my dictation.
28. As a Zed user, I want to add and remove Glossary terms one per row, so that the list stays readable.
29. As a Zed user, I want to pick the microphone on the sub-page using the same setting as the Audio page, so that there is one source of truth for the input device.
30. As a Zed user, I want the microphone I picked to actually be used for recording, so that the setting is not decorative.
31. As a Zed user, I want to toggle sounds, «Keep model loaded» and «Save last recording» on the sub-page, so that every dictation flag is discoverable.
32. As a Zed user, I want to toggle Post-processing on the sub-page, so that I can dictate raw when the model is unavailable.
33. As a Zed user, I want to pick the Post-processing provider and then a model of that provider from two dropdowns, so that I choose only among models Zed actually knows.
34. As a Zed user, I want to edit the Post-processing prompt in a multi-line editor, so that long prompts are readable.
35. As a Zed user, I want a «Reset to default» for the prompt, so that I can recover from a broken edit.
36. As a Zed user, I want the default prompt to be short and to mention `${glossary}` and `${output}`, so that I understand the template at a glance.
37. As a Zed user, I want every sub-page field to write to `settings.json`, so that my settings survive and stay diffable.
38. As a Zed user, I want the hotkey to stay in the keymap, so that key configuration works the way it does everywhere in Zed.
39. As a Zed user, I want an engine error (missing model, bad backends folder, microphone busy) to appear inside the Dictation Window as a Callout, so that I learn what to fix.
40. As a Zed user, I want a Post-processing failure to still let me Accept the raw text, so that a model outage never loses a dictation.
41. As a Zed user, I want Resume of a Dictation Block to reuse the loaded model, so that continuing a block is as fast as starting one.
42. As a Zed user, I want stopping the session to recognize the last unconfirmed tail before review opens, so that the final words are never lost.
43. As a fork maintainer, I want the recognition loop to accept a pre-recorded PCM source, so that recognition quality can be tested on real recordings without a microphone.
44. As a fork maintainer, I want tests to skip cleanly when the model path environment variable is unset, so that CI-less machines still run the rest of the suite.

## Implementation Decisions

### Recognition loop (crate `dictation`)

- The audio source behind `LiveDictation` becomes an abstraction with two implementations: the live microphone and a pre-loaded PCM buffer that is fed in real time or faster. The loop code is the same for both.
- Schema whisper.cpp stream. Every second the loop transcribes the audio accumulated since the last commit point (capped at the Whisper 30 s window). Whisper segment timestamps decide what becomes Confirmed Text: every segment except the last whose end lies at least about one second before the buffer end is committed and the commit point moves to its end. The last segment is Pending Text. When the buffer approaches the window cap, everything except the last segment is committed regardless.
- No energy threshold, no silence trimming, no phrase blacklist. `no_speech_thold` and temperature settings stay as decoder hygiene, nothing text-level is filtered by the engine.
- The stop path transcribes the remaining tail once and appends it to Confirmed Text.
- Recording starts only after Model Loading finishes. `Recorder` is opened after the transcriber is acquired, not in parallel.
- The recorder honours the configured input device: the dead code path for device selection in the audio pipeline is completed so that `audio.experimental.input_audio_device` is used.
- Diagnostics: when `save_last_recording` is on, the full session PCM is written as 16 kHz mono WAV to a fixed file in the OS temp directory (one file, overwritten each session). Path is logged at info level.

### Engine lifetime and single session (crate `agent_ui`)

- Engine cache stays process-global. A process-global «session active» flag is added next to it. Starting a Dictation Session while the flag is set fails immediately with the message «Dictation is already running in another window», shown in the Dictation Window as a Callout, without touching the cache.
- `keep_model_loaded` (default true): when false, the transcriber is dropped instead of returned to the cache after the session ends. When true, it stays until process exit.
- Phase `Starting` renders «Loading Whisper model…» in the body and an empty footer (no timer, no dot, no hints except «esc Cancel»).

### Dictation Window as a section (crate `agent_ui`)

- The window is rendered as a child of the thread view directly above the Composer, full width, with the panel's border and background per the prototype's variant C. The deferred/anchored overlay and the cursor position helper are removed.
- Body height: recording body and review editor both cap at 10 lines of the buffer font and scroll beyond that. Review keeps `Editor::auto_height` with the same cap.
- Footer stays one row: timer left, hints right. Hints use small `KeyBinding` size. The two `ctrl-alt-space` hints are removed; the microphone button tooltip lists «Start / Stop dictation» and «Resume selected block» with the key binding.
- Focus rules unchanged: Composer keeps focus during recording, review editor takes focus in review.

### Settings model (crates `settings_content`, `agent_settings`, default settings)

- `agent.dictation` gains `keep_model_loaded: bool` (default true) and `save_last_recording: bool` (default false).
- `language` becomes an enum of all Whisper languages plus `auto`, serialized as the ISO code Whisper expects (`en`, `ru`, …). Default `en`. The enum derives what the settings UI dropdown needs (strum variant array and names) and displays English language names.
- Default `post_processing.prompt` is replaced with the short prompt below. `${glossary}` and `${output}` semantics unchanged.

  ```
  Clean up this dictated text. Fix punctuation and casing, remove filler words and repeated words, remove Whisper artifacts like "Продолжение следует" or "Subtitles by". Keep the language and the meaning. Spell these terms exactly as given: ${glossary}. Output only the cleaned text.

  ${output}
  ```

- The user's `settings.json` gets an explicit `"language": "ru"` under `agent.dictation` as part of this work (this machine only).
- Microphone is not a dictation setting: the sub-page shows the existing `audio.experimental.input_audio_device` field.

### Settings sub-page (crate `settings_ui`)

- A `SubPageLink` «Dictation» in the «General» section of the «AI» page, next to «LLM Providers», with search aliases (dictation, voice, whisper, microphone, speech).
- The sub-page is a custom render function in the style of the existing Sandbox and LLM Providers sub-pages, and contains, in this order: Whisper model path (text), backends folder (text), language (dropdown), microphone (existing input device dropdown), Glossary (one term per row, add input and remove buttons, like the Sandbox path list), sounds (toggle), keep model loaded (toggle), save last recording (toggle), Post-processing enabled (toggle), Post-processing provider (dropdown over providers in the language model registry) and model (dropdown over models of the selected provider), Post-processing prompt (multi-line editor with soft wrap like the Skill Creator editor, plus «Reset to default»).
- All fields read and write through the standard settings file update path so that `settings.json` remains the single store. The provider/model pair writes a `LanguageModelSelection`; clearing the provider removes the selection so the default agent model is used again.

### Out of the engine, into Post-processing

- The Post-processing prompt is the only place Recognizer Artifacts are handled. If Post-processing is disabled or fails, raw text is accepted as-is with the existing warning Callout.

## Testing Decisions

- Good tests check external behaviour through the crate's public API: given audio, which text and in which order of confirmation comes out; given settings JSON, which resolved values; given a URI, does it round-trip. No tests of loop internals or timing constants.
- `dictation` crate, integration tests on the Handy recordings already used with the `transcribe_wav` example: Confirmed Text is a prefix-monotonic sequence across updates, the final text contains the expected key words, the final text equals the text obtained by a single whole-file transcription up to punctuation and casing differences, and stopping mid-file still yields the tail. Tests run only when the model path environment variable used by the example is set, otherwise they return early. Prior art: `examples/transcribe_wav.rs` in the same crate.
- `agent_settings`: unit tests for parsing `agent.dictation` with new fields, the language enum round-trip through JSON, and the default prompt containing both placeholders. Prior art: existing tests at the bottom of the agent settings module.
- `acp_thread`: the existing Dictation `MentionUri` round-trip test remains the seam for the Dictation Block link.
- Not covered by automated tests, verified by hand in the dev build with the isolated data dir: section layout above the Composer, footer hints, Settings sub-page widgets, second-window refusal, microphone selection.

## Out of Scope

- Quote Reply (separate feature, glossary terms already reserved).
- Sounds playback (the setting exists, nothing plays).
- Output audio device on the sub-page.
- Automatic model download; model path is a manual setting.
- Silero or other dedicated VAD.
- Dictation Blocks surviving a Zed restart.
- Pre-warming the model before the first Dictation Session.
- Ollama keep-alive tuning for the Post-processing model.
- Upstream contribution, keymap changes beyond what exists.

## Further Notes

- Build hygiene from `LOCAL_DEV.md` applies: close the dev `zed.exe` before linking, run with `--user-data-dir %LOCALAPPDATA%\Zed-Local\dictation`, keymap or asset changes trigger a long rebuild.
- Whisper Large v3 q5_0 takes about 3.5 s to load on Vulkan on this machine and about 1.1 GB of VRAM; the Zed Preview process and the dev build each hold their own copy, that is expected.
- The window type keeps its name `DictationWindow` in code even though it is rendered as a section; renaming it is not worth the diff.
