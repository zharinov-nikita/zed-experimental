# Voice Dictation, итерация 4: Speech Gate, запуск Ollama, звуки

Метка: `ready-for-agent`
Словарь: `CONTEXT.md` в корне репозитория. Термины (Dictation Session, Dictation Window, Live Transcript, Confirmed Text, Pending Text, Post-processing, Recognizer Artifact, Decoder Loop, Speech Gate, Session Audio, Transcription Engine, Resume, Composer) используются строго в его значениях.
Решения: `docs/adr/0001-decoder-loop-guard-stays-in-the-engine.md`, `docs/adr/0002-speech-gate-decides-whether-to-decode.md`.
Первичные источники: ручная проверка dev-сборки итерации 3 (2026-09-06), спека итерации 3 `.scratch/voice-dictation-3/spec.md`, `LOCAL_DEV.md`.

## Problem Statement

Итерация 3 проверена руками. Четыре наблюдения:

1. Пока пользователь молчит, в Pending Text появляются Recognizer Artifacts («Продолжение следует», «Время обновления»). Они остаются синими, но секция выглядит сломанной, а после долгой паузы артефакт может подтвердиться раньше настоящих слов.
2. Post-processing использует Ollama, а Ollama после перезагрузки не запущен: его приходится поднимать руками, иначе диктовка молча уходит на модель агента по умолчанию или падает с ошибкой.
3. В теле Dictation Window появляется горизонтальный scrollbar, которому там нечего делать.
4. Настройка `agent.dictation.sounds` включена, но звука нет: настройка есть, кода за ней нет.

## Solution

1. Speech Gate в Transcription Engine: пока в недекодированном звуке нет речи относительно шума сессии, движок ничего не декодирует, Pending Text пуст; когда речь начинается, декодирование идёт от её начала с запасом; при остановке хвостовая тишина не декодируется. Гейт no-speech из итерации 3 удаляется (ADR 0002).
2. Zed сам поднимает Ollama в начале Dictation Session, если провайдер Post-processing это Ollama на `localhost` и сервер не отвечает; если за отведённое время сервер не поднялся, просмотр показывает, что Ollama не запустился, и оставляет сырой текст.
3. В теле Dictation Window только вертикальный scrollbar.
4. `sounds: true` даёт звук на старте записи и Resume и звук на остановке.

## User Stories

### Speech Gate

1. As a Zed user, I want the Pending Text to stay empty while I am silent, so that a pause looks like a pause and not like a bug.
2. As a Zed user, I want «Продолжение следует» and similar phrases never to appear in the Live Transcript while I am silent, so that I do not read text I never said.
3. As a Zed user, I want the words I say after a long pause to be confirmed without an artifact in front of them, so that the pause does not leak garbage into the Confirmed Text.
4. As a Zed user, I want a quiet first word after a pause to be kept, so that the gate never costs me speech.
5. As a Zed user, I want the gate to adapt to my microphone and room, so that a noisy fan does not count as speech and a quiet voice does not count as silence.
6. As a Zed user, I want the timer to keep running and «Listening…» to stay while the gate is closed, so that I know recording is on.
7. As a Zed user, I want a long silence not to fill the recognition window, so that the first words after it are recognized as fast as the first words of the session.
8. As a Zed user, I want the tail after my last word not to be decoded on stop, so that «Thank you.» has no silence to grow from.
9. As a Zed user, I want the loop guard from iteration 3 to keep working on whatever the gate lets through, so that a Decoder Loop still never reaches the text.
10. As a fork maintainer, I want the gate to be a pure function over samples with engine constants, so that it can be tuned on fixtures without a model and without settings.
11. As a fork maintainer, I want the no-speech re-decode of tail segments removed, so that the engine has one explainable silence decision instead of two.
12. As a fork maintainer, I want real microphone fixtures for the gate, so that the tests see the noise floor a synthetic silence cannot provide.

### Ollama

13. As a Zed user, I want Zed to start Ollama when a Dictation Session starts and the server is not answering, so that Post-processing works after a reboot without my help.
14. As a Zed user, I want the server started in the background while I dictate, so that the startup time hides behind the dictation.
15. As a Zed user, I want Zed to start only Ollama on `localhost`, so that it never launches anything for a remote server.
16. As a Zed user, I want Zed to start the Ollama desktop application when it is installed, so that the server lives in the tray the way it does when I start it myself.
17. As a Zed user, I want `ollama serve` used when the desktop application is absent, so that a plain install works too.
18. As a Zed user, I want Post-processing to wait for the server up to a limit, so that a slow start still ends in processed text.
19. As a Zed user, I want to be told that Ollama did not start when the limit is over, so that I know why the text is raw.
20. As a Zed user, I want no silent fallback to the agent's default model when Ollama is configured, so that my local model is never replaced by a cloud one without my knowledge.
21. As a Zed user, I want the footer to say that Ollama is starting while Post-processing waits, so that the spinner is not a mystery.
22. As a Zed user, I want a second start attempt in the same Zed run when the first one failed, so that a fixed Ollama needs no restart of Zed.
23. As a fork maintainer, I want the launcher tested with fake reachability and a fake process start, so that no test starts a real server.

