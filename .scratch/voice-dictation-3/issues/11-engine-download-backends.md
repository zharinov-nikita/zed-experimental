# 11 — Engine Download: бэкенды

Спецификация: `.scratch/voice-dictation-3/spec.md`. Словарь: `CONTEXT.md`.

**What to build:** У поля «Backends Folder» кнопка «Download…» скачивает архив transcribe.cpp закреплённой версии 0.2.3, сборка windows-x86_64-cpu-vulkan, с GitHub-релиза тем же загрузчиком, что в тикете 10, сверяет SHA256, распаковывает существующей утилитой архивов в `dictation\backends` data dir и записывает во `backends_dir` путь к внутренней папке артефакта. Прогресс, Cancel, Banner и живучесть при закрытом окне как у модели. Версия и сумма живут константами рядом с версией `transcribe-cpp-sys`, чтобы обновлялись одним коммитом.

**Blocked by:** 10 — Engine Download: модель.

**Status:** ready-for-agent

- [ ] Тест с фейковым HTTP-клиентом: архив распакован, возвращён путь внутренней папки, настройка записана; неверная сумма удаляет архив.
- [ ] Ручная проверка: после загрузки `backends_dir` указывает в data dir и диктовка стартует на Vulkan.
- [ ] `./script/clippy` и тесты `settings_ui` зелёные; `LOCAL_DEV.md` описывает папку бэкендов и откуда берётся версия.
