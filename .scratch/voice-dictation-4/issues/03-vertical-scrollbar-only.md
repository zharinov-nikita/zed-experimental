# 03 — Только вертикальный scrollbar в теле записи

Спецификация: `.scratch/voice-dictation-4/spec.md`. Словарь: `CONTEXT.md`.

**What to build:** В теле Dictation Window во время записи нет горизонтального scrollbar: текст переносится по ширине, прокручивается только по вертикали, и вертикальный scrollbar по-прежнему подчиняется `scrollbar.show`. Тело просмотра ведёт себя так же.

**Blocked by:** None — can start immediately.

**Status:** done

- [x] Ручная проверка: диктовка длиннее десяти строк показывает только вертикальный scrollbar в записи и в просмотре, горизонтального нет ни при какой ширине панели.
- [x] `scrollbar.show: "never"` убирает вертикальный scrollbar.
- [x] `./script/clippy` зелёный.
