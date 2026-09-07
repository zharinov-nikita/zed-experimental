# 02 — Session Audio: хранилище и настройка

Спецификация: `.scratch/voice-dictation-3/spec.md`. Словарь: `CONTEXT.md`.

**What to build:** Звук каждой Dictation Session сохраняется локально как Session Audio: 16 кГц mono WAV в папке `dictation\audio` внутри data dir Zed, по id Dictation Block, который сессия породила; отменённая сессия получает свой id и тоже сохраняется. Resume дописывает звук в файл того же блока. Настройка `agent.dictation.session_audio.keep` (по умолчанию 20) задаёт, сколько файлов хранить, старейшие вытесняются; 0 выключает запись. Ключ `save_last_recording` и файл в temp удаляются. Подстраница Dictation показывает новое числовое поле вместо переключателя. Файлы читаются примером `transcribe_wav` и годятся как фикстуры.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] Тесты хранилища с временной папкой: лимит вытесняет старейшие по времени изменения, Resume дописывает в файл блока, отменённая сессия сохранена, `keep = 0` не пишет ничего.
- [ ] Тесты `agent_settings`: `session_audio.keep` читается, умолчание 20, старый ключ `save_last_recording` не ломает разбор.
- [ ] После сессии в dev-сборке файл появляется в `dictation\audio` data dir и читается `transcribe_wav`.
- [ ] Подстраница Dictation: числовое поле «Keep session audio» пишет `settings.json`, переключателя «Save last recording» нет.
- [ ] `./script/clippy` и тесты `dictation`, `agent_settings`, `settings_ui` зелёные; `LOCAL_DEV.md` описывает папку и лимит.
