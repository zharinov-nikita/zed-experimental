# 01 — Поля настроек диктовки

Спецификация: `.scratch/voice-dictation/spec.md`. Словарь: `CONTEXT.md`.

**What to build:** Настройки `agent.dictation` получают всё, что нужно следующим тикетам, и диктовка после сборки работает как раньше. Появляются флаги `keep_model_loaded` (по умолчанию включён) и `save_last_recording` (по умолчанию выключен). Язык становится перечислением всех языков Whisper плюс `auto`, сериализуется кодом ISO, который ждёт Whisper, показывает английское название языка и годится для выпадающего списка в Settings UI (те же derive, что у других перечислений с выпадающим списком). Умолчание языка становится English. Промпт Post-processing по умолчанию заменяется коротким, из спецификации, с `${glossary}` и `${output}`. В личный `settings.json` этой машины добавляется явный `"language": "ru"`.

**Blocked by:** None — can start immediately.

**Status:** done (коммит 9ffac5665b; ручная проверка диктовки в dev-сборке не выполнялась)

- [x] `agent.dictation.keep_model_loaded` и `agent.dictation.save_last_recording` читаются из настроек с указанными умолчаниями.
- [x] Язык принимает только значения из перечисления; `"ru"`, `"en"`, `"auto"` проходят round-trip через JSON; движок получает код языка, а не название.
- [x] Умолчания в `assets/settings/default.json`: язык `en`, короткий промпт из спецификации.
- [x] Тесты разбора `agent.dictation` в `agent_settings`: новые поля, round-trip языка, наличие обоих плейсхолдеров в промпте по умолчанию.
- [x] Пользовательский `settings.json` содержит `"language": "ru"` под `agent.dictation`. Распознавание русского в dev-сборке вручную не проверено.
- [x] `./script/clippy` и тесты `agent_settings`, `settings_content` зелёные.
