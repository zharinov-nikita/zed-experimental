# Локальные STT-движки быстрее/точнее Whisper для диктовки — проверенные факты

Источники (первичные, если не помечено иначе):

- **Локально на диске**: `crates/dictation/src/dictation.rs`, `crates/dictation/src/engine_download.rs`,
  `crates/settings_content/src/agent.rs`, `docs/adr/0001-*.md`, `docs/adr/0002-*.md`;
  вендорённые крейты `transcribe-cpp-0.2.3` и **`transcribe-cpp-sys-0.2.3`** (в sys лежит весь
  нативный C++: `src/arch/<family>/`, `include/transcribe/*.h`, `src/transcribe.cpp`).
- **Апстрим transcribe.cpp**: github.com/handy-computer/transcribe.cpp, тег `v0.2.3` =
  `63a44d9239d610b3908e8a66b384924cd4a77217` (совпадает с `.cargo_vcs_info.json` крейта).
  README, `docs/models/*.md`, `docs/build-windows.md`, `docs/tools/wer.md`,
  `docs/extension-kinds.md`, `docs/porting/families/*.md`, `scripts/convert-*.py`. Лицензия MIT.
- **Карточки моделей HF**: `nvidia/nemotron-3.5-asr-streaming-0.6b`, `nvidia/parakeet-tdt-0.6b-v3`,
  `nvidia/canary-1b-v2`, `mistralai/Voxtral-Mini-4B-Realtime-2602`, `ai-sage/GigaAM-v3`,
  `ai-sage/GigaAM-Multilingual`, `moonshine-ai/moonshine-streaming-small`, `kyutai/stt-1b-en_fr`,
  `openai/whisper-large-v3-turbo`, `alphacep/vosk-model-*`, `t-tech/T-one`.
- **Статьи**: Whisper (arXiv 2212.04356, Appendix D.2.4, Table 13), Voxtral Realtime (arXiv 2602.11298),
  Moonshine v1/v2 (arXiv 2410.15608 / 2602.12241), Flavors of Moonshine (arXiv 2509.02523).
- **Репозитории/бенчмарки**: salute-developers/GigaAM (`evaluation.md`), voicekit-team/T-one,
  k2-fsa/sherpa-onnx, alphacep/vosk-api, ekhodzitsky/gigastt, NVIDIA/NeMo-Speech.cpp,
  ggml-org/whisper.cpp, SYSTRAN/faster-whisper, OpenNMT/CTranslate2, huggingface/distil-whisper,
  huggingface/candle, huggingface/open_asr_leaderboard,
  HF Space `Vikhrmodels/Russian_ASR_Leaderboard` + датасет `Vikhrmodels/russian-asr-leaderboard`.
- **Проверка дат (§8)**: HF API `models?author=nvidia&sort=createdAt|lastModified`,
  GitHub Releases `NVIDIA/NeMo`, README `NVIDIA/NeMo-Speech.cpp`, crates.io API
  `/api/v1/crates/transcribe-cpp`, GitHub compare `handy-computer/transcribe.cpp v0.2.3...main`.
  Дата проверки — 2026-09-12.

---

## 1. Главное: что `transcribe-cpp` 0.2.3 уже умеет без смены зависимости

- Крейт — биндинг к **transcribe.cpp** (MIT, ggml). В `transcribe-cpp-sys-0.2.3/src/arch/` лежат
  **18 архитектур**, зарегистрированных в `src/transcribe-arch.cpp`: `parakeet`, `canary`,
  `canary_qwen`, `cohere_asr`, `qwen3_asr`, `voxtral`, `voxtral_realtime`, `whisper`, `moonshine`,
  `moonshine_streaming`, `sensevoice`, `funasr_nano`, **`gigaam`**, `granite_speech`,
  `granite_speech_nar`, `medasr`, `moss`, `sortformer`. **Смена семейства = смена файла модели.**
  Enum-ы `RunExtension`/`StreamExtension` в Rust — это не список семейств, а подмножество тех,
  у кого есть типизированные ручки.
- В дереве апстрима **нет** zipformer, kyutai/moshi, fireredasr, wav2vec, distil — проверено
  поиском по всему дереву (3221 путь).
- Формат — **один самодостаточный `.gguf` на (вариант, квант)**, токенизатор и метаданные внутри
  KV. Легаси `.bin` whisper.cpp читает только whisper (`src/transcribe-bin-loader.h`: «the per-arch
  adapter — currently only whisper»). Сейчас проект качает именно `.bin` из
  `huggingface.co/ggerganov/whisper.cpp` (`engine_download.rs`), значит переход на другое семейство =
  новый список ассетов из `huggingface.co/handy-computer/<variant>-gguf`.
- Лестница квантов: `F32, F16, Q8_0, Q6_K, Q5_K_M, Q4_K_M`. Исключения задокументированы:
  Moonshine (обе линии) пропускает K-тиры, Sortformer ограничен F32/F16/Q8_0.
- **Whisper в этой библиотеке не стримит.** `src/arch/whisper/capabilities.cpp`:
  «supports_streaming left at its zero-init default (false)». `src/transcribe.cpp:1778` возвращает
  `TRANSCRIBE_ERR_NOT_IMPLEMENTED`. `docs/models/whisper.md`: «What's not supported … real-time
  streaming (whisper is not streaming-first; chunked 30-second windows only)». Отсюда весь
  самодельный цикл в `dictation.rs`.
- `supports_streaming = true` дают: `moonshine_streaming` (хардкод), `voxtral_realtime` (хардкод) и
  `parakeet` — **по GGUF KV `stt.capability.streaming`**, которое конвертер выставляет только для
  cache-aware (`chunked_limited`) и buffered (`chunked_limited_with_rc`) вариантов
  (`scripts/convert-parakeet.py:1480`). Из 14 parakeet-вариантов стримят четыре:
  `nemotron-speech-streaming-en-0.6b`, `nemotron-3.5-asr-streaming-0.6b`,
  `multitalker-parakeet-streaming-0.6b-v1` (cache-aware) и `parakeet-unified-en-0.6b` (buffered).
  GigaAM, Canary, Qwen3-ASR, SenseVoice — не стримят.
- 0.2.3 — **головной релиз**: на crates.io `newest_version = 0.2.3` (2026-08-30), последний тег на
  GitHub тоже `v0.2.3`; `main` опережает на 2 коммита без новых семейств и без изменений API.

### Что именно означает каждое стриминговое расширение

- **`ParakeetStream` (`PKST`)** — настоящий cache-aware стриминг: инкрементальная подача PCM,
  состояние энкодера (`cache_last_channel` / `cache_last_time`) переносится между вызовами,
  RNN-T greedy-декодер тащит LSTM-state. Константная память, без переэнкодинга.
  Меню lookahead у nemotron-EN: `right ∈ {13, 6, 1, 0}` = `{1040, 480, 80, 0}` мс при кадре 80 мс
  (`include/transcribe/parakeet.h`). У nemotron-3.5 — `R ∈ {0, 3, 6, 13}`.
  `docs/models/nemotron-speech-streaming-en-0.6b.md`: при R=13 стриминговый транскрипт
  **байт-в-байт равен офлайновому**.
- **`ParakeetBuffered` (`PKBS`)** — буферизованный: «the encoder re-runs over each new
  `[left | chunk | right]` PCM window». Только `parakeet-unified-en-0.6b` (английский).
  Значения обязаны быть кратны 80 мс, иначе `INVALID_ARG` — «the runtime never silently floors».
- **`MoonshineStreaming` (`MSST`)** — инкрементальный энкодер + **полное переразложение префикса**
  авторегрессивным декодером на каждый partial. `docs/porting/families/moonshine_streaming.md`:
  «a token that was committed on an earlier feed can still change in the raw hypothesis later.
  `committed_text` is append-only and is **not** rolled back». ~240 мс правого контекста.
