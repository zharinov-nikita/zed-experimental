# Voice Dictation, итерация 3: устойчивость, Session Audio, устройства, Engine Download

Метка: `ready-for-agent`
Словарь: `CONTEXT.md` в корне репозитория. Термины (Dictation Session, Dictation Window, Dictation Block, Live Transcript, Confirmed Text, Pending Text, Post-processing, Recognizer Artifact, Decoder Loop, Session Audio, Glossary, Engine Assets, Engine Download, Composer, Transcription Engine) используются строго в его значениях.
Решения: `docs/adr/0001-decoder-loop-guard-stays-in-the-engine.md`.
Первичные источники: спека итерации 2 `.scratch/voice-dictation/spec.md` и её тикеты, ручная проверка dev-сборки 2026-09-05, `LOCAL_DEV.md` (раздел про диктовку).

## Problem Statement

Итерация 2 собрана и проверена руками. Диктовка работает, но пользоваться ей всё ещё неудобно, а в двух местах она ведёт себя как сломанная:

1. На почти пустом буфере Transcription Engine уходит в Decoder Loop: при диктовке «1, 2, 3, 4, 5» Pending Text заполняется бесконечным «git work tree, git work tree, …». При остановке в хвост попадает «Thank you.», и Post-processing его не убирает.
2. В Dictation Window нет scrollbar: непонятно, есть ли за нижним краем ещё текст.
3. Пока идёт Post-processing, подсказки Accept и Tab выглядят рабочими, хотя принимать ещё нечего.
4. В просмотре не видно, какой текст показан, сырой или обработанный, и какой промпт и какая модель его переписали; до промпта из окна не добраться.
5. Свою речь нельзя переслушать: WAV последней сессии лежит в temp и перезаписывается.
6. Во время записи не видно, в какой микрофон идёт запись. В настройках устройства названы «Microphone (wasapi:{…})», два микрофона неотличимы, а fifine по имени не найти.
7. Глоссарий из семнадцати терминов занимает экран строками по одному термину.
8. Проверить микрофон из настроек диктовки нельзя, хотя в Zed есть окно проверки звука.
9. Engine Assets нужно искать и скачивать руками, а потом вписывать пути.

## Solution

1. Decoder Loop и no-speech хвост отбрасываются движком по форме вывода, без словаря (ADR 0001). Промпт Post-processing получает пример «Thank you».
2. У тела Dictation Window стандартный scrollbar Zed в записи и просмотре.
3. Во время «Recognizing…» и Post-processing доступен только Cancel, Accept и Tab погашены, в подвале спиннер.
4. В подвале просмотра метка «Raw» или «Processed · <модель>»; тултип метки показывает начало промпта, клик открывает подстраницу Dictation. Подсказка Tab называется «Show Raw» / «Show Processed».
5. Session Audio каждой сессии хранится локально с лимитом по числу файлов; в просмотре есть Play.
6. Во время записи в подвале имя микрофона; устройства везде называются как в Windows; список устройств обновляется при открытии выпадающего списка и при старте сессии.
7. Glossary на подстранице показан чипами.
8. Кнопка «Test Microphone…» открывает существующее окно проверки звука.
9. Engine Download: у полей модели и бэкендов кнопка «Download…», пути подставляются сами.

## User Stories

### Decoder Loop и хвост

1. As a Zed user, I want a repeated phrase like «git work tree, git work tree» never to appear in the Live Transcript, so that a pause does not turn into garbage.
2. As a Zed user, I want the Pending Text to be empty while the engine is looping, so that the section looks like a pause and not like a bug.
3. As a Zed user, I want «Thank you.» and similar silence phrases not to be appended when I stop, so that the last words of my dictation are mine.
4. As a Zed user, I want the loop guard to work without Post-processing, so that raw dictation is usable when the model is down.
5. As a Zed user, I want the engine to keep every legitimate word, so that the guard never removes speech by its meaning.
6. As a fork maintainer, I want the boundary between engine guards and Post-processing written down, so that nobody adds a phrase blacklist to the engine again.

### Dictation Window

7. As a Zed user, I want a scrollbar in the section body, so that I can see there is more text below.
8. As a Zed user, I want the scrollbar to follow my `scrollbar.show` setting, so that it behaves like any editor.
9. As a Zed user, I want Accept and Tab to be visibly disabled while Post-processing runs, so that I do not press what cannot work.
10. As a Zed user, I want Cancel to stay available during Post-processing, so that I can leave without waiting.
11. As a Zed user, I want a spinner and «Post-processing…» in the footer, so that I know the wait is expected.
12. As a Zed user, I want the same rules during «Recognizing…», so that stopping feels consistent.
13. As a Zed user, I want a footer label «Raw» or «Processed · qwen3:14b», so that I know which text I am looking at and who rewrote it.
14. As a Zed user, I want the label to name the model actually used, including the agent default fallback, so that the label never lies.
15. As a Zed user, I want the label tooltip to show the beginning of the prompt, so that I can recall what it asks for.
16. As a Zed user, I want clicking the label to open Settings > AI > Dictation, so that I can edit the prompt in one place.
17. As a Zed user, I want the Tab hint to read «Show Raw» / «Show Processed», so that its effect is obvious.
18. As a Zed user, I want the microphone name in the footer while recording, so that I know where my voice goes.
19. As a Zed user, I want the footer to say «configured device not found» when my chosen microphone is missing, so that I learn why the default is used while recording still works.
20. As a Zed user, I want a Play hint in the review footer, so that I can hear what I said.
21. As a Zed user, I want Play to turn into Stop while playing, so that I can interrupt it.
22. As a Zed user, I want Accept, Cancel and Resume to stop playback, so that sound never outlives the section.
23. As a Zed user, I want playback on the output device from the Audio settings, so that it goes to my headphones.

