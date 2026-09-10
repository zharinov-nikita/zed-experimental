# Voice Dictation, итерация 7: Post-processing через External Agent

Метка: `ready-for-agent`
Словарь: `CONTEXT.md` в корне репозитория. Термины (Dictation, Dictation Session, Dictation Window, Post-processing, **Post-processing Session**, **External Agent**, Resume, Commit, Discard, Composer, Glossary, Transcription Engine, Session Audio, Live Transcript) используются строго в его значениях.
Решения: `docs/adr/0004-post-processing-may-leave-the-machine-through-an-agent-session.md`. Итерация 4 (`docs/adr/0002`, запуск Ollama) остаётся в силе.
Первичные источники: разбор установленного пакета агента `@agentclientprotocol/claude-agent-acp@0.75.1` в `%LOCALAPPDATA%\Zed\external_agents\registry\npx\claude-acp\`, код `crates/agent_servers`, `crates/acp_thread`, `crates/agent_ui`, `crates/settings_ui`, спека итерации 4 `.scratch/voice-dictation-4/spec.md`.

## Problem Statement

Post-processing умеет ходить только к моделям из списка провайдеров Zed. На практике это Ollama, и она стоит владельцу форка больше памяти, чем стоит сама фича: локальная модель висит в памяти всё время, пока Zed запущен.

Очевидный обход — указать Post-processing на облачную модель через того же провайдера — здесь закрыт: у владельца форка есть подписка Claude Code и нет API-ключа Anthropic, так что ни одна облачная модель через список провайдеров недостижима.

При этом External Agent, которого владелец запускает каждый день, облачную модель предоставляет — и умеет, среди прочего, переписывать текст. Но выбрать его для Post-processing нельзя: дропдаун моделей в настройках диктовки перечисляет только провайдеров Zed, а External Agent к ним не относится. Выбрать внутри него модель подешевле (Haiku вместо Sonnet) отдельно от рабочего чата тоже нельзя.

## Solution

У Post-processing появляется второй бэкенд: вместо модели из списка провайдеров можно выбрать External Agent из тех, что настроены у пользователя, и указать, с какими его настройками работать — в частности, какую модель.

Переписывание идёт в Post-processing Session: собственная сессия агента, отдельная от рабочего разговора пользователя с тем же агентом. Она живёт одну Dictation Session, поэтому Resume переписывается против текста, который эта же сессия только что выдала, а следующая Dictation Session начинает с нуля.

Сессия ничего не наследует от рабочих настроек агента: модель и режим выставляются явно. Она не может действовать на машине — рабочий каталог пуст, режим самый строгий, а любой её запрос к файловой системе, терминалу, разрешениям или к самому пользователю отклоняется. Попытка что-то сделать вместо ответа текстом считается провалом прогона: пользователь остаётся с сырым текстом и видит, почему.

Ollama и весь локальный путь остаются как есть.

## User Stories

### Выбор бэкенда

1. As a Zed user, I want to pick an External Agent as the rewriter for Post-processing, so that I can use the subscription I already pay for instead of running a local model.
2. As a Zed user, I want the list of agents to be the ones I have configured, so that I am not offered something I never set up.
3. As a Zed user, I want to keep choosing a language model provider instead, so that nothing changes for me if I liked the local path.
4. As a Zed user, I want the agent and the language model to be mutually exclusive, so that it is never ambiguous which one rewrites my transcript.
5. As a Zed user, I want my existing dictation settings to keep working untouched, so that adding this feature does not cost me a migration.
6. As a Zed user, I want the agent's own default model to be an explicit choice, so that I can say "whatever the agent thinks is right" on purpose rather than by omission.
7. As a Zed user, I want to pin Haiku for Post-processing while my working thread runs Sonnet, so that cleaning up dictation does not cost what real work costs.
8. As a Zed user, I want a configured-but-missing agent to be a visible error and never a silent switch to something else, so that my transcript is never rewritten by a model I did not choose.

### Настройки

9. As a Zed user, I want the agent and its settings on the same Dictation page as everything else, so that I do not go hunting across settings.
10. As a Zed user, I want to see the agent's models by their human names, so that I choose "Haiku" and not an id I have to decode.
11. As a Zed user, I want to be told plainly that the model list is not known yet because the agent has not been contacted, so that an empty dropdown does not look broken.
12. As a Zed user, I want the list to fill in by itself after the first rewrite, so that I do not have to perform a special ritual to populate it.
13. As a Zed user, I want the Glossary and the prompt to work exactly as they do for a language model, so that switching the rewriter does not change what the rewriting does.
14. As a Zed user, I want the settings I pick to be written in a form I can also edit by hand, so that the settings file stays the source of truth.

### Post-processing Session

15. As a Zed user, I want the rewrite to happen in a conversation of its own, so that my transcripts do not end up in the context of my working thread.
16. As a Zed user, I want my working thread's context never to leak into the rewrite, so that the cleanup is not influenced by whatever I was discussing.
17. As a Zed user, I want the rewrite to reuse the agent process that is already running, so that dictation does not start a second copy and spend the memory I was trying to save.
18. As a Zed user, I want a Resume to be rewritten in the same session as the rest of that dictation, so that the agent sees the text it produced a moment earlier.
19. As a Zed user, I want the next Dictation Session to start from nothing, so that yesterday's transcripts are not in the context of today's.
20. As a Zed user, I want the session to use the model I picked for Post-processing and not the one my chat is on, so that pinning Haiku actually means Haiku.
21. As a Zed user, I want none of my stored agent settings to leak into that session, so that an option that is invalid for the chosen model cannot break the rewrite.
22. As a Zed user, I want the rewrite session not to litter my agent's thread history, so that my list of recent conversations stays about my work.

### Запрет действий

23. As a Zed user, I want the rewrite session to be unable to touch my files, so that a transcript of whatever was said near my microphone cannot become an action on my machine.
24. As a Zed user, I want it to be unable to run commands, so that the same is true of my terminal.
25. As a Zed user, I want it to be unable to ask me for permission, so that dictation never turns into a dialog I have to answer.
26. As a Zed user, I want it to be unable to ask me a question, so that a rewrite either produces text or fails.
27. As a Zed user, I want an attempt to act to count as a failed rewrite, so that I get my raw text rather than a half-done one.
28. As a Zed user, I want the ban to hold regardless of the permission mode I use for my own work, so that a permissive mode in my chat does not apply to my dictation.
29. As a fork maintainer, I want the ban enforced where the request arrives rather than detected afterwards, so that a tool cannot run before we notice.

### Провалы и ожидание

30. As a Zed user, I want the raw text kept whenever the rewrite fails, so that a failure never costs me what I said.
31. As a Zed user, I want to be told why it failed, so that I can fix the cause instead of guessing.
32. As a Zed user, I want an agent that is not installed or not signed in to be a clear message, so that I do not read it as a dictation bug.
33. As a Zed user, I want a rewrite that hangs to give up after a limit, so that a stuck agent does not hold my dictation forever.
34. As a Zed user, I want a failure not to poison the next dictation, so that a fixed agent needs no restart of Zed.
35. As a Zed user, I want to see that the rewrite is in progress and who is doing it, so that the wait is not a mystery.

### Просмотр результата

36. As a Zed user, I want the footer to name the agent and the model that rewrote the text, so that I can tell a Haiku cleanup from a Sonnet one.
37. As a Zed user, I want to switch between raw and rewritten text exactly as I do now, so that the agent path is not a special case.
38. As a Zed user, I want Commit, Discard, Resume and playback to behave exactly as they do now, so that only the rewriter changed.

### Сопровождение

39. As a fork maintainer, I want the choice of backend to be one pure decision over settings, so that it can be tested without a model, an agent or a window.
40. As a fork maintainer, I want the session policy — options, mode, working directory — to be a pure function over settings and what the agent announced, so that it is testable against a recorded announcement.
41. As a fork maintainer, I want the outcome of a run classified by one pure function, so that "text", "tried to act", "timed out" and "unavailable" are decided in one place.
42. As a fork maintainer, I want everything external injected, so that no test opens a session, spawns a process or reaches the network.
43. As a fork maintainer, I want the fork-specific code kept in its own module, so that upstream merges stay cheap.

## Implementation Decisions

### Настройки (`settings_content`, `agent_settings`)

- Рядом с существующим `agent.dictation.post_processing.model` появляется `agent.dictation.post_processing.agent` — объект с идентификатором External Agent и картой его настроек «идентификатор опции → значение». Значения — строки и булевы, ровно то, что агент принимает.
- Ключи взаимоисключающие. Если заданы оба, это ошибка конфигурации, видимая пользователю, а не молчаливый приоритет одного над другим.
- Идентификатор агента — тот же, которым он назван в `agent_servers`. Отдельного списка допустимых агентов в коде диктовки нет.
- Ни один существующий ключ не мигрирует, дефолты не меняются: `post_processing.enabled` по-прежнему включён, при обоих незаданных ключах Post-processing по-прежнему берёт модель агента по умолчанию.
- Кэш объявленных агентом опций — тоже настройка, но пишется кодом, а не человеком: список, который агент прислал в ответ на создание сессии, сохраняется под идентификатором агента, чтобы страница настроек могла показать его до всякого подключения.

### Post-processing (`agent_ui`, новый модуль)

- Вся логика Post-processing выносится из `DictationWindow` в отдельный модуль. `DictationWindow` остаётся владельцем состояния и вида, но решения принимает не он.
- **Выбор бэкенда** — чистая функция над разрешёнными настройками диктовки: языковая модель, External Agent, либо модель агента по умолчанию. Прежнее поведение (заданная, но отсутствующая модель — жёсткая ошибка, а не подмена) распространяется и на агента.
- **Политика Post-processing Session** — чистая функция над настройками и тем, что агент объявил при создании сессии. Возвращает: какие опции выставить (только свои, ничего из сохранённых настроек агента), какой режим выбрать из объявленных агентом — самый строгий из доступных, и пустой список рабочих каталогов. Опции, которых агент не объявил, не выставляются вовсе: попытка выставить неизвестную опцию — ошибка на его стороне.
- **Классификация исхода** — чистая функция над тем, что накопилось в сессии: текст ответа; попытка действия; отказ агента; истёкший предел ожидания; агент недоступен. Мысли модели отбрасываются тем же способом, что и сейчас.
- Внешние способности входят инъекцией: «дай соединение с этим агентом», «создай сессию», «отправь текст и дождись ответа», «удали сессию». Реализация берёт соединение из общего хранилища соединений Zed и своего процесса не создаёт.
- Одна Post-processing Session на одну Dictation Session: создаётся при первом прогоне внутри неё, переиспользуется на Resume, удаляется по завершении Dictation Session. Если агент удаление сессий не поддерживает, сессия помечается так, чтобы список тредов Zed её не показывал.
- Предел ожидания ответа — 60 секунд; по его истечении Post-processing не выполнено, текст остаётся сырым. Предел живёт рядом с существующим пределом запуска Ollama.
- Промпт, `${output}` и `${glossary}` — те же, что и для языковой модели. Post-processing не знает, чем именно его выполнили.

### Запрет действий (`agent_servers`)

- Сессия может быть помечена как «только текст» в момент создания. Пометка живёт вместе с остальным состоянием сессии, а не соединения: одно соединение обслуживает и рабочий тред, и Post-processing Session.
- Клиентские обработчики запросов агента — файловая система, терминал, запрос разрешения, вопрос пользователю — спрашивают эту пометку до того, как что-либо сделать, и на помеченной сессии отвечают отказом.
- Отказ виден вызывающей стороне: прогон, в котором он случился, классифицируется как попытка действия.
- Возможности клиента объявляются один раз на соединение и не меняются: урезать их для одной сессии нельзя, поэтому запрет реализуется именно отказом по сессии.

### Настройки, страница Dictation (`settings_ui`)

- В разделе Post-processing выбор становится двухуровневым: сперва чем переписывать — провайдером Zed или External Agent, затем конкретика. Существующий выбор провайдера и модели сохраняется как одна из ветвей.
- Список моделей агента строится из кэша объявленных опций. Пока кэша нет, вместо пустого списка показывается объяснение, что агент ещё не отвечал и список появится после первого переписывания.
- Опции агента, кроме модели, редактируются как есть: селекты — списком объявленных значений, булевы — переключателем.
- `settings_ui` не получает зависимости на `agent_ui` и агента не поднимает: страница только читает кэш.

### Просмотр (`agent_ui`)

- Подпись «Processed · …» получает имя агента и выбранной модели. Для модели агента по умолчанию — имя агента и указание, что модель его собственная.
- Сообщения о провале — те же Callout, что и для Ollama, с текстами под новые причины: агент недоступен, агент попытался действовать, истёк предел ожидания.
- Спиннер во время ожидания называет агента, как сейчас называет Ollama.

## Testing Decisions

Хороший тест здесь проверяет наблюдаемое поведение: какое решение принято по настройкам, какие опции ушли бы в сессию, чем закончился прогон, что увидит пользователь в футере. Тест не знает, как устроены структуры внутри модуля, и не поднимает ни агента, ни процесс, ни сеть.

- **Выбор бэкенда, политика сессии, классификация исхода** — чистые `#[test]` в новом модуле `agent_ui`. Прообраз: `dictation_host.rs` (чистые решения `start_decision`, `accept_destination`) и `dictation_model_server.rs` (внешние способности инъекцией, фальшивая достижимость и фальшивый запуск).
- **Жизненный цикл Post-processing Session** — тесты над модулем с инъектированными способностями: одна сессия на Dictation Session, переиспользование на Resume, удаление в конце, отсутствие второго процесса. Прообраз тот же.
- **Разрешение настроек** — `#[test]` рядом с существующими тестами `from_settings` в `agent_settings`: взаимоисключение ключей, дефолты, кэш опций.
- **Запрет действий** — чистый тест на предикат «только текст» плюс прогон через уже существующий фальшивый ACP-агент в `agent_servers`: помеченная сессия получает отказ, непомеченная — нет.
- **Футер** — `#[test]` в существующей state-машине `dictation_footer.rs`: подпись с агентом и моделью, спиннер, сообщения о провале.
- **Страница настроек** — `#[test]` рядом с существующими тестами `dictation_page.rs`: построение списка из кэша, состояние «кэша нет», взаимоисключение веток.

