# 04 — Звуки старта, Resume и остановки

Спецификация: `.scratch/voice-dictation-4/spec.md`. Словарь: `CONTEXT.md`.

**What to build:** При `agent.dictation.sounds: true` начало записи и Resume дают существующий звук «unmute», остановка записи (esc, хоткей, подсказка Review) даёт звук «mute»; Accept и Cancel в просмотре, Post-processing и ошибки беззвучны. Звук идёт на выход из настроек Audio. Умолчание остаётся выключенным. Какой звук соответствует какому переходу, решает чистая функция с тестами; окно только вызывает её на переходах.

**Blocked by:** None — can start immediately.

**Status:** done

- [x] Тесты функции «переход → звук»: старт и Resume дают «unmute», остановка даёт «mute», Accept, Cancel, Failed и Post-processing дают ничего.
- [x] Ручная проверка: с `sounds: true` слышны звуки на старте, Resume и остановке на выбранном выходе; с `sounds: false` тишина.
- [x] `./script/clippy` и тесты `agent_ui` зелёные.