- **`VoxtralRealtime` (`VRST`)** — настоящий инкрементальный стриминг (StaticCache энкодера
  бит-в-бит совпадает с офлайновым), один текстовый токен на слот 80 мс.
  `num_delay_tokens` принимает 1..15 (80–1200 мс) или 30 (2400 мс), дефолт 6 = 480 мс.
  Но *tentative*-декод «reprocesses the accumulated buffer» — это то, что душит
  `min_decode_interval_ms`.
- **Sortformer — не стриминговое расширение**: его kind `SFST` сидит на **RUN**-слоте.
  `include/transcribe/sortformer.h`: «the shipped entry point is the batch `transcribe_run` over a
  whole recording». Текста не даёт вообще, только сегменты говорящих.

### Механика Confirmed/Pending уже есть в библиотеке

- `StreamText { full, committed, tentative }`, `CommitPolicy::{Auto, OnFinalize, StablePrefix}`,
  `stable_prefix_agreement_n` (0 → дефолт 3). Это ровно та пара, что сейчас собирается вручную из
  `segments_to_confirm` + `quietest_point`.
- Реализация выбирается по имени арки (`src/transcribe.cpp:1100`):
  `parakeet → FamilyNativeCommit` («Parakeet publishes native committed chunks; agreement_n does not
  add useful evidence»), `moonshine_streaming → FamilyTokenAgreement`, **всё остальное (включая
  `voxtral_realtime`) → `GenericTextAgreement`** — дек из последних N сырых гипотез, коммитится их
  общий байтовый префикс с обрезкой по границе UTF-8.
- **`CommitPolicy::Auto` и `CommitPolicy::StablePrefix` в 0.2.3 ведут себя одинаково** — обе падают
  в одну ветку `switch` в `apply_stream_text_policy`. Разница будет, только если появится
  family-specific auto.
- ⚠ Заголовок прямо предупреждает: «committed_text is best-effort, not a correctness guarantee.
  For models that re-attend over a growing audio context (e.g. moonshine_streaming), the raw
  hypothesis can revise a byte that was already committed… the committed/tentative seam is
  transiently incoherent mid-stream. full_text is the authoritative raw hypothesis at all times».
  Для parakeet с native-commit это не проблема, для voxtral/moonshine — надо смотреть глазами.

---

## 2. Кандидаты внутри 0.2.3

### 2.1 `nemotron-3.5-asr-streaming-0.6b` — единственный «стриминг + русский + английский» в семействе

- **Настоящий cache-aware streaming.** Карточка NVIDIA: «Unlike buffered inference, this model
  maintains caches for all encoder self-attention and convolution layers… there are no overlapping
  computations; each processed frame is strictly non-overlapping.»
- **Русский — в верхнем тире.** Карточка: «Transcription-ready (19 locales): … **Russian (ru-RU)** …».
  Всего 40 локалей: 19 transcription-ready + 13 broad-coverage + 8 adaptation-ready (требуют
  файнтюна и **не** попадают в `general.languages`, `scripts/convert-parakeet.py:358-376`).
- **Задержка — рантайм-кнопка.** `StreamExtension::ParakeetStream(ParakeetStreamOptions {
  att_context_right })`, меню `R ∈ {0, 3, 6, 13}`; апстрим-док перечисляет чанки
  80 / 160 / 320 / 560 / 1120 мс. Карточка: «Choose the optimal operating point on the
  latency-accuracy Pareto curve at inference time. No re-training is required.»
- **WER на русском (FLEURS, таблица карточки NVIDIA), режим LangID:**
  80 мс → 10.84 %, 160 мс → 10.73 %, 320 мс → 9.87 %, 560 мс → 9.60 %, 1.12 с → 9.17 %.
  Auto-detect дороже на 1.3–1.6 п.п. (12.47 → 10.03). **Язык надо задавать явно.**
  Нормализация — своя у NVIDIA, карточка сама признаёт: «Normalization is not perfect across all
  40 language-locales… actual transcription quality may be somewhat better».
  ⚠ Апстрим transcribe.cpp **русский не валидировал**: «WER is gated on English only… The other
  39 locales are exercised functionally but not WER-scored here».
- **Лицензии.** Код transcribe.cpp — MIT. Веса — `license: other`, `license_name: openmdw-1.1`
  (openmdw.ai/license/1-1/): «permission is hereby granted, free of charge, to deal in the Model
  Materials without restriction», обязанность сохранить текст соглашения и notices,
  patent-retaliation по образцу Apache-2.0, **никаких non-commercial ограничений**.
  Структурно это MIT/Apache-подобная лицензия; юридического заключения о совместимости с
  GPL-3.0-or-later никто не давал — [НЕПОДТВЕРЖДЕНО как юридический вывод].
  ⚠ Английский родитель `nemotron-speech-streaming-en-0.6b` идёт под **другой** лицензией —
  `nvidia-open-model-license`.
- **GGUF** (`handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf`, репозиторий проверен через HF API):
  F32 2.38 GB, F16 1.19 GB, **Q8_0 716 MB**, Q6_K 593 MB, Q5_K_M 534 MB, Q4_K_M 473 MB.
  RAM/VRAM NVIDIA не публикует — [НЕПОДТВЕРЖДЕНО].
- **Скорость на Vulkan** (апстрим, AMD Ryzen 7 PRO 4750U / RADV RENOIR, Fedora 43, 3 итерации +
  1 warmup): Q8_0 — jfk 11.0 с → 773 мс (14×), dots 35.3 с → 2.37 с (15×); CPU — 1.37 с (8×) и
  4.76 с (7×). Metal M4 Max — 113 мс (98×). Windows+Vulkan именно для этой модели не мерили —
  [НЕПОДТВЕРЖДЕНО].
- **Единственная найденная стриминговая latency-цифра** (у английского родителя, Metal):
  `docs/models/nemotron-speech-streaming-en-0.6b.md` даёт `lat_p50` при R=13 — NeMo-референс 718 мс
  против 206 мс у cpp-Q8_0.
- Длина не ограничена: `max_audio_ms == 0`, «the cache-aware streaming path carries constant-memory
  caches rather than a growing KV». Окно 30 с и коммит-поинт становятся не нужны.
- Пунктуация и регистр нативные (PnC), таймстемпы token/word.
- **Поддержка в 0.2.3 подтверждена по исходнику**: `arch/parakeet/weights.cpp:431` читает
  `stt.parakeet.prompt.dictionary.locales`, `arch/parakeet/model.cpp:645` содержит
  `is_lang_tag_piece` с комментарием «The nemotron-3.5 vocab emits one per segment».
  Что конкретный GGUF из `handy-computer` грузится библиотекой 0.2.3 — [НЕПОДТВЕРЖДЕНО],
  проверяется одной загрузкой.
- ⚠ **Ловушка с кодом языка.** `general.languages` у этого GGUF содержит **только локали**
  (`ru-RU`, `en-US`, …) — конвертер перечисляет их явно. При этом словарь промптов
  (`stt.parakeet.prompt.dictionary.*`) «still carries all 40 locales + aliases (en, en-US, enGB …)
  + the auto slot», а `resolve_prompt_id` (`arch/parakeet/model.cpp:143`) ищет по словарю промптов.
  То есть `ru` из `DictationLanguage::code()` скорее всего сработает для кондиционирования, но
  сверка выбранного языка с `Model::capabilities().languages` даст промах. Проверять на живой модели.

### 2.2 GigaAM-v3 — лучшая точность на русском, но **не** стриминг

- Порт `ai-sage/GigaAM-v3` в transcribe.cpp: 4 варианта (`e2e-rnnt`, `e2e-ctc`, `rnnt`, `ctc`),
  общий 16-слойный Conformer 768d + RNN-T или CTC голова, ~180M параметров.