Ручная проверка на dev-сборке обязательна и покрывает то, что тестами не берётся: реальное переписывание через установленного агента, переключение Haiku/Sonnet, отсутствие Post-processing Session в списке тредов, поведение при выключенном агенте.

## Out of Scope

- Quote Reply и любые другие места, где текст мог бы обрабатываться моделью. Только Post-processing диктовки.
- Общая команда «обработать этот текст моделью» вне диктовки.
- Удаление локального пути: Ollama, её автозапуск и выбор провайдера Zed остаются без изменений.
- Выбор модели для рабочего треда пользователя — он и сейчас работает, его не трогаем.
- Стриминг переписанного текста: как и сейчас, результат появляется целиком.
- Починка невалидного `"agent": "default"` в личных настройках владельца форка — это строка в его `settings.json`, а не код.
- Попытки научить Zed перечислять модели External Agent без подключения: это свойство протокола, а не наш недосмотр.

## Further Notes

- Установленный агент объявляет опцию `model` со значениями `default`, `opus`, `opus[1m]`, `sonnet`, `claude-sonnet-4-6[1m]`, `haiku`, `opusplan`, принимает человеческие алиасы и публикует режимы `default` (Manual, «always ask»), `acceptEdits`, `plan`, `auto` и `bypassPermissions`. Непубликуемый `dontAsk` («auto-deny anything that would prompt») подошёл бы лучше всего, но его нет среди объявленных режимов, поэтому опираться на него нельзя.
- Режим `plan` запрещает запись, но разрешает чтение. Ни один объявленный режим не означает «совсем без инструментов» — отсюда отказ по сессии как единственная настоящая гарантия.
- Опции `fast`, `effort` и `agent` публикуются не всегда: `fast` — только на моделях, которые её поддерживают, `effort` — только на моделях с уровнями усилия, `agent` — только при наличии кастомных агентов. Отсюда правило «выставляем только то, что агент объявил».
- Список моделей агента частично зависит от прав аккаунта, поэтому в коде его быть не должно ни в каком виде.
