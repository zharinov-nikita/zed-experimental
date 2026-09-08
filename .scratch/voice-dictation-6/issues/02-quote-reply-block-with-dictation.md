# 02 — Quote Reply Block: «Dictate Reply to Selection»

Спецификация: `.scratch/voice-dictation-6/spec.md`. Словарь: `CONTEXT.md`. Решение: `docs/adr/0003-quote-reply-block-keeps-quote-and-comment-together.md`.

**What to build:** Пункт «Dictate Reply to Selection» и хоткей `ctrl-alt-space` при фокусе в ответе агента с выделением создают у курсора Composer Quote Reply Block с пустым комментарием и открывают над Composer Dictation Window, где над транскриптом стоит нередактируемая плашка с цитатой. Accept заполняет комментарий, Discard при пустом комментарии удаляет блок. Crease подписан первыми словами комментария, тултип показывает цитату и комментарий; блок открывается из чипа как Dictation Block, Resume и правка касаются только комментария; удаляется целиком. Агенту уходит цитата, пометка и комментарий.

**Blocked by:** 01 — Quoted Fragment и «Reply to Selection».

**Status:** done

- [x] Тесты сериализации Quote Reply Block и подписи crease (короткий и длинный комментарий).
- [x] Тесты хоста: пункт и хоткей погашены при активной сессии и без выделения; Discard пустого комментария удаляет блок.
- [x] Ручная проверка: диктовка с плашкой цитаты, Resume добавляет к комментарию, цитата неизменна, отправка агенту.
- [x] `./script/clippy` и тесты `agent_ui` зелёные.