- **Лицензия MIT** и у апстрим-весов (HF API `ai-sage/GigaAM-v3` → `license: mit`), и у кода
  (`salute-developers/GigaAM/LICENSE` — MIT, «Copyright (c) 2024 GigaChat Team»), и у GGUF-репозитория
  `handy-computer/gigaam-v3-e2e-rnnt-gguf`. Чище не бывает.
  ⚠ Историческая ловушка: до 12/2024 GigaAM был под **non-commercial** лицензией, и старые
  ONNX-экспорты это унаследовали — `sherpa-onnx-nemo-ctc-giga-am-russian-2024-10-24` до сих пор
  помечен «It is for non-commercial use only». Артефакты 2024-10-24 не использовать.
- **WER на FLEURS ru, измерено самим transcribe.cpp** (Q8_0, полная выборка 775 реплик, greedy,
  без внешней LM, `BasicTextNormalizer`): `gigaam-v3-e2e-rnnt` **5.36 %**, `gigaam-v3-e2e-ctc` 5.50 %,
  `gigaam-v3-rnnt` 8.08 %, `gigaam-v3-ctc` 8.40 %. Апстрим-пакет на том же манифесте даёт 6.78 %;
  разница объяснена в доке — upstream отбрасывает 5 реплик длиннее 25 с. На 770 общих репликах
  C++ совпадает с апстримом **точно**.
- e2e-варианты дают **кейс и пунктуацию** (1024-piece SentencePiece); не-e2e — lowercase без
  пунктуации, 33-символьный алфавит.
- **Размер:** e2e-rnnt Q8_0 261 MB, Q5_K_M 197 MB, Q4_K_M 175 MB, F16 431 MB. Квантование почти не
  двигает WER (5.35 → 5.42 %).
- **Скорость на Vulkan:** AMD Ryzen 7 PRO 4750U — ru-сэмпл 4.5 с → 202 мс (22×) на Q8_0;
  CPU — 552 мс (8×). Metal M4 Max — 51 мс (88×).
- **Ограничения, важные для диктовки:**
  - `supports_streaming = false` → `session.stream()` вернёт `NOT_IMPLEMENTED`. Только существующий
    самодельный цикл через `session.run()`.
  - **Стриминга нет и в апстриме.** В `salute-developers/GigaAM` нет ни одного вхождения
    `cache|stream|chunk|att_context` в `encoder.py`/`model.py`; issue #18 «streaming inference»
    открыт с 12/2024 без ответа мейнтейнеров. Всё, что называют «streaming GigaAM», —
    это VAD-нарезка офлайнового декода.
  - **Монолингв, только русский** (`arch/gigaam/capabilities.cpp`: «Monolingual Russian»).
    Английский — второй моделью. Отдельная линия `ai-sage/GigaAM-Multilingual` (ru/en/kk/ky/uz)
    существует, но **в transcribe.cpp не портирована**, и её карточка сама пишет «moderate quality
    on English» (FLEURS en 9.4 против 3.9 у Whisper large-v3).
  - Мягкое окно **25 с**: `max_audio_ms = 25000`, дальше WARN и деградация (не отказ).
  - Нет `initial_prompt`, нет temperature-fallback, нет long-form: `apply_family_invariants`
    выставляет только `FEATURE_CANCELLATION`. **Глоссарий и контекст `CONTEXT_CHARS` перестают
    работать.**
  - `max_timestamp_kind = TOKEN` (шаг 40 мс), сегментов нет.

### 2.3 `parakeet-tdt-0.6b-v3` — русский + английский офлайн, ложится в текущий цикл

- 25 европейских языков, включая русский и английский. **Веса CC-BY-4.0** (frontmatter),
  код NeMo — Apache-2.0.
- **Не стриминговая**: `docs/models/parakeet.md` — «Most Parakeet variants here are offline-only.
  For low-latency streaming use … nemotron». Чанкованная инференция в NeMo требует
  `right_context_secs=2.0` — ≥2 с задержки, для живой диктовки неприемлемо.
- ⚠ **Автоопределения языка нет** — хинт обязателен. `DictationLanguage::Auto` (которое сейчас
  маппится в `None`) для этой модели работать не будет.
- **WER на русском (карточка NVIDIA): FLEURS ru 5.51 %, CoVoST2 ru 3.00 %.** Средний FLEURS
  по 25 языкам 11.97 %. LibriSpeech test-clean по измерению transcribe.cpp — 1.94 % (Q8_0).
- Размер: Q8_0 740 MB, Q4_K_M 502 MB. Карточка: «At least 2GB RAM for model to load».
- **Единственное найденное Windows+Vulkan число во всём исследовании** (`docs/build-windows.md`):
  Intel Iris Xe, parakeet-v3 Q8_0, jfk.wav, 1 warmup + 3 iters —
  **Vulkan ≈ 877 мс (12.5× realtime) против CPU ≈ 1621 мс (6.8×)**.
- Как и GigaAM: нет `initial_prompt`, таймстемпы только TOKEN.

### 2.4 Voxtral Realtime (`Voxtral-Mini-4B-Realtime-2602`) — стриминг + русский, но тяжёлый

- **Apache-2.0** и код, и веса. Frontmatter: `en, fr, es, de, ru, zh, ja, it, pt, nl, ar, hi, ko` —
  **русский есть**. Конвертер апстрима перечисляет те же 13 с комментарием «Multilingual streaming
  model; **English-only acceptance gate**, but advertise the full set in the GGUF».
- Нативный стриминг (delayed streams modeling), один текстовый токен на слот 80 мс,
  `num_delay_tokens` 1..15 или 30; дефолт 6 = **480 мс**.
- **WER на русском (FLEURS, таблица карточки Mistral):** 160 мс → 9.53 %, 240 мс → 7.87 %,
  **480 мс → 6.02 %**, 960 мс → 5.56 %, 2400 мс → 5.41 %. Офлайновый Voxtral Mini Transcribe 2.0 —
  4.75 %. Это лучший русский WER среди стриминговых.
- **Цена:** ~4.37 B параметров. GGUF: BF16 8.87 GB, Q8_0 4.73 GB, **Q4_K_M 2.83 GB**.
  Карточка: «can run on a single GPU with >= 16GB memory» (bf16).
- ⚠ В transcribe.cpp стриминговый путь — **только auto-detect языка**
  (`arch/voxtral_realtime/capabilities.cpp`: «Auto-language only»). Зафиксировать `ru` нельзя.
- ⚠ Коммит идёт по `GenericTextAgreement` (voxtral нет в таблице семейств), то есть по совпадению
  3 последних гипотез, а не по родной границе модели.
- Апстрим-бенчи только Metal (M4 Max, 11 с → 1.14–1.22 с ≈ 9–9.7× realtime). Windows+Vulkan —
  [НЕПОДТВЕРЖДЕНО].

### 2.5 Whisper как базовая линия

- `whisper-large-v3-turbo`: **MIT** (и код, и веса). Карточка: «the number of decoding layers have
  reduced from 32 to 4. As a result, the model is way faster, at the expense of a minor quality
  degradation». В анонсе OpenAI (`whisper/discussions/2363`) названы языки с бо́льшей деградацией —
  тайский и кантонский; русский там не упомянут (это отсутствие упоминания, а не цифра).
- Апстрим-порт: turbo Q5_K_M 591 MB, Q8_0 845 MB, WER LibriSpeech test-clean 2.01–2.04 %
  (large-v3 — 1.81–1.86 % при 951 MB–2.88 GB). Русских цифр в апстрим-доках нет.
- Whisper — **единственное семейство с `FEATURE_INITIAL_PROMPT`, `FEATURE_TEMPERATURE_FALLBACK` и
  `FEATURE_LONG_FORM`**. Весь текущий код с глоссарием, контекстом и `no_speech_thold = 0.6`
  держится именно на нём.
- **distil-whisper отпадает**: «Distil-Whisper is only available for **English** speech recognition»;
  русского чекпоинта нет.
