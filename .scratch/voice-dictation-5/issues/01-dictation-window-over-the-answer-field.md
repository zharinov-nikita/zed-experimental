# 01 — Dictation Window над Answer Field

Спецификация: `.scratch/voice-dictation-5/spec.md`. Словарь: `CONTEXT.md`.

**What to build:** При фокусе в Answer Field хоткей `ctrl-alt-space` и кнопка микрофона у поля открывают Dictation Window внутри карточки Agent Question над этим полем. Окно ведёт себя как над Composer: Live Transcript, просмотр, Post-processing, Session Audio, Play, Resume. Хост (`AcpThreadView`) помнит, для какого поля открыто окно, передаёт ему focus handle поля и погашает кнопки и хоткей диктовки везде, пока сессия идёт. Окно об Agent Question не знает.

**Blocked by:** None — can start immediately.

**Status:** implemented — ручная проверка в dev-сборке не проведена

- [x] Тесты: окно открывается для Answer Field, а не для Composer; пока оно открыто, старт из Composer и из других полей отклоняется.
- [ ] Ручная проверка: вопрос от Claude Code, диктовка по хоткею и по кнопке, окно раскрывается над полем, Live Transcript виден.
- [x] `./script/clippy` и тесты `agent_ui` зелёные.