### Dictation Window

24. As a Zed user, I want only a vertical scrollbar in the section body, so that nothing scrolls sideways.
25. As a Zed user, I want a sound when recording starts, so that I know the microphone is open without looking.
26. As a Zed user, I want the same sound on Resume, so that continuing a block feels like starting one.
27. As a Zed user, I want a sound when recording stops, so that I know the microphone is closed.
28. As a Zed user, I want no sound on Accept and Cancel in review, so that reviewing stays quiet.
29. As a Zed user, I want the sounds on the output device from the Audio settings, so that they go where my headphones are.
30. As a Zed user, I want the sounds off by default, so that nothing changes for anyone who did not ask.

## Implementation Decisions

### Transcription Engine (crate `dictation`)

- Speech Gate: a pure state machine over 16 kHz mono samples with engine constants. It keeps a running estimate of the session's noise floor from the quiet frames it has seen, opens when the energy of a short frame rises clearly above that floor, and closes only after a clear stretch of silence. On opening it reports where speech began, minus a margin of audio, so the decoder sees the syllable the gate reacted to. It errs on the side of speech: opening is easy, closing is slow.
- The live loop asks the gate before decoding. While the gate is closed, the loop does not run the decoder, sends updates with an empty Pending Text and moves the commit point forward past the silence so the recognition window never fills with it. When the gate opens, decoding starts at the reported start of speech. Buffers that already contain speech are handled as today.
- On stop, the tail is cut at the gate's end of speech before it is decoded; the tail segments are then subject only to the Decoder Loop guard. The isolated no-speech re-decode of tail segments from iteration 3 and the `logprob_thold` trick are removed. The probing example stays for future study.
- The gate constants live next to the other loop constants; there are no settings.
- Session Audio keeps the whole session including silence, so fixtures recorded with it exercise the gate.

### Ollama launch (crate `agent_ui`)

- A model server launcher with two injected capabilities: «is the server answering» and «start the server». It runs when a Dictation Session starts, only when the Post-processing provider is Ollama and its URL is the default local one. If the server does not answer, it starts the Ollama desktop application found next to the `ollama` executable on the PATH, or `ollama serve` hidden when the application is absent, then polls until the server answers or a limit of 20 seconds passes. A failed attempt does not stop the next Dictation Session from trying again.
- Post-processing awaits the launcher before resolving the model. While it waits, the footer spinner reads «Starting Ollama…». When the limit passes, Post-processing does not run: the review shows a Callout that Ollama did not start and the text stays raw. The fallback to the agent's default model remains only for the case where no Post-processing model is configured at all.

### Dictation Window (crate `agent_ui`)

- The recording body gets a scrollbar along the vertical axis only, still honouring `scrollbar.show`.
- Sounds: a pure function maps a session transition to a sound: start of recording and Resume to the existing «unmute» sound, stop of recording to the existing «mute» sound, everything else to no sound. The window plays it through the audio crate on the configured output device when `agent.dictation.sounds` is on. The default stays off.

## Testing Decisions

- Good tests check external behaviour through public APIs: given audio, which updates come out; given a reachability sequence, whether and when the launcher starts a process and what it returns; given a transition, which sound. No tests of render code.
- `dictation`: gate tests on synthetic signals (noise floor, a burst above it, closing after silence, the margin before the opening point); loop tests on the recorded fixtures through `LiveDictation::start_from_pcm`: during the silent stretch every update has an empty Pending Text, after the pause the Confirmed Text starts with the spoken words, nothing follows the last word, quiet words survive. Prior art: `recognition_loop` tests and the loop-guard unit tests.
- Fixtures: two Session Audio recordings made by the user with the real microphone, stored next to the Handy recordings and enabled by the same environment variables: digits followed by six seconds of silence, and a phrase, an eight-second pause, a continuation. The synthetic digits fixture from iteration 3 stays.
- `agent_ui`: launcher tests with fake reachability and start closures: answering at once starts nothing; answering after a few polls returns success and started once; never answering returns failure after the limit and started once. Footer state tests for the «Starting Ollama…» spinner. Sound mapping tests. Prior art: `dictation_footer` and `dictation_engine` tests.
- Verified by hand in the dev build: empty Pending Text during silence, no artifact after a long pause, Ollama started by Zed after being closed from the tray, the Callout when it cannot start, no horizontal scrollbar, sounds on start, Resume and stop.

## Out of Scope

- A neural VAD (Silero) — kept in reserve if the relative-energy gate cuts quiet speech.
- Settings for the gate.
- Starting any model server other than Ollama, or Ollama at a non-default URL.
- Enabling Ollama's own start-at-login; the user does that in the Ollama tray.
- Sounds for Accept, Cancel and Post-processing.
- Quote Reply.

## Further Notes

- ADR 0002 records why an energy-based gate is acceptable on the input side after ADR 0001 rejected it as a word filter.
- The no-speech probe example documented in `LOCAL_DEV.md` stays; the description of the tail guard there is replaced by the Speech Gate.