- **faster-whisper / CTranslate2 отпадает по Vulkan.** Бенч README измерен на
  **RTX 3070 Ti 8GB, CUDA 12.4 / i7-12700K, 8 потоков, 13-минутное видео, beam_size=5, large-v2** →
  ~2.3× быстрее openai/whisper на fp16. GPU-путь CTranslate2 — **только CUDA (+ROCm)**, Vulkan нет.
  Rust-биндинг (`ct2rs`) существует, но наследует CUDA-only.
- whisper.cpp сам по себе ничего не добавляет (тот же ggml, тот же Vulkan, меньше семейств).
  Одна деталь на заметку: с v1.8.0 flash-attention включён по умолчанию, и есть открытый issue
  #3020 о деградации качества на **неанглийском** при `-fa`.

### 2.6 Что из семейства заведомо не подходит

- **`moonshine-streaming-{tiny,small,medium}` — English-only.** Это отказ, а не частичный ответ:
  `arch/moonshine_streaming/capabilities.cpp` — «English-only model»; апстрим-док — «Non-English
  audio. Not supported on this family». Мультиязычная линия «Flavors of Moonshine» покрывает
  ar/zh/ja/ko/uk/vi/es — **русского нет**; в transcribe.cpp есть 12 языковых файнтюнов
  `moonshine-{tiny,base}-{vi,uk,zh,ko,ar,ja}`, русского среди них тоже нет.
  Плюс легаси-не-английские веса Moonshine идут под non-commercial Moonshine Community License.
  Латентность из статьи v2 (50/148/258 мс для tiny/small/medium) измерена **на Apple M3**;
  «5× быстрее» из v1 — это сокращение **вычислений** против Whisper tiny.en на 10-секундном
  сегменте, а не wall-clock множитель.
- **`canary-1b` — единственная non-commercial модель в репозитории**: `CC-BY-NC-4.0`,
  апстрим помечает её блоком-предупреждением, и `general.license: CC-BY-NC-4.0` зашит в KV
  каждого GGUF. Не брать.
- `canary-1b-v2`: русский есть, CC-BY-4.0, но FLEURS ru **6.90 %** (хуже parakeet-v3), ≥6 GB RAM,
  GGUF апстримом не опубликован, не стриминговая.
- `canary-1b-flash`, `canary-180m-flash` (en/de/es/fr), `canary-qwen-2.5b` (en), `parakeet-tdt-0.6b-v2`,
  `parakeet-unified-en-0.6b`, `nemotron-speech-streaming-en-0.6b`,
  `multitalker-parakeet-streaming-0.6b-v1` — английские.
- `qwen3-asr-{0.6b,1.7b}`: русский в таблице языков есть (`arch/qwen3_asr/model.cpp:430` —
  `{ "ru", "Russian" }`), но `supports_streaming` не выставлен и таймстемпов нет.
  Русских WER-цифр не нашёл — [НЕПОДТВЕРЖДЕНО].
- `sortformer` — диаризация, текста не даёт. Для однопользовательской диктовки бесполезна.

---

## 3. Точность на русском против Whisper

### 3.1 Единственный кросс-модельный русский бенчмарк — Vikhrmodels Russian ASR Leaderboard

HF Space `Vikhrmodels/Russian_ASR_Leaderboard`, результаты в датасете
`Vikhrmodels/russian-asr-leaderboard` (Apache-2.0, последнее обновление **2025-08-31**).
Шесть наборов: Russian_LibriSpeech (1352), Common_Voice_22.0 (10244), Tone_Webinars (21587),
Tone_Books (4930), Tone_Speak (700, синтетический TTS), Sova_RuDevices (5799). WER в долях.

| Модель | Лицензия | Overall | RuLS | CV22 | Webinars | Books | Speak | Sova |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `openai/whisper-large-v3` | Apache-2.0 | **0.1016** | 0.1162 | 0.0751 | 0.0724 | 0.1219 | 0.0274 | 0.1965 |
| `bond005/whisper-podlodka-turbo` | Apache-2.0 | 0.1036 | 0.1191 | 0.0636 | 0.1521 | 0.0896 | 0.0314 | 0.1655 |
| `openai/whisper-large-v3-turbo` | Apache-2.0 | 0.1107 | 0.1188 | 0.0817 | 0.0989 | 0.1329 | 0.0280 | 0.2037 |
| `bond005/whisper-large-v3-ru-podlodka` | Apache-2.0 | 0.1162 | 0.1024 | 0.0780 | 0.1593 | 0.1031 | 0.0323 | 0.2221 |
| `nvidia/canary-1b-v2` | CC-BY-4.0 | 0.1355 | 0.2016 | 0.0912 | 0.1371 | 0.1145 | 0.0497 | 0.2189 |
| `VOSK-model-ru-0.42` | Apache-2.0 | 0.1396 | 0.1206 | 0.1187 | 0.2729 | 0.1080 | 0.0261 | 0.1915 |
| `GigaAM-ASR-V2-RNNT` | MIT | 0.1821 | **0.0524** | **0.0285** | 0.8003 | 0.0806 | 0.0308 | **0.1001** |
| `GigaAM-ASR-V2-CTC` | MIT | 0.1874 | 0.0526 | 0.0342 | 0.8019 | 0.0772 | 0.0301 | 0.1286 |

Что здесь важно и что легко прочитать неправильно:

- Это **единственная найденная прямая цифра large-v3 против large-v3-turbo на русском одной
  харнесой**: 0.1016 против 0.1107 overall, 0.0751 против 0.0817 на Common Voice 22.
  То есть turbo на русском **хуже**, примерно на 0.9 п.п. Другого первичного источника, который
  сравнивал бы v3 и turbo на русском, нет.
- 🚩 **Колонку Overall у GigaAM читать нельзя.** GigaAM выигрывает 4 набора из 6 с большим отрывом
  (CV22 2.85 % против 7.51 % у Whisper), но получает **80 % WER на Tone_Webinars**, что в одиночку
  ломает среднее. Это почти наверняка артефакт 25-секундного лимита: `.transcribe` у GigaAM обрезает
  длинное аудио, если не гнать `transcribe_longform` с VAD, а вебинарные клипы длинные.
  **Для короткой диктовки этот ряд нерелевантен, и GigaAM в ней явно впереди.**
- Лидерборд отстал примерно на год: нет GigaAM v3, T-one, Vosk 0.54, Parakeet v3, nemotron-3.5.
- **HF Open ASR Leaderboard русский не покрывает вообще**: в `constants.py` список языков —
  ровно `de, fr, it, es, pt, hi, nl`. Подлежащий датасет
  `hf-audio/open-asr-leaderboard-multilingual-datasets` метку `language:ru` несёт, но лидерборд её
  не рендерит.

### 3.2 Вендорские таблицы — каждая со своим интересом

**GigaAM `evaluation.md`** (Sber). «Whisper» здесь = large-v3 с постобработкой (снятие пунктуации
и регистра, замена числительных). WER, %:

| Набор | V3 CTC | V3 RNNT | V2 CTC | V2 RNNT | T-One + LM | Whisper large-v3 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Golos Farfield | 4.5 | 3.9 | 4.3 | 4.0 | 12.2 | 16.4 |
| Golos Crowd | 2.8 | 2.4 | 2.5 | 2.3 | 5.7 | 19.0 |
| Russian LibriSpeech | 4.7 | 4.4 | 5.2 | 5.2 | 6.2 | 9.4 |
| Common Voice 19 | 1.3 | 0.9 | 1.5 | 0.9 | 5.2 | 5.5 |
| Callcenter | 10.3 | 9.5 | 13.6 | 12.9 | 13.5 | 23.1 |
| OpenSTT Phone Calls | 18.6 | 17.4 | 20.7 | 19.8 | 19.8 | 27.4 |
| OpenSTT Youtube | 11.6 | 10.6 | 13.9 | 13.0 | 21.9 | 17.8 |
| OpenSTT Audiobooks | 8.7 | 8.2 | 10.8 | 10.3 | 13.4 | 14.3 |
| **Среднее** | **9.1** | **8.3** | 11.1 | 10.6 | 16.3 | 21.0 |