### Session Audio

24. As a Zed user, I want the sound of every Dictation Session kept locally, so that I can listen to it later.
25. As a Zed user, I want the sound of a discarded session kept too, so that a bad recognition can still be reproduced.
26. As a Zed user, I want a Resume to add its sound to the block's file, so that one block has one recording.
27. As a Zed user, I want a «Keep session audio» count in settings, defaulting to 20, so that old files disappear on their own.
28. As a Zed user, I want 0 to turn Session Audio off, so that nothing is written when I do not want it.
29. As a Zed user, I want files to stay after a block is deleted or a message is sent, so that I can replay what was sent.
30. As a Zed user, I want the old «Save last recording» flag gone, so that there is one setting for this.
31. As a fork maintainer, I want Session Audio files readable by the `transcribe_wav` example and the integration tests, so that they double as fixtures.

### Devices

32. As a Zed user, I want devices named as Windows names them («Microphone (fifine Microphone)»), so that I can find my microphone.
33. As a Zed user, I want the device identifier only in a tooltip, so that the list stays readable.
34. As a Zed user, I want the device list refreshed when I open the dropdown, so that a microphone plugged in after Zed started is there.
35. As a Zed user, I want the device list refreshed when a session starts, so that the footer names a real device.
36. As a Zed user, I want the fix to apply on the Audio page and in the audio test window too, so that names agree everywhere.
37. As a Zed user, I want a «Test Microphone…» button on the Dictation sub-page, so that I can check the microphone without leaving.

### Glossary

38. As a Zed user, I want Glossary terms shown as chips that wrap, so that seventeen terms take three lines, not seventeen.
39. As a Zed user, I want a remove cross on every chip, so that deleting a term is one click.
40. As a Zed user, I want an add field after the chips, so that adding is where I look.
41. As a Zed user, I want «TypeScript, Docker, Kubernetes» pasted into the add field to become three chips, so that I can paste lists.

### Engine Download

42. As a Zed user, I want a «Download…» button next to the model path, so that I do not hunt for files.
43. As a Zed user, I want to choose from four whisper.cpp models with their sizes, so that I know what I am fetching.
44. As a Zed user, I want a «Download…» button next to the backends folder, so that the Vulkan backend comes from one click.
45. As a Zed user, I want the backends version pinned by the fork, so that a download always matches the built engine.
46. As a Zed user, I want progress in percent and a Cancel in the field row, so that I see the download and can stop it.
47. As a Zed user, I want a download to survive closing the Settings window, so that I can keep working.
48. As a Zed user, I want progress shown again when I reopen Settings, so that I know it is still going.
49. As a Zed user, I want the settings paths filled in automatically when a download finishes, so that dictation just works.
50. As a Zed user, I want a checksum mismatch to delete the file and show an error, so that a corrupt model never loads.
51. As a Zed user, I want a network error shown as a banner on the sub-page, so that I know what failed.
52. As a Zed user, I want downloads to go through Zed's HTTP client, so that my proxy settings apply.
53. As a Zed user, I want the Download button always available, so that I can refresh a copy any time.
54. As a fork maintainer, I want downloads tested against a fake HTTP client, so that no test touches the network.

## Implementation Decisions

### Transcription Engine (crate `dictation`)

- Decoder Loop guard: a transcribed segment whose text is one n-gram repeated three or more times in a row is dropped before it reaches Confirmed Text or Pending Text. The detector is a pure function over segment text; it looks at structure only, never at a word list. The same guard applies to the final tail on stop.
- Trailing no-speech: on stop the tail is transcribed once; a tail segment whose no-speech probability exceeds the configured threshold is dropped. Both guards are the only text-shaped decisions the engine makes (ADR 0001).
- Session Audio store: a new module in the crate owning a directory, one WAV per Dictation Session, 16 kHz mono, named by the id of the Dictation Block the session produced (a discarded session gets its own id). Resume appends PCM to the block's existing file. The store keeps at most N files and evicts the oldest by modification time; N = 0 disables writing. The temp-file «last recording» path and its setting are removed.
- `LiveDictation` reports the input device it actually opened and whether it fell back from a configured device that was not found.

### Settings (crates `settings_content`, `agent_settings`, default settings)

- `agent.dictation.save_last_recording` is removed. `agent.dictation.session_audio.keep` (u32, default 20) replaces it. An old key is ignored like any unknown setting.
- The default Post-processing prompt gains «Thank you» in its list of artifact examples.

