# 06 — Подстраница Settings > AI > Dictation, поля движка

Спецификация: `.scratch/voice-dictation/spec.md`. Словарь: `CONTEXT.md`. Образцы вёрстки внутри `settings_ui`: подстраницы Sandbox (список путей с добавлением и удалением) и LLM Providers (ссылка из секции General).

**What to build:** В окне Settings на странице «AI» в секции «General» появляется ссылка «Dictation» рядом с «LLM Providers», с поисковыми синонимами (dictation, voice, whisper, microphone, speech). Подстраница показывает, в порядке: путь к модели Whisper (текст), папка бэкендов (текст), язык (выпадающий список из перечисления тикета 01), микрофон (существующее поле `audio.input_audio_device`), Glossary (один термин в строке, поле добавления, кнопка удаления у каждой строки), переключатели: звуки, «Keep model loaded», «Save last recording». Каждое поле читает и пишет `settings.json` стандартным путём обновления файла настроек; `settings.json` остаётся единственным хранилищем. Блок Post-processing на подстранице делается в тикете 07.

**Blocked by:** 01 — Поля настроек диктовки.

**Status:** implemented, not verified by hand (коммит 147e2c0ea2) — код написан, clippy и тесты зелёные; пункты ниже без галочки требуют ручной проверки в dev-сборке

- [ ] Ссылка «Dictation» видна в Settings > AI > General и находится поиском по «whisper» и «microphone».
- [ ] Правка каждого поля на подстранице меняет `settings.json` под `agent.dictation` (микрофон под `audio.input_audio_device`) и наоборот: правка JSON отражается на подстранице.
- [ ] Glossary: добавление и удаление терминов по одному, порядок сохраняется.
- [x] Только нативные компоненты `ui` и уже существующие компоненты `settings_ui`.
- [x] `./script/clippy` зелёный.