⚠ GigaAM обучался в основном на Golos/OpenSTT — эти строки внутридистрибутивные, то есть верхняя
оценка. Отдельно карточка `ai-sage/GigaAM-v3` заявляет счёт `70:30` против Whisper large-v3, но
судьёй там **Gemini 2.5 Pro как LLM-as-a-Judge** на 500 случайных сэмплах — это предпочтение
модели-судьи, а не WER.

**T-one README** (Т-Банк), WER %, телефонный уклон:

| Категория | T-one (71M) | GigaAM-RNNT v2 | Vosk-ru 0.54 | Vosk-small-streaming-ru 0.54 | Whisper large-v3 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Call-center | **8.63** | 10.22 | 11.28 | 15.53 | 19.39 |
| Другая телефония | **6.20** | 7.88 | 8.69 | 13.49 | 17.29 |
| Именованные сущности | **5.83** | 9.55 | 12.12 | 17.65 | 17.87 |
| CommonVoice 19 | 5.32 | **2.68** | 6.22 | 11.3 | 5.78 |
| OpenSTT calls_2_val relabeled | **7.94** | 11.14 | 13.22 | 21.03 | 20.82 |

**gigastt `docs/benchmarks.md`** — единственная таблица, где **все движки прогнаны одной харнесой**
(Apple M1, CPU, int8/greedy, 1000 сэмплов на домен, отказ = 100 % WER, 95 % bootstrap CI,
общие манифесты и нормализация), и единственная, где рядом с WER стоят RTF, диск и RAM.
Автор одиночный и сам является предметом измерения — взвешивать соответственно, но методологически
это самая аккуратная из трёх. Оттуда: GigaAM v3 rnnt 4.08 % far-field / 18.50 % phone / RTF 0.10;
Vosk 0.54 2.97 % clean / 6.29 % far-field / RTF ~0.03; whisper.cpp large-v3 15.26 % clean /
17.91 % far-field / RTF 0.36–0.77; faster-whisper large-v3 RTF > 1.0 на CPU.

### 3.3 Академическая точка отсчёта

Whisper **large-v2**, FLEURS Russian — **5.6 %** (OpenAI, arXiv 2212.04356, Appendix D.2.4,
Table 13; ряд по размерам: tiny 31.1 / base 20.5 / small 11.4 / medium 7.2 / large 6.4 / large-v2 5.6).
**Для large-v3 и large-v3-turbo первичного FLEURS ru не опубликовано** — статья старше v3, карточки
per-language таблиц не содержат, анонс turbo даёт только графики.
Статья Voxtral Realtime приводит строку «Whisper 5.13 %» на FLEURS ru, но чекпоинт не называет —
[НЕПОДТВЕРЖДЕНО, какой именно Whisper].

### 3.4 Сводка по FLEURS ru — и почему это не рейтинг

| Модель | FLEURS ru WER | Кто мерил |
| --- | ---: | --- |
| GigaAM-v3-e2e-rnnt Q8_0 | 5.36 % | transcribe.cpp, `BasicTextNormalizer` + `jiwer` |
| Voxtral Realtime @ 2400 мс | 5.41 % | Mistral, карточка |
| parakeet-tdt-0.6b-v3 | 5.51 % | NVIDIA, карточка |
| Voxtral Realtime @ 960 мс | 5.56 % | Mistral, карточка |
| Whisper **large-v2** | 5.6 % | OpenAI, статья, Table 13 |
| Voxtral Realtime @ 480 мс | 6.02 % | Mistral, карточка |
| canary-1b-v2 | 6.90 % | NVIDIA, карточка |
| Voxtral Realtime @ 240 мс | 7.87 % | Mistral, карточка |
| nemotron-3.5 streaming @ 1.12 с | 9.17 % | NVIDIA, карточка (LangID) |
| nemotron-3.5 streaming @ 320 мс | 9.87 % | NVIDIA, карточка (LangID) |
| nemotron-3.5 streaming @ 80 мс | 10.84 % | NVIDIA, карточка (LangID) |

**Это не ранжирование.** Нормализаторы и харнесы разные: transcribe.cpp применяет
`BasicTextNormalizer` из `whisper_normalizer` через `jiwer` (`docs/tools/wer.md`), NVIDIA — свою
и сама признаёт её неидеальность, Mistral методику не раскрывает, OpenAI мерил на своём наборе
семь лет назад. Разница в 0.2–0.5 п.п. между соседними строками ничего не означает.
**Контролируемого прогона Whisper и альтернатив одной харнесой на русском FLEURS не существует ни
в одном первичном источнике** — в апстриме transcribe.cpp есть `scripts/wer/`, но каталог
`reports/wer/` не закоммичен.

Содержательный вывод, который таблица всё же поддерживает: **у стриминговых моделей WER на русском
заметно хуже офлайновых** (9.2–10.8 % у nemotron против 5.4–5.5 % у GigaAM/parakeet-v3), и на
русском эта плата больше, чем на испанском (4.11 %) или итальянском (4.25 %) у той же nemotron.
FLEURS — читаная студийная речь; как это переносится на диктовку в микрофон с шумом, не говорит
ни один источник.

---

## 4. Что находится за пределами 0.2.3

- **Официальный Rust-крейт `sherpa-onnx`** (crates.io, **v1.13.8, 2026-09-11**, Apache-2.0,
  владелец — мейнтейнер sherpa-onnx, 377k загрузок). Линкуется **статически по умолчанию**: если
  `SHERPA_ONNX_LIB_DIR` не задан, build-script скачивает готовый архив под платформу, для Windows
  x64 есть и static-MT, и shared-MT сборки. `OnlineModelConfig` уже содержит `transducer`,
  `zipformer2_ctc`, `nemo_ctc` и **`t_one_ctc`**. Это самый дешёвый путь к стриминговым
  не-ggml-моделям из Rust.
  ⚠ **Готовые Windows-архивы — CPU-only.** GPU на Windows означает самосборку с
  `-DSHERPA_ONNX_ENABLE_DIRECTML=ON` (или `_GPU=ON` под CUDA) и `SHERPA_ONNX_LIB_DIR` на свою сборку.
  Vulkan там нет вообще. Для моделей 130–230 MB int8 при RTF 0.06–0.38 CPU — вменяемый дефолт.
  Есть и сторонний `sherpa-rs` (MIT, v0.6.8), но он теперь избыточен.
- **T-one** (voicekit-team/T-one, `t-tech/T-one`) — **Apache-2.0 и код, и веса**, настоящий
  стриминг: аудио режется на **300 мс** сегментов, Conformer получает сегмент плюс скрытое
  состояние предыдущего и возвращает новое состояние + логпробы кадров; свой сплиттер ищет границы
  фраз по N подряд неречевым кадрам. 71M параметров, `model.onnx` 138 MB, доступен из Rust через
  `OnlineModelConfig.t_one_ctc`.
  **Только русский** (алфавит `tokens.txt` — русские буквы + пробел + blank), **английского нет
  вообще**, и модель заточена под телефонию **8 кГц**. Для диктовки в качественный микрофон это
  сильное несовпадение домена.
