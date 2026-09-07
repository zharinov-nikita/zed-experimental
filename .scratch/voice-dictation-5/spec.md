# Voice Dictation, итерация 5: диктовка в Answer Field

Метка: `ready-for-agent`
Словарь: `CONTEXT.md` в корне репозитория. Термины (Dictation Session, Dictation Window, Live Transcript, Confirmed Text, Pending Text, Post-processing, Session Audio, Resume, Composer, Dictation Block, Commit, Discard, Agent Question, Answer Field) используются строго в его значениях.
Решения: `docs/adr/0001…0003`.
Первичные источники: грилинг 2026-09-07, код `crates/agent_ui/src/conversation_view/elicitation.rs`, `crates/agent_ui/src/conversation_view/thread_view.rs`, `crates/agent_ui/src/dictation_window.rs`.

## Problem Statement

Когда агент задаёт Agent Question, ответ пишется в Answer Field внутри карточки вопроса, а не в Composer. Хоткей диктовки там не работает, кнопки микрофона нет: единственный способ ответить голосом отсутствует, хотя это самый частый диалог с Claude Code в середине задачи.

## Solution

Dictation Window разворачивается внутри карточки Agent Question над Answer Field, в котором стоит фокус, и ведёт себя как над Composer: Live Transcript, просмотр, Post-processing, Session Audio, Play. Accept вставляет текст у курсора Answer Field и ничего не отправляет. Если Agent Question исчезает во время сессии, окно закрывается, а распознанный текст переезжает в Composer как Dictation Block.

## User Stories

1. As a Zed user, I want `ctrl-alt-space` to start dictation when the focus is in an Answer Field, so that answering the agent by voice works the same way as dictating a prompt.
2. As a Zed user, I want a microphone button next to every Answer Field, so that I can discover the feature and start it with the mouse.
3. As a Zed user, I want the Dictation Window to unfold inside the Agent Question above the Answer Field I am answering, so that I look where I type.
4. As a Zed user, I want the Live Transcript, review, Post-processing and Play to work exactly as in the Composer, so that there is one dictation to learn.
5. As a Zed user, I want Accept to insert the text at the cursor of the Answer Field without sending, so that I can add or fix words before I submit the answer.
6. As a Zed user, I want a multi-line answer to keep its line breaks, so that a dictated paragraph does not collapse into one line.
7. As a Zed user, I want the Answer Field to grow with the text instead of scrolling sideways, so that I see the whole answer.
8. As a Zed user, I want my dictation kept when the agent withdraws the question mid-session, so that what I said is not lost.
9. As a Zed user, I want the microphone button and the hotkey to be disabled while another Dictation Session runs, so that the one-session rule is visible instead of silent.
10. As a fork maintainer, I want the Dictation Window to know nothing about Agent Questions, so that it keeps one host contract with two hosts.

## Implementation Decisions

- Хост окна: `AcpThreadView`, который уже владеет состоянием форм Agent Question и единственным `dictation_window`. Окно получает focus handle Answer Field вместо focus handle Composer и тот же набор событий. Хост различает, для кого открыто окно: для Composer или для конкретного Answer Field (идентификатор вопроса и поля).
- `Accept` для Answer Field вставляет текст у курсора редактора поля (`Editor::insert`), с пробелом или переносом по контексту, как `join_text` в окне; Dictation Block не создаётся, `block_id` игнорируется. Session Audio пишется как обычно под идентификатором сессии и уходит по ротации `keep`.
- Answer Field становится `Editor::auto_height` с мягким переносом и лимитом строк как у Composer; горизонтального scrollbar нет.
- Кнопка микрофона рендерится у каждого текстового поля карточки (`render_dictation_button` переиспользуется с другим обработчиком); хоткей `ctrl-alt-space` обрабатывается в контексте Answer Field и не всплывает в обработчик Composer.
- Пока `dictation_window` существует, кнопки у полей и хоткей в них погашены; то же для кнопки Composer, пока окно открыто над Answer Field.
- Если Agent Question уходит (ответ отправлен другим путём, агент отменил, сессия завершилась), хост закрывает окно: при активной записи она останавливается, распознанный и обработанный текст вставляется в Composer как Dictation Block с тем же `block_id`; при пустом тексте окно просто закрывается.
- Escape в записи и просмотре работает как над Composer; после Accept и Discard фокус возвращается в Answer Field.

## Testing Decisions

- Чистые функции с тестами: выбор хоста для Accept (текст в Answer Field или блок в Composer) по состоянию «вопрос ещё открыт»; вставка текста у курсора с правильным разделителем (пусто, середина слова, конец строки).
- Тесты `AcpThreadView` на уровне сущностей там, где уже есть прототипы (`thread_view` tests): открытие окна для Answer Field, `Accept` вставляет текст и не отправляет ответ, исчезновение вопроса переносит текст в Composer.
- Ручная проверка в dev-сборке: Claude Code задаёт вопрос, диктовка по хоткею и по кнопке, многострочный ответ, Accept без отправки, Play, отмена вопроса во время записи.

## Out of Scope

- Диктовка в поля выбора вариантов (только текстовые поля).
- Автоотправка ответа после Accept.
- Quote Reply (итерация 6).

## Further Notes

- Словарь дополнен терминами Agent Question и Answer Field; Session Audio теперь принадлежит Dictation Session.
