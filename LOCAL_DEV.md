# Локальная разработка Zed на Windows (личная шпаргалка)

> Подробности для Claude — в `.claude/skills/zed-local/SKILL.md`.

## Быстрая сборка (не лагает)

```powershell
cargo run --profile release-fast          # собрать и запустить
cargo build --profile release-fast --package zed   # только собрать
```

- Бинарник: `target\release-fast\zed.exe` — **малиновая иконка** = ваша локальная сборка.
- Профиль `release-fast`: оптимизации как в release, но линкуется быстро (без LTO). Задача есть и в Zed: `.zed/tasks.json`.
- ⚠️ Не задавайте глобальный `RUSTFLAGS` — сломает сборку (перебьёт обязательные флаги из `.cargo/config.toml`).

## Параллельные задачи в worktree

```powershell
.\script\new-worktree.ps1 фича1              # новая ветка "фича1" + worktree ..\zed-фича1
.\script\new-worktree.ps1 фича1 имя-ветки    # из существующей ветки
```

Скрипт создаёт изолированный data-dir `%LOCALAPPDATA%\Zed-Local\фича1` (настройки/кеймапы общие через junction на `%APPDATA%\Zed`, а база/окна/расширения — свои). Запуск из worktree:

```powershell
cargo run --profile release-fast -- --user-data-dir "$env:LOCALAPPDATA\Zed-Local\фича1"
```