- **Vosk**: код Apache-2.0; **русские модели тоже Apache-2.0** — `vosk-model-ru-0.42` (1.8 GB) и
  `vosk-model-small-ru-0.22` (45 MB); ограничительные лицензии в каталоге Vosk приходятся на другие
  языки (AGPL у `en-us-daanzu`, CC-BY-NC-SA у `small-fr-pguyot`, LGPL у zamia-моделей).
  Нативный стриминг — это его архитектура (`accept_waveform` + `partial_result`).
  Страница моделей: маленькие модели «requires about 300 Mb of memory in runtime», большие —
  «up to 16 Gb in memory».
  🚩 **Практический блокер на Windows**: крейт `vosk` 0.3.1 (2024-10-27, MIT) линкуется
  **динамически** к `libvosk`, а последний Windows-бандл в релизах vosk-api — `vosk-win64-0.3.45.zip`
  от **2022-12-14**; у релиза 0.3.50 ассетов нет вовсе. То есть это 2022-й DLL.
  Более новые Zipformer2-модели 0.54 (`alphacep/vosk-model-ru`, CV-ru WER 6.1;
  `alphacep/vosk-model-small-streaming-ru`, CV-ru WER 11.3, encoder.int8 26.2 MB) существуют на HF
  под Apache-2.0, но в документации sherpa-onnx как streaming-transducer не запакованы —
  [НЕПОДТВЕРЖДЕНО, что они грузятся в `OnlineTransducerModelConfig`].
- **`gigastt` / `gigastt-core`** (crates.io v2.20.0, 2026-09-03, MIT) — GigaAM v3 нативно в Rust
  через `ort`/ONNX Runtime, есть prebuilt под Windows x86_64 CPU, `gigastt-core` — обычная
  библиотека без серверных зависимостей. GPU: CoreML (macOS), CUDA (**только Linux x86_64**),
  NNAPI. **Windows GPU нет.** Автор честно пишет, что «стриминг» там — буферизация поверх
  офлайнового RNN-T: «TTFP p50 ~0.82 s (far-field) / ~1.65 s (crowd)… streaming WER is ~11–15 pp
  worse than the same files over REST batch». Один мейнтейнер, 1230 загрузок — риск по bus factor,
  но MIT позволяет вендорить.
- **NVIDIA/NeMo-Speech.cpp** — официальный нативный рантайм NVIDIA на ggml, **Apache-2.0**,
  с Vulkan-бэкендом и сборкой под Windows, стабильными C-заголовками и CMake-пакетом
  (`find_package(NeMoSpeech ... COMPONENTS ASR)`). Снимает аргумент «NeMo — только Python».
  Но даёт то же подмножество моделей, что уже доступно через `transcribe-cpp` 0.2.3,
  Rust-биндинга не имеет, бенчей не публикует. Смысл только ради VAD/пунктуатора/эндпойнтинга
  из коробки.
- **sherpa-onnx по моделям**: даёт **либо** русский, **либо** стриминг — кроме T-one.
  Стриминговые cache-aware FastConformer-экспорты только английские; русский есть в офлайновых
  GigaAM v2 (231 MB), parakeet-tdt-0.6b-v3 int8 (640 MB) и
  `nemo-fast-conformer-ctc-be-de-en-es-fr-hr-it-pl-ru-uk-20k`. Открытые issue #2177, #2918, #3573
  подтверждают, что ONNX-экспорт cache-aware моделей отстаёт (нужны `cache_last_channel`,
  `cache_last_time`, `cache_last_channel_len`). GigaAM v3 лежит на HF
  (`csukuangfj/sherpa-onnx-nemo-transducer-giga-am-v3-russian-2025-12-16`), но не в доках и не в
  теге релиза — issue #3619 без ответа.
- **Kyutai STT** отпадает дважды: моделей всего две (`stt-1b-en_fr`, `stt-2.6b-en`), **русского
  нет**; и бэкенд Rust-реализации — Candle, у которого **нет Vulkan/WGPU**, только CUDA/Metal/WASM.
- **`antony66/whisper-large-v3-russian`**: на карточке и в метаданных HF **лицензия не указана
  вообще**. Считать неподходящим для распространения, пока автор не заявит лицензию.
  Более чистая альтернатива того же класса — `bond005/whisper-podlodka-turbo` (Apache-2.0, ru+en),
  и она реально присутствует в Vikhrmodels-лидерборде (overall 0.1036, лучше turbo).
- **Открытого русского ASR от Яндекса не найдено** — [НЕПОДТВЕРЖДЕНО, что он существует].
- **Лицензионная заметка.** MIT, Apache-2.0, Unlicense — вопросов нет. **CC-BY-4.0 (parakeet,
  canary, NeMo fastconformer) — это лицензия на *веса*, не на код, и её нет в списке
  GPL-совместимых лицензий FSF.** Авторитетного заявления о совместимости с GPLv3 найти не удалось —
  [НЕПОДТВЕРЖДЕНО]. Практически это attribution-only, но если веса поедут в репозиторий, стоит
  посмотреть отдельно. То же и для OpenMDW-1.1 у nemotron-3.5.

---

## 5. Цена переключения в `crates/dictation/src/dictation.rs`

Whisper-специфичного в движке больше, чем кажется. По убыванию стоимости:

1. **`RunExtension::Whisper(WhisperRunOptions { … })` исчезает.** `initial_prompt` (глоссарий +
   `CONTEXT_CHARS` хвоста Confirmed Text), `temperature`/`temperature_inc = 0.0`,
   `no_speech_thold = 0.6`, `condition_on_prev_tokens = false` — всё это есть только у whisper
   (`Feature::InitialPrompt` / `TemperatureFallback` выставляются только в
   `arch/whisper/capabilities.cpp`). `Transcriber::segments_after` теряет смысл целиком.
   **Глоссарий придётся перенести в Post-processing** — что, кстати, согласуется с ADR 0001:
   движок не судит слова.
2. **Со стриминговой моделью самодельный цикл заменяется на `Stream`.**
   `WINDOW`/`STEP`/`SETTLE`/`MIN_AUDIO`/`BOUNDARY_SEARCH`, `segments_to_confirm`, `quietest_point`,
   `Transcript::commit_point` существуют только потому, что whisper не стримит.
   `Stream::feed` + `StreamText { committed, tentative }` даёт ту же пару Confirmed/Pending
   готовой, `stable_prefix_agreement_n` играет роль `SETTLE`. Это **упрощение**, но `live_loop`
   переписывается целиком.
3. **Speech Gate (ADR 0002) остаётся, но с другой ролью.** Он решает, запускать ли декодер;
   у cache-aware стриминга декодер идёт непрерывно и в константной памяти. Гейт стоит сохранить
   для отсечения тишины на старте/стопе и для выбора момента `Stream::finalize`, но его
   экономическое обоснование («не гонять декодер на пустом буфере») исчезает.
4. **Decoder Loop guard (ADR 0001) — под вопросом.** `is_decoder_loop` ловит повторы n-грамм
   авторегрессивного декодера. У RNN-T/CTC (gigaam, parakeet, nemotron) этого класса отказов нет
   по конструкции; у Voxtral (LLM-декодер) — есть. Guard структурный и вреда не несёт, но на
   трансдьюсерах становится мёртвым кодом.
5. **`segments` → `tokens`/`words`.** `max_timestamp_kind`: whisper = SEGMENT,
   parakeet/gigaam = TOKEN, voxtral/moonshine/canary/sensevoice/qwen3 = NONE. Любой код, читающий
   `transcript.segments[i].{t0_ms,t1_ms}`, надо пересобрать — а при переходе на `Stream` он в
   основном не нужен.
6. **Загрузчик и настройки.** `WHISPER_MODELS` в `engine_download.rs` завязан на `ggml-<name>.bin`
   с SHA1 из README whisper.cpp; GGUF живут в `huggingface.co/handy-computer/<variant>-gguf` и
   потребуют своего списка. `DictationLanguage` в `crates/settings_content/src/agent.rs` — это
   99 языков Whisper; у любой другой модели список у́же, и его надо сверять с
   `Model::capabilities().languages` (наполняется из GGUF KV `general.languages`).
   Отдельно: у parakeet-v3 **нет автоопределения языка**, а у nemotron-3.5 `general.languages`
   содержит локали (`ru-RU`), а не короткие коды.
7. **Что НЕ меняется:** `init_backends` и `BACKENDS_ARCHIVE` (тот же
   `transcribe-native-0.2.3-windows-x86_64-cpu-vulkan.tar.gz`), захват микрофона 16 кГц моно f32,
   `Backend::Auto`, `SessionOptions`, структура `DictationEvent`, весь Post-processing.