### Dictation Window (crate `agent_ui`)

- Footer state is computed by a pure function from the phase and the running-tasks flags: which hints are enabled, their labels, whether the spinner shows, the Raw/Processed label and the microphone label. The render function only draws that result.
- Recording and review bodies get the standard Zed scrollbar honouring `scrollbar.show`.
- During Recognizing and Post-processing: only Cancel enabled; Accept, Tab and Resume disabled and dimmed; «accept when done» behaviour is removed.
- Review footer left side: «Raw» or «Processed · <model name>». Model name is the one actually used, including the agent default fallback. Tooltip: model, provider and the first lines of the prompt. Click: opens the Settings window on the Dictation sub-page. The Tab hint reads «Show Raw» / «Show Processed».
- Play hint in the review footer plays the block's Session Audio on the configured output device; it reads «Stop» while playing; Accept, Cancel and Resume stop playback. Play is absent when Session Audio is off or the file is missing.
- Recording footer shows the input device name after the timer, truncated to the available width, full name in the tooltip; if the configured device was not found, the label reads «<actual device> · configured device not found».
- Dictation Block chip tooltip is unchanged; Play lives only in review.

### Devices (crate `audio`)

- Display name of a device is the Windows friendly name (the extended description cpal provides), falling back to the short name; the identifier moves to a tooltip. Implemented as a pure function over the description fields so the Audio page, the audio test window and the Dictation sub-page share it.
- Device enumeration can be refreshed on demand; the Dictation sub-page refreshes when its microphone dropdown opens, the Dictation Window refreshes at session start.

### Settings sub-page (crate `settings_ui`)

- Glossary: chips with a remove cross, wrapping, add field last. Pasting or typing a comma-separated list adds one chip per term; editing a term is remove and add.
- «Test Microphone…» button opens the existing audio test window.
- Engine Download: a process-global downloader with an injectable HTTP client. A download has a target (model by name or backends), a destination in the Zed data directory (`dictation/models`, `dictation/backends`), a pinned URL and checksum, progress in bytes, and a result path. Models: large-v3-q5_0, large-v3-turbo-q5_0, medium-q5_0, small-q5_1 from the whisper.cpp HuggingFace repository, SHA1 pinned from the whisper.cpp README. Backends: transcribe.cpp 0.2.3 windows-x86_64-cpu-vulkan archive from the GitHub release, SHA256 pinned; the archive is extracted with the existing archive utility and the result path is the inner folder. On success the corresponding setting is written. A checksum mismatch deletes the file and reports an error; Cancel deletes the partial file. No resume of partial downloads. The sub-page shows a percent and Cancel in the field row while a download runs and a banner on error; closing the window does not cancel.

## Testing Decisions

- Good tests check external behaviour through public APIs: given audio, which text in which order; given a directory and N, which files remain; given an HTTP body, which file, checksum verdict and setting; given a phase, which actions are enabled. No tests of render code.
- `dictation`: integration test on a new fixture «1, 2, 3, 4, 5» with trailing silence (recorded with Session Audio, stored next to the Handy recordings, enabled by the same environment variables): Pending Text never contains a repeated n-gram, final text has nothing after the last digit. Pure-function tests of the repetition detector on synthetic strings. Session Audio store tests with a temporary directory: eviction, append on Resume, discarded session kept, N = 0 writes nothing. Prior art: existing `recognition_loop` tests and the `last_recording` unit tests in the crate.
- `agent_settings`: parsing of `session_audio.keep`, default 20, old key ignored. Prior art: dictation settings tests at the bottom of the module.
- `agent_ui`: footer state function tests per phase. Prior art: `dictation_engine` tests.
- `audio`: display name function tests. `settings_ui`: term splitting tests next to the existing glossary helper tests.
- Engine Download: tests with a fake HTTP client and a temporary directory: progress, checksum mismatch deletes, cancel deletes partial, archive extracted, setting written. Prior art: crates using `FakeHttpClient` in their tests.
- Verified by hand in the dev build with the isolated data dir: scrollbar, disabled hints and spinner, Raw/Processed label and its tooltip and click, playback on the configured output, microphone name in the footer, device names and refresh, chips, Test Microphone, Download buttons with progress.

## Out of Scope

- Editing the Post-processing prompt inside the Dictation Window.
- Play on the Dictation Block chip.
- Resuming partial downloads; «latest release» lookups; models outside the fixed list; non-Windows backends.
- Deleting Session Audio when a block is deleted or a message is sent.
- Silero or other dedicated VAD; temperature fallback in the decoder.
- Sounds playback for start and stop.
- Quote Reply.

## Further Notes

- Build hygiene from `LOCAL_DEV.md` applies; changes in `audio` and `settings_content` touch upstream crates, keep them in clearly delimited `Local:` blocks.
- The fifine microphone shows up in cpal today; only its name is wrong. Nothing about device filtering needs changing.
- Whisper large-v3 q5_0 is 1.1 GB; a download at home speeds takes minutes, which is why downloads are process-global.
