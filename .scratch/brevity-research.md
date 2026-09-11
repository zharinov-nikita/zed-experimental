# Управление многословностью в Claude Code — проверенные факты

Источники: code.claude.com/docs (официальные), platform.claude.com/docs (официальные),
github.com/Piebald-AI/claude-code-system-prompts (реверс системного промпта, v2.1.267 — вторичный,
помечен как [РЕВЕРС]). Локальная версия CLI: 2.1.263.

## 1. Output styles

- Меняют **сами дефолтные инструкции** Claude Code, а не добавляются к ним. «Claude Code sends the
  active style's instructions with every request» + напоминание о стиле по ходу диалога.
- Встроенный краткий стиль **есть**: `Concise` — «leads with the result, skips preamble and
  narration, keeps responses short by default… Requires Claude Code v2.1.237 or later». Также
  Default, Proactive, Explanatory, Learning. (docs/en/output-styles)
- `/output-style` **удалён**: deprecated в v2.1.73, removed в v2.1.91. Сейчас — `/config` →
  Output style, либо поле `outputStyle` в settings-файле (меню пишет в `.claude/settings.local.json`).
- Файлы: `~/.claude/output-styles/`, `.claude/output-styles/`, managed-policy. Имя файла = имя стиля.
- Frontmatter (полный список полей из доков): `name`, `description`,
  `keep-coding-instructions` (default `false`), `force-for-plugin` (только плагины).
- Ключевая ловушка: кастомный стиль **выкидывает** встроенные software-engineering инструкции, если
  не поставить `keep-coding-instructions: true`.
- С v2.1.251 смена стиля применяется со следующего сообщения без `/clear`. Файлы стилей читаются при
  старте CLI — правку существующего файла подхватит только рестарт.
- На сабагентов стиль **не действует** (у них свой системный промпт); исключение — fork.
- Текст встроенного Concise [РЕВЕРС]: 6 нумерованных правил + финальная строка «Where these rules
  conflict with more general communication or formatting guidance elsewhere in your instructions,
  these rules win» — то есть он явно перебивает остальной промпт.

## 2. Отличия и приоритет

- CLAUDE.md — «delivered as a user message after the system prompt, not as part of the system prompt
  itself… no guarantee of strict compliance» (docs/en/memory). Слабее системного промпта.
- `--append-system-prompt` / `--append-system-prompt-file` — дописывают в **конец** системного
  промпта, ничего не удаляют; `--system-prompt(-file)` заменяет целиком. Работают и в интерактиве, и
  в `-p`. Доки по памяти прямо рекомендуют их «for instructions you want at the system prompt level».
- Формального ранга «стиль > CLAUDE.md > append» в доках нет. Фактическая иерархия по позиции в
  контексте: output style (внутри system prompt, замещает дефолт) → append (конец system prompt) →
  CLAUDE.md (user-сообщение). Гипотеза, но подтверждается цитатой выше про CLAUDE.md.
- Precedence самих settings-файлов: managed → `--settings` → `.claude/settings.local.json` →
  `.claude/settings.json` → `~/.claude/settings.json`.

## 3. Официальные рекомендации по формулировкам

- «**Tell Claude what to do instead of what not to do**. Instead of: "Do not use markdown in your
  response" — Try: "Your response should be composed of smoothly flowing prose paragraphs."»
  (prompt-engineering/claude-prompting-best-practices).
- Про Opus 5 отдельно: «Positive examples of the communication style you want tend to be more
  effective than instructions about what not to do» (prompting-claude-opus-5).
- Числовых лимитов строк Anthropic **не** предлагает нигде. Все официальные примеры — описание формы
  ответа: «Keep responses focused, brief, and concise… When asked to explain something, give a
  high-level summary unless an in-depth explanation is specifically requested», плюс короткое
  напоминание в конце длинного промпта: `<tone_preference>Keep outputs reasonably concise.</tone_preference>`.
- Важно для Opus 5: «default user-facing responses run longer than prior Opus models'… lowering
  effort can reduce thinking volume without reliably shortening the visible response. To control
  response length, prompt for it explicitly.» Отдельный рецепт для нарратива между тулколами и
  отдельный — для длины файлов-документов.

## 4. Что дефолтный промпт говорит про краткость

- «≤4 lines unless asked» — **историческое**. В реверсе v2.1.267 такого фрагмента нет; в актуальном
  наборе есть `system-prompt-outcome-first-communication-style` (v2.1.235+) и
  `system-prompt-writing-for-the-user` (v2.1.247+) [РЕВЕРС].
- Почему «быть кратким» не срабатывает в агентной работе: действующий дефолт прямо ставит читаемость
  выше краткости — «Being readable and being concise are different things, and readable matters
  more… The way to keep output short is to be selective about what you include… not to compress the
  writing into fragments» [РЕВЕРС]. Плюс «Writing for the user» разрешает списки, таблицы и до трёх
  заголовков в сообщении >500 слов. То есть без Concise-стиля модель следует инструкции, которая
  разрешает длину — и это официально усилено верхнеуровневой склонностью Opus 5 к длинным ответам.

## 5. Хуки и постобработка финального ответа

- `Stop` (и `SubagentStop`) получают `last_assistant_message` — полный текст финального ответа.
  Могут вернуть `decision: "block"` + `reason`, что заставит Claude продолжить (например,
  переписать ответ короче). Лимит — 8 подряд блокировок, флаг `stop_hook_active`.
  Поддерживают `type: "prompt"` (оценка через Haiku) и `type: "agent"`.
- `MessageDisplay` — единственный хук, который может подменить **отображаемый** текст через
  `hookSpecificOutput.displayContent`. «Display-only: the transcript and what Claude sees keep the
  original». Не поддерживает `prompt`/`agent`, таймаут 10 с, срабатывает на каждый стрим-батч.
- Ни один хук не может переписать финальный ответ *в транскрипте*. `PostToolUse` к тексту ответа
  отношения не имеет.

## Практический вывод

Реально работающий механизм для 2.1.263 — `"outputStyle": "Concise"` в settings (встроенный стиль
с явным приоритетом над остальным промптом). Кастомный стиль имеет смысл только с
`keep-coding-instructions: true` и формулировками «делай Y», без числовых лимитов строк.
CLAUDE.md для этой задачи — самый слабый рычаг.