---

## 6. Рекомендация

_Свежесть проверена 2026-09-12 (см. §8): новее разобранного у NVIDIA ничего нет, и `transcribe-cpp` после 0.2.3 не выпускался. Список ниже актуален._

1. **Сначала — `nemotron-3.5-asr-streaming-0.6b` Q8_0 (716 MB) через `Stream` API.**
   Единственный вариант, который даёт настоящий стриминг **и** русский **и** английский **и**
   укладывается в уже пришпиленную `transcribe-cpp = "0.2.3"`. Начинать с `att_context_right`,
   соответствующего 320 мс lookahead: FLEURS ru 9.87 % против 9.17 % на 1.12 с — треть задержки за
   0.7 п.п. Язык задавать явно, не полагаясь на auto (auto стоит ~1.5 п.п.).
   Риск: WER на русском примерно вдвое хуже офлайновых моделей, и апстрим его не валидировал.
   Выбор дополнительно подтверждён со стороны NVIDIA: её собственный нативный рантайм
   `NeMo-Speech.cpp` (Apache-2.0, push 2026-09-10, «day-0 support for the latest models») из всех
   ASR-моделей поддерживает ровно nemotron-3.5, nemotron-en, parakeet-tdt-0.6b-v3 и
   parakeet-ctc-1.1b — ничего свежее у неё просто нет (§8.6).
2. **Параллельно — GigaAM-v3-e2e-rnnt Q8_0 (261 MB) как «финализатор» в существующем цикле.**
   MIT и веса, и код; лучшая измеренная точность на русском (FLEURS ru 5.36 %, и в собственной
   таблице Sber среднее 8.3 % против 21.0 % у Whisper large-v3); крошечный; 22× realtime на слабой
   Vulkan-iGPU; кейс и пунктуация на выходе. Не стримит — годится ровно туда, где сейчас
   вызывается `recognize_final`: на закрытую гейтом фразу и на хвост при стопе. Ограничение 25 с
   в диктовке почти не мешает, потому что гейт режет по паузам. Английский — отдельной моделью.
3. **Третьим — `parakeet-tdt-0.6b-v3` Q8_0 (740 MB)**, если нужен один файл на оба языка без
   стриминга: FLEURS ru 5.51 %, CC-BY-4.0, и единственное найденное Windows+Vulkan число
   (12.5× realtime на Intel Iris Xe). Ложится в текущий `live_loop` почти без изменений структуры,
   но требует обязательного хинта языка.
4. **Whisper large-v3 оставить как fallback, и не менять его на turbo ради русского.**
   Единственный прямой замер одной харнесой (Vikhrmodels) даёт turbo **хуже** на русском:
   0.1107 против 0.1016 overall. Плюс только у whisper есть `initial_prompt` под глоссарий и
   99 языков.
5. **Voxtral Realtime — только если не жалко 2.8–4.7 GB и есть сильная GPU.** Лучший из
   стриминговых по русскому WER (6.02 % @ 480 мс), Apache-2.0 целиком, но в transcribe.cpp у него
   auto-detect без возможности зафиксировать `ru`, и коммит идёт по generic-агрименту.
6. **Новая зависимость не нужна.** Она понадобилась бы только ради T-one (единственный настоящий
   русский стриминг вне ggml — но русский-только и телефонный 8 кГц), Vosk (2022-й Windows DLL)
   или sherpa-onnx (Windows-prebuilt только CPU). Ни один из них не даёт
   «стриминг + русский + английский + Vulkan на Windows + Rust» одновременно, а nemotron-3.5
   даёт — внутри уже имеющейся зависимости.

---

## 7. Открытые вопросы — решаются только локальным замером

- **Time-to-first-partial на конкретной машине.** Ни один источник не публикует single-stream
  латентность на потребительской Vulkan-GPU под Windows. Всё, что есть: Metal/M4 Max,
  датацентровые H100/L40S (и то throughput, а не интерактивная задержка), Linux+RADV и одна строка
  про Intel Iris Xe. Мерить `Stream::feed` → первое непустое `StreamText::tentative`.
- **Стоимость первого запуска на Vulkan.** `docs/build-windows.md`: «A single `transcribe-cli`
  invocation pays a large one-time cost on Vulkan: the backend compiles its compute pipelines
  (SPIR-V → device shaders)… Warm steady-state is the real number». Для диктовки это значит
  прогревать модель при старте Zed. Сколько именно — неизвестно.
- **Свёртки на Vulkan.** `docs/porting/ggml-reference-map.md`: «When a backend lacks an F32 path for
  depthwise / pointwise conv (historically Vulkan), route through `conv_1d_f32` / `conv_2d_dw_f32`…»,
  и в troubleshooting: «Conv path wrong on Vulkan only → … force the im2col path with
  `TRANSCRIBE_CONV_NO_DIRECT_PW=1`». И GigaAM, и parakeet — Conformer-ы, то есть depthwise-conv
  тяжёлые. Если качество на Vulkan разойдётся с CPU, первый подозреваемый — здесь.
  Апстрим Vulkan-на-Windows в CI прогоняет только сборку и линковку, не инференс.
- **Грузится ли GGUF от `handy-computer` библиотекой 0.2.3.** Доки апстрима живут на `main`,
  который новее релиза. Проверяется одной загрузкой + `model.capabilities()`.
- **Код языка для nemotron-3.5**: пройдёт ли `ru` (а не `ru-RU`) через `resolve_prompt_id`,
  и что вернёт `capabilities().languages`.
- **Реальный WER на русской диктовке в микрофон.** FLEURS и CV — читаная речь; телефонные наборы
  у T-one и Golos у GigaAM — не наш домен. Нужны свои фикстуры; в `.scratch/voice-dictation-*`
  они уже есть.
- **Поведение `CommitPolicy::Auto` на parakeet** (`FamilyNativeCommit`) против нынешнего
  `segments_to_confirm` + `quietest_point`: как часто растёт Confirmed Text и не станет ли он
  дёрганее текущего.
- **Смешанная ru/en речь.** Nemotron auto-detect ставит тег языка и может переключиться внутри
  сессии; GigaAM на английском выдаст мусор. Как это выглядит в живой диктовке — не проверял никто.

---

## 8. Проверка на свежесть: есть ли у NVIDIA что-то новее (проверено 2026-09-12)

Проверялось по датам публикации, а не по ощущению новизны. Источники: HF API по организации
`nvidia` (сортировка `createdAt` и отдельно `lastModified`, newest first), GitHub-релизы
`NVIDIA/NeMo`, README `NVIDIA/NeMo-Speech.cpp`, crates.io API и diff `v0.2.3...main` апстрима
transcribe.cpp.

### 8.1 Короткий ответ

**Нет, ничего новее уже разобранного нет.** `nemotron-3.5-asr-streaming-0.6b` создан
**2026-05-15** и остаётся самой свежей ASR-моделью NVIDIA на сегодня. Это именно «нет», а не
«не нашёл»: полный листинг организации `nvidia`, отсортированный по дате создания, покрыт до
2025-12-17 включительно, то есть **весь 2026 год просмотрен целиком**, и в интервале
2026-05-15 → 2026-09-12 у NVIDIA не появилось ни одной новой ASR-модели.

### 8.2 Хронология семейств NVIDIA ASR (дата создания репозитория на HF)

