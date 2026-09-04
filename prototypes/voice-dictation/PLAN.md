# Голосовая диктовка: план реализации

Основание: `DECISION.md` (утверждённый прототип) и `CONTEXT.md` (термины).

## Архитектура

```
crates/dictation            новый крейт, без GPUI: движок + захват + оркестрация живого транскрипта
crates/agent_ui/src/dictation_window.rs   окно диктовки (Popover у курсора), состояния, постобработка
crates/agent_ui              действие, кнопка микрофона, MentionUri::Dictation (чип-crease), настройки
```

### Движок (`crates/dictation`)

- `transcribe-cpp` 0.2.3 с features `shared` + `dynamic-backends`, без `vulkan` (иначе нужен Vulkan SDK).
  Сборка ggml/transcribe.cpp идёт через cmake внутри cargo (cmake уже обязателен для Zed на Windows).
- Vulkan-бэкенд не компилируем: `ggml-vulkan.dll` берётся из официального артефакта
  `transcribe-native-0.2.3-windows-x86_64-cpu-vulkan` (лежит в `%LOCALAPPDATA%\zed-dictation\transcribe-native`),
  подгружается на старте через `init_backends(dir)`. Путь задаётся настройкой.
- Модель: `ggml-large-v3-q5_0.bin` из Handy (`%APPDATA%\com.pais.handy\models`), путь задаётся настройкой.
- У Whisper в transcribe.cpp нет потокового API (`supports_streaming = false`), поэтому Live Transcript
  строится так: захват микрофона 16 кГц моно → буфер → раз в ~700 мс распознаётся текущая фраза
  (от последней паузы до сейчас) → результат показывается как Pending Text; когда энергетический
  VAD видит паузу ≥ 600 мс, фраза распознаётся окончательно и переходит в Confirmed Text.
- Словарь терминов передаётся Whisper через `WhisperRunOptions::initial_prompt`, язык через `language`.

### UI (`crates/agent_ui`)

1. Действие `agent::ToggleDictation`, привязка `ctrl-alt-space` в контексте `AcpThread > Editor`
   (перекрывает `editor::ShowCharacterPalette` только внутри композера).
2. Кнопка `IconButton(Mic)` в правой группе футера композера перед Send; в записи `Tinted(Error)` + пульсация.
3. Окно диктовки: `deferred(anchored().position(cursor))` над композером, контейнер `Popover`
   (`elevation_2`). Единый каркас: текст + строка «таймер слева, KeyBinding-подсказки справа».
   Состояния: Recording, Review (редактируемый `Editor`), Error (`Callout`), Resume.
4. Постобработка: `LanguageModelRegistry` → модель из настройки (`ollama/qwen3:14b`), промпт из настройки
   с `${output}`, дефолт = промпт «RU + EN Terms Cleanup» из Handy. Ошибка → Callout, Enter принимает сырой текст.
5. Dictation Block: `MentionUri::Dictation { id }` + `Mention::Text`, вставка через
   `insert_crease_for_mention`, подпись `Dictation · 1:03 · 46 words`, иконка Mic. Хранилище блоков
   (текст, длительность) в `MessageEditor`. Backspace удаляет crease целиком (штатно). Клик → окно в Review.
   При отправке `build_chunks_from_creases` разворачивает блок в текст.
6. Настройки `agent.dictation`: `model_path`, `backends_dir`, `language` (ru), `glossary` (из Handy),
   `post_processing: { enabled, model: { provider, model }, prompt }`, `sounds: false`.

## Настройки для этой машины

Проверено 2026-09-04 на записях из Handy: модель грузится за 3,5 с на Vulkan (RTX 3070 Ti),
9 секунд речи распознаются за 0,35 с. В `settings.json`:

```jsonc
"agent": {
  "dictation": {
    "model_path": "C:\\Users\\NikitaDev\\AppData\\Roaming\\com.pais.handy\\models\\ggml-large-v3-q5_0.bin",
    "backends_dir": "C:\\Users\\NikitaDev\\AppData\\Local\\zed-dictation\\transcribe-native\\transcribe-native-windows-x86_64-cpu-vulkan",
    "post_processing": {
      "model": { "provider": "ollama", "model": "qwen3:14b" }
    }
  }
}
```

Папка `backends_dir` содержит официальный артефакт transcribe.cpp 0.2.3 (`ggml-vulkan.dll` и CPU-варианты).
Без неё движок работает на CPU, что для large-v3 слишком медленно.

## Этапы

1. Крейт `dictation`: сборка, загрузка модели на Vulkan, распознавание wav-файла (проверка на машине).
2. Захват микрофона + живой транскрипт (Confirmed / Pending) без UI, лог в консоль.
3. Окно диктовки в панели агента: Recording → Review → Accept как обычный текст.
4. Dictation Block как crease, Resume, постобработка, ошибка постобработки.
5. Настройки, кнопка микрофона, keymap, полировка по прототипу.