Несколько экземпляров Zed работают одновременно и независимо (dev-канал не имеет single-instance блокировки). Первая сборка нового worktree — почти холодная, ~35–40 мин (sccache попадает в кеш лишь на ~27%: workspace-крейты привязаны к абсолютному пути); дальше пересборки в worktree быстрые. Удаление: `git worktree remove --force ..\zed-фича1` + удалить data-dir (в git включён `core.longpaths`, без него удаление падает на глубоких путях `target\`).

## Иконки

- Цвета официальных каналов: чёрный = stable, синий = preview, тёмно-фиолетовый = nightly, серый = обычный dev. Локальная сборка — **малиновый**.
- Перекрасить (например, свой цвет для worktree): см. секцию Recolor в `.claude/skills/zed-local/SKILL.md` (скрипт `recolor.py`, сдвиг тона от preview-иконки).
- После замены иконки перед сборкой: `(Get-Item crates\zed\build.rs).LastWriteTime = Get-Date` — иначе cargo может не перевстроить ресурсы.

## sccache

Установлен, включён через пользовательские env (`RUSTC_WRAPPER=sccache`, `SCCACHE_CACHE_SIZE=40G`). Реально помогает при пересборке после `cargo clean` в той же директории и на зависимостях из registry; между worktree выигрыш скромный (~27%). Статистика: `sccache --show-stats`.

## Голосовая диктовка (фича форка)

Код: крейт `crates/dictation` (движок transcribe.cpp + захват микрофона), `crates/agent_ui/src/dictation_window.rs`
(секция над композером), Dictation Block = crease с `MentionUri::Dictation`. Решения и словарь: `prototypes/voice-dictation/DECISION.md`, `CONTEXT.md`.

- Хоткей `ctrl-alt-space` в композере панели агента: старт, повторно — остановить и перейти в просмотр (как `esc`).
  В просмотре: `enter` принять, `esc` отменить, `tab` сырой/обработанный текст, `ctrl-p` Play/Stop, `ctrl-alt-space` продолжить.
  Пока идёт «Recognizing…» или Post-processing, `enter` и `tab` ничего не делают, работает только `esc`.
  Dictation Window разворачивается секцией над композером на всю его ширину (тело до 10 строк, дальше прокрутка);
  в подвале только короткие подсказки `esc`/`enter`/`tab`, хоткей старта, остановки и Resume показывает тултип
  кнопки микрофона.
- Настройки в окне Settings: AI → General → Dictation (поиск по «whisper», «microphone»). Подстраница правит
  `agent.dictation` и `audio.experimental.input_audio_device` в `settings.json`; провайдер и модель Post-processing
  пишутся парой в `agent.dictation.post_processing.model` (как `agent.default_model`), «Agent default model» удаляет
  ключ; «Reset to default» у промпта удаляет `prompt`, и действует умолчание из `assets/settings/default.json`.
- Нужны настройки `agent.dictation.model_path` и `backends_dir` (см. `prototypes/voice-dictation/PLAN.md`,
  там же готовый фрагмент для этой машины). Vulkan-бэкенд не собирается из исходников: `ggml-vulkan.dll`
  берётся из официального артефакта transcribe.cpp в `%LOCALAPPDATA%\zed-dictation\transcribe-native`.
- Сборка крейта `dictation` компилирует ggml/transcribe.cpp через cmake (первый раз ~4–7 мин). DLL (`transcribe.dll`,
  `ggml*.dll`) кладутся рядом с `zed.exe` скриптом сборки крейта `transcribe-cpp-sys`.
- Проверка движка без UI: `cargo run --profile release-fast -p dictation --example transcribe_wav -- <model.bin> <backends_dir> <file.wav>`
  (печатает сегменты, переходы Speech Gate и то, что распознаётся, если обрезать файл по концу речи, как при остановке);
  `--example no_speech_probe` показывает, что декодер и гейт делают со срезами записи и с тишиной.
- Интеграционные тесты цикла распознавания (`crates/dictation/tests/recognition_loop.rs`) гоняют записи Handy и
  включаются только переменными окружения: `ZED_DICTATION_MODEL=<model.bin>`, `ZED_DICTATION_BACKENDS=<backends_dir>`,
  `ZED_DICTATION_RECORDINGS=%APPDATA%\com.pais.handy\recordings`; затем `cargo test --profile release-fast -p dictation`.
  Без переменных тесты выходят сразу. Фикстура `zed-digits-with-silence.wav` в той же папке (цифры «один … пять» с
  6 с тишины в конце, синтезирована голосом Windows «Microsoft Pavel» через `System.Speech`) проверяет, что Decoder Loop
  не попадает в Live Transcript и после последней цифры ничего нет; без этого файла тест пропускается.
  Фикстуры Speech Gate там же, это Session Audio с реальным микрофоном (шумовой пол драйвера: тихие кадры обнулены):
  `zed-mic-digits-then-silence.wav` (счёт до десяти и 9 с молчания), `zed-mic-silence.wav` (48 с молчания с дыханием
  и щелчками, движок раньше читал в них «Продолжение следует») и `zed-mic-phrase-pause-continuation.wav` (счёт до десяти,
  пауза 8 с, счёт до двадцати; склеена из двух Session Audio того же микрофона, чтобы пауза несла настоящий шум).
  Тесты на них проверяют пустой Pending Text во время молчания, подтверждение речи после паузы без артефакта впереди
  и что после последнего слова ничего нет. Эталон для сравнения с потоковым текстом теперь считается по файлу,
  обрезанному по концу речи гейта.
- Decoder Loop (одна n-грамма три и более раз подряд, или три одинаковых сегмента подряд) отбрасывается движком по форме
  вывода (`is_decoder_loop`, ADR 0001).
- Speech Gate (`crates/dictation/src/speech_gate.rs`, ADR 0002): чистый автомат над кадрами 20 мс с константами движка, без
  настроек. Шумовой пол сессии — минимум уровня кадров, растёт не быстрее 3 дБ/с и не ниже −70 дБFS; кадр считается речью
  при уровне ≥ max(пол + 10 дБ, −50 дБFS). Гейт открывается после 100 мс речи и сообщает её начало с запасом 0,4 с,
  закрывается после 3 с тишины (паузы «на подумать» внутри фразы доходят до 2,2 с, а деление там теряет слова) и
  сообщает конец речи как последний громкий кадр + 0,5 с. Пока гейт закрыт, цикл не декодирует и шлёт пустой Pending Text;
  при открытии точка фиксации прыгает к началу речи, так что тишина не заполняет окно; при закрытии фраза декодируется
  один раз без тишины после неё и подтверждается целиком. Живой буфер тоже режется по концу речи, так что артефакт не
  растёт из тишины и до закрытия. При остановке хвост режется там же; гейта no-speech и `logprob_thold` в движке больше нет.
  Константы подобраны по записям Handy и Session Audio; на них гейт не открывается на щелчках и дыхании, но открывается
  на громком шорохе (−39 дБFS, 0,7 с), который Whisper читает как «Время обновления», это остаётся Post-processing.
- Микрофон берётся из `audio.experimental.input_audio_device` (страница Audio в Settings); пустое или неизвестное
  значение даёт устройство по умолчанию, в подвале записи тогда стоит «<устройство> · configured device not found».
  Устройства везде называются как в Windows (friendly name из WASAPI), идентификатор в тултипе выпадающего списка;
  список перечисляется заново при открытии списка и при старте сессии.
- Модель грузится при первом старте (секция показывает «Loading Whisper model…», микрофон открывается после загрузки)
  и остаётся в памяти до закрытия Zed; `agent.dictation.keep_model_loaded: false` выгружает её после каждой сессии.
  На процесс одна сессия: старт во втором окне даёт Callout «Dictation is already running in another window».
- Session Audio: звук каждой сессии пишется в `<data dir>\dictation\audio\<id блока>.wav` (16 кГц mono), Resume дописывает
  в файл того же блока, отменённая сессия тоже сохраняется. `agent.dictation.session_audio.keep` (по умолчанию 20) задаёт,
  сколько файлов хранить, старейшие удаляются; 0 выключает запись. В просмотре подсказка Play (`ctrl-p`) играет файл на
  `audio.experimental.output_audio_device`. Файлы читаются `transcribe_wav` и годятся как фикстуры. Старый ключ
  `save_last_recording` игнорируется. Data dir локальной сборки: `%LOCALAPPDATA%\Zed` (или `--user-data-dir`).
- Engine Download: на подстранице Dictation у полей «Whisper Model Path» и «Backends Folder» кнопки «Download…». Модели
  (large-v3-q5_0, large-v3-turbo-q5_0, medium-q5_0, small-q5_1) качаются с HuggingFace в `<data dir>\dictation\models`,
  SHA1 из README whisper.cpp; бэкенды — архив transcribe.cpp `windows-x86_64-cpu-vulkan` с GitHub-релиза в
  `<data dir>\dictation\backends`, SHA256 из релиза. Версия и суммы закреплены в `crates/dictation/src/engine_download.rs`
  (`TRANSCRIBE_CPP_VERSION` должна совпадать с `transcribe-cpp` в корневом `Cargo.toml`, есть тест). Загрузка живёт
  процесс-глобально (закрытие Settings её не отменяет), по завершении путь пишется в `settings.json`.
- В подвале Dictation Window во время «Recognizing…» и Post-processing доступен только Cancel; в просмотре слева метка
  «Raw» / «Processed · <модель>» (тултип с началом промпта, клик открывает Settings › AI › Dictation), `tab` — «Show Raw» /
  «Show Processed». Тело секции только с вертикальным scrollbar по `scrollbar.show` (текст переносится, вбок не
  прокручивается); для этого в `ui` добавлен `Scrollbars::for_settings_along`, потому что `show_along` ось не убирает.
- Запуск Ollama (`crates/agent_ui/src/dictation_model_server.rs`): если провайдер Post-processing это `ollama` с адресом
  по умолчанию (`http://localhost:11434`, пустой `language_models.ollama.api_url` считается им же) и `/api/version` не
  отвечает, в начале Dictation Session Zed запускает `ollama serve` из PATH скрытым процессом (приложение Ollama с окном и
  треем намеренно не трогается) и опрашивает сервер каждые 0,5 с до 20 с. Post-processing ждёт
  лаунчер прежде чем выбирать модель, в подвале тогда спиннер «Starting Ollama…». Если сервер не поднялся, просмотр
  показывает Callout «Ollama did not start» и сырой текст. Настроенная, но недоступная модель Post-processing теперь
  ошибка («Post-processing Unavailable»), а не тихая подмена моделью агента; подмена остаётся только когда модель вообще
  не задана. Удалённый адрес и другие провайдеры ничего не запускают; неудача не мешает следующей сессии попробовать снова.
- Звуки (`agent.dictation.sounds`, по умолчанию `false`): старт записи и Resume играют `unmute`, остановка записи (`esc`,
  хоткей, подсказка Review) — `mute`, всё через `audio::Audio::play_sound` на `audio.experimental.output_audio_device`.
  Accept, Cancel, Post-processing и ошибки беззвучны; какой переход чем звучит, решает `sound_for` в `dictation_window.rs`.

## Ветки и форк

Это личный экспериментальный форк `zharinov-nikita/zed-experimental`; PR в оригинальный Zed не планируются. Иконки, `script/new-worktree.ps1`, skill и этот файл коммитятся в ветку `zed-experimental` и пушатся в форк. `main` — чистое зеркало апстрима, свои коммиты туда не добавлять.
