# 08 — Scrollbar в теле Dictation Window

Спецификация: `.scratch/voice-dictation-3/spec.md`. Словарь: `CONTEXT.md`.

**What to build:** Тело Dictation Window получает стандартный scrollbar Zed и в записи, и в просмотре, с поведением по настройке `scrollbar.show`, как у любого редактора. Когда текст длиннее 10 строк, видно, что ниже есть ещё; при автопрокрутке к концу во время записи scrollbar следует за текстом.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] Ручная проверка: диктовка длиннее 10 строк показывает scrollbar в записи, после Escape scrollbar есть в просмотре, `scrollbar.show: "never"` его убирает.
- [ ] Высота секции и правила 10 строк не изменились.
- [ ] `./script/clippy` зелёный.