| Дата | Модель | Что это | Отношение к отчёту |
| --- | --- | --- | --- |
| 2026-08-24 | `Nemotron-3-Diarization-preview` | диаризация/VAD, streaming-sortformer | **новее**, но текста не даёт |
| 2026-07-29 | `NVIDIA-NemotronLabs-VoiceChat-11B` | дуплексный голосовой чат, 11B | **новее**, но не ASR-для-диктовки |
| **2026-05-15** | **`nemotron-3.5-asr-streaming-0.6b`** | cache-aware streaming RNN-T, 40 локалей | **самая свежая ASR; разобрана в §2.1** |
| 2026-04-07 | `parakeet-unified-en-0.6b` | buffered streaming | английский |
| 2026-01-15 | `parakeet-ctc-0.6b-Vietnamese` | вьетнамский файнтюн | нерелевантно |
| 2025-12-17 | `nemotron-speech-streaming-en-0.6b` | cache-aware streaming, английский родитель 3.5 | английский |
| 2025-10-22 | `diar_streaming_sortformer_4spk-v2.1` | диаризация | текста не даёт |
| 2025-10-15 | `multitalker-parakeet-streaming-0.6b-v1` | streaming, многоголосый | английский |
| 2025-10-10 | `parakeet_realtime_eou_120m-v1` | streaming + EOU, 120M | **только английский**, см. §8.4 |
| 2025-08-04 | `canary-1b-v2` | AED, 25 языков | разобрана в §2.6 |
| **2025-08-04** | **`parakeet-tdt-0.6b-v3`** | офлайн, 25 европейских языков | **разобрана в §2.3** |
| 2025-06-26 | `canary-qwen-2.5b` | SALM | английский |
| 2025-04-15 | `parakeet-tdt-0.6b-v2` | офлайн | английский |
| 2025-03-07 / 03-11 | `canary-1b-flash`, `canary-180m-flash` | AED | en/de/es/fr |
| 2024-02-07 | `canary-1b` | AED | **CC-BY-NC-4.0**, не брать |
| 2023-12-27/28 | `parakeet-{ctc,rnnt}-{0.6b,1.1b}` | офлайн | английский |

`parakeet-tdt-0.6b-v4` на HF **не существует** (поиск даёт ноль результатов).
Правки `lastModified` у старых репозиториев (например `parakeet-tdt-0.6b-v3` — 2026-08-05,
`canary-1b-v2` — 2026-08-31) — это обновления карточек и метаданных, новых чекпоинтов за ними нет:
ни один ASR-репозиторий `nvidia` не был **создан** после 2026-05-15.

### 8.3 Две новинки после 2026-05-15 — обе мимо задачи

- **`nvidia/Nemotron-3-Diarization-preview`** (создан 2026-08-24, обновлён 2026-09-10).
  Pipeline `voice-activity-detection`, теги `speaker-diarization`, `streaming-sortformer`.
  **Транскрипта не производит вообще** — как и Sortformer из §2.6, выдаёт только сегменты
  говорящих. Языковой список в карточке отсутствует.
  Лицензия — `nvidia-software-and-model-evaluation-license`, то есть **evaluation-only**.
  Это блокер сам по себе, безотносительно GPL.
  Архитектурно это Sortformer-линия; `arch/sortformer` в `transcribe-cpp-sys` 0.2.3 есть, но
  порта именно этого превью в апстриме нет. Для диктовки бесполезно.
- **`nvidia/NVIDIA-NemotronLabs-VoiceChat-11B`** (создан 2026-07-29, обновлён 2026-09-11).
  Дуплексный речевой ассистент (STT + LLM + TTS) поверх `Nemotron-Nano-9B-v2`, 11B параметров,
  лицензия OpenMDW-1.1, **`language: ['en']` — только английский**. Это не движок диктовки,
  и такой архитектуры в `transcribe-cpp-sys` 0.2.3 нет. Новая модель на новой архитектуре,
  которой нет в биндинге, для нас бесполезна.

### 8.4 `parakeet_realtime_eou_120m-v1` — не новее, но объясняет упоминания «Parakeet EOU»

Создан **2025-10-10**, то есть **старше** и nemotron-3.5, и parakeet-tdt-0.6b-v3.
В первый заход не попал, потому что у него не выставлен pipeline-тег ASR.

- Cache-aware streaming FastConformer, 17 слоёв энкодера, attention context `[70, 1]`, RNN-T,
  **120M параметров**, задержка «80ms~160 ms», плюс токен `<EOU>` для детекта конца реплики.
- Карточка прямым текстом: «The model supports only English and does not output punctuation or
  capitalization». `language: ['en']`. **Русского нет, пунктуации нет.**
- Лицензия весов — NVIDIA Open Model License (не OpenMDW).
- В transcribe.cpp **не портирован**: в `docs/models/` такого варианта нет.

### 8.5 NeMo Speech 3.0 (2026-08-07) — релиз тулкита, а не моделей

`NVIDIA/NeMo` выпустил **v3.0.0 «NVIDIA NeMo Speech 3.0» 2026-08-07** (предыдущий — v2.7.3
от 2026-04-23). Это первый мажор после разделения репозитория: «removed 800k deprecated LOC»,
переезд на uv, чистка зависимостей.

По ASR там инфраструктура, а не публичные чекпоинты: «Unified prompt-model support now covers
multilingual ASR and streaming inference», «Streaming ASR gained Unified RNNT inference, batched
streaming beam search… and Canary streaming policies», «New and updated model families include
prompt Parakeet Hybrid RNNT/CTC, ASR EOU models, Streaming Sortformer, Canary2 with NFA».
**Ни у «Canary2», ни у «prompt Parakeet Hybrid» публичного чекпоинта на HF нет** — поиск по
`canary2` даёт только посторонние репозитории третьих лиц. Это возможности фреймворка для
самостоятельного обучения, а не то, что можно скачать и запустить.

### 8.6 NVIDIA сама считает nemotron-3.5 самой свежей

`NVIDIA/NeMo-Speech.cpp` (Apache-2.0, создан 2026-07-15, последний push **2026-09-10**) —
официальный нативный C++/ggml рантайм NVIDIA, который в README обещает «providing day-0 support
for the latest models». Его таблица поддерживаемых моделей на сегодня:

> Speech recognition | Nemotron 3.5 ASR Streaming 0.6B, Nemotron Speech Streaming 0.6B,
> Parakeet TDT 0.6B v3, Parakeet CTC 1.1B

Ровно те же модели, что разобраны в §2.1–2.3. Если бы у NVIDIA было что-то новее и пригодное для
локального запуска, оно стояло бы здесь первым.

### 8.7 `transcribe-cpp` после 0.2.3 — ничего не вышло

- crates.io API: `newest_version = 0.2.3`, `max_stable_version = 0.2.3`, опубликована
  **2026-08-30**. История: 0.1.0 (2026-06-30) → 0.1.3 (2026-07-12) → 0.2.0 (2026-08-17) →
  0.2.1 (08-20) → 0.2.2 (08-24) → **0.2.3 (08-30)**. Ничего после.
- Апстрим `main` опережает тег `v0.2.3` ровно на **два коммита**: `e2f82cb6` «patch offline voxtral»
  (2026-09-02) и `92fc36d4` «update descriptions» (2026-09-10). Затронуто 20 файлов, из них
  исходников — только `src/arch/voxtral_realtime/{capabilities,model}.cpp` и
  `include/transcribe/voxtral_realtime.h`. **Ни одной новой директории в `src/arch/`, ни одного
  нового семейства, никаких изменений стримингового API.** 18 архитектур как были, так и остались.
- GGUF-порт `handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf` опубликован **2026-06-07** —
  за два с половиной месяца **до** релиза крейта 0.2.3 (2026-08-30). Дополнительный аргумент
  в пользу того, что 0.2.3 его переваривает, хотя прямой проверкой остаётся загрузка.

### 8.8 Вывод

На 2026-09-12 `nemotron-3.5-asr-streaming-0.6b` (2026-05-15) — самая новая ASR-модель NVIDIA,
`parakeet-tdt-0.6b-v3` (2025-08-04) — самая новая мультиязычная офлайновая, и обе уже разобраны.
Всё, что NVIDIA выпустила после мая 2026, — это диаризация под evaluation-only лицензией и
англоязычный дуплексный voice-chat на 11B. **Рекомендация из §6 не меняется.**
