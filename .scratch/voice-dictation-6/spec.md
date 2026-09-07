# Voice Dictation, итерация 6: Quote Reply

Метка: `ready-for-agent`
Словарь: `CONTEXT.md` в корне репозитория. Термины (Quote Reply, Quoted Fragment, Quote Reply Block, Dictation Session, Dictation Window, Dictation Block, Composer, Commit, Resume, Post-processing, Session Audio) используются строго в его значениях.
Решения: `docs/adr/0003-quote-reply-block-keeps-quote-and-comment-together.md`.
Первичные источники: грилинг 2026-09-07, код `crates/markdown/src/markdown.rs` (выделение, `selected_source`, `CopyAsMarkdown`), `crates/agent_ui/src/conversation_view/thread_view.rs` (`render_message_context_menu`, хостинг Dictation Window), `crates/agent_ui/src/message_editor.rs` (Dictation Block как crease, `MentionUri::Dictation`).

## Problem Statement

Чтобы прокомментировать кусок ответа агента, сейчас нужно скопировать выделенное, вставить в Composer, оформить как цитату и только потом писать или диктовать. Связь комментария с фрагментом держится на ручном оформлении, а голосом сделать это вообще не получается за одно действие.

## Solution

В контекстном меню ответа агента два пункта: «Reply to Selection» кладёт выделенный фрагмент в Composer как Quoted Fragment, после которого пользователь печатает; «Dictate Reply to Selection» (и `ctrl-alt-space` при фокусе в ответе с выделением) кладёт фрагмент и сразу открывает Dictation Window, где фрагмент стоит над транскриптом, а результат Commit'ится как один Quote Reply Block. Агенту уходит цитата с пометкой и следом комментарий.

## User Stories

1. As a Zed user, I want «Reply to Selection» in the context menu of an agent response, so that the selected fragment lands in the Composer as a quote and I type after it.
2. As a Zed user, I want «Dictate Reply to Selection» in the same menu and `ctrl-alt-space` on a selection, so that one action quotes the fragment and starts dictation.
3. As a Zed user, I want the quote to come from the response text only, so that pieces of tool cards never end up quoted.
4. As a Zed user, I want the quote to keep code blocks and lists, so that the agent sees exactly what I mean.
5. As a Zed user, I want the fragment shown above the transcript while I dictate, so that I see what I am commenting on.
6. As a Zed user, I want the quote and the dictated comment to be one unit in the Composer, so that they cannot drift apart or be deleted separately.
7. As a Zed user, I want to resume or edit only the comment when I open a Quote Reply Block, so that the quote stays as I selected it.
8. As a Zed user, I want the chip labelled with the first words of my comment and a tooltip with both parts, so that I recognize the block at a glance.
9. As a Zed user, I want as many quotes and Quote Reply Blocks per message as I like, inserted at the cursor, so that I can answer several points at once.
10. As a Zed user, I want the agent to receive `> quote`, a note that it is quoting its own reply, and then my comment, so that the agent understands the reference.
11. As a Zed user, I want the note in the language of `agent.dictation.language`, so that it matches the language I dictate in.
12. As a Zed user, I want both menu items disabled while a Dictation Session runs, so that the one-session rule is visible.
13. As a Zed user, I want the menu items disabled without a selection and hidden for anything but agent responses, so that the menu stays honest.

## Implementation Decisions

- Источник цитаты: `Markdown::selected_source` элемента ответа под меню (тот же перебор `AssistantMessageChunk`, что и для «Copy Selection»); выделение ограничено одним markdown-блоком ответа, карточки инструментов не участвуют. Пункты меню только у записей ответа агента.
- Quoted Fragment в Composer: crease по образцу Dictation Block с собственным `MentionUri` (например, `Quote`), хранит markdown цитаты; удаляется целиком, в текст сообщения сериализуется как `> …` плюс пометка.
- Quote Reply Block: crease с `MentionUri` своего вида, хранит цитату и комментарий; подпись из первых слов комментария, тултип с обеими частями; открывается в Dictation Window так же, как Dictation Block (`edit_dictation_block` расширяется на новый вид). В окне над транскриптом нередактируемая плашка с цитатой (Callout или collapsed markdown); `prefix` окна это только комментарий.
- «Dictate Reply to Selection»: хост создаёт Quote Reply Block с пустым комментарием у курсора Composer и открывает окно для него; `Accept` заполняет комментарий, `Discard` при пустом комментарии удаляет блок.
- Хоткей: `ctrl-alt-space` в контексте `Markdown` ответа агента при `has_selection` запускает «Dictate Reply to Selection»; без выделения ничего не делает и не всплывает.
- Формат для агента: `> цитата` (построчно), пустая строка, пометка на языке `agent.dictation.language` (`ru`: «(из твоего ответа выше)», иначе «(quoting your reply above)»), пустая строка, комментарий. Для Quoted Fragment без комментария — цитата и пометка, дальше печатный текст пользователя как есть.
- Пока `dictation_window` открыт, оба пункта погашены.

## Testing Decisions

- Чистые функции с тестами: сериализация Quoted Fragment и Quote Reply Block в текст сообщения (оба языка пометки, многострочная цитата, кодовый блок в цитате); подпись crease из первых слов комментария; выбор пунктов меню по наличию выделения, виду записи и активной сессии.
- Тесты `message_editor` на вставку и удаление новых crease целиком (прототип: тесты Dictation Block).
- Ручная проверка: выделить фрагмент ответа, оба пункта меню, хоткей, диктовка с плашкой цитаты, Resume только комментария, отправка агенту и проверка, что он понял ссылку.

## Out of Scope

- Цитирование своих сообщений и вывода инструментов.
- Хоткей для печатного «Reply to Selection».
- Несколько фрагментов в одном Quote Reply Block.

## Further Notes

- Словарь: Quote Reply, Quoted Fragment и Quote Reply Block уточнены по итогам грилинга; ADR 0003 фиксирует, почему цитата и комментарий одна сущность.
