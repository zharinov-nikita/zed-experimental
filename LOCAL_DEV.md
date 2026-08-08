# Локальная разработка Zed на Windows (личная шпаргалка)

> Локальный файл, не для push. Подробности для Claude — в `.claude/skills/zed-local/SKILL.md`.

## Быстрая сборка (не лагает)

```powershell
cargo run --profile release-fast          # собрать и запустить
cargo build --profile release-fast --package zed   # только собрать
```

- Бинарник: `target\release-fast\zed.exe` — **малиновая иконка** = ваша локальная сборка.
- Профиль `release-fast`: оптимизации как в release, но линкуется быстро (без LTO). Задача есть и в Zed: `.zed/tasks.json`.
- ⚠️ Не задавайте глобальный `RUSTFLAGS` — сломает сборку (перебьёт обязательные флаги из `.cargo/config.toml`).

## Параллельные задачи в worktree

```powershell
.\script\new-worktree.ps1 фича1              # новая ветка "фича1" + worktree ..\zed-фича1
.\script\new-worktree.ps1 фича1 имя-ветки    # из существующей ветки
```

Скрипт создаёт изолированный data-dir `%LOCALAPPDATA%\Zed-Local\фича1` (настройки/кеймапы общие через junction на `%APPDATA%\Zed`, а база/окна/расширения — свои). Запуск из worktree:

```powershell
cargo run --profile release-fast -- --user-data-dir "$env:LOCALAPPDATA\Zed-Local\фича1"
```

Несколько экземпляров Zed работают одновременно и независимо (dev-канал не имеет single-instance блокировки). Первая сборка нового worktree ускоряется sccache. Удаление: `git worktree remove ..\zed-фича1` + удалить data-dir.

## Иконки

- Цвета официальных каналов: чёрный = stable, синий = preview, тёмно-фиолетовый = nightly, серый = обычный dev. Локальная сборка — **малиновый**.
- Перекрасить (например, свой цвет для worktree): см. секцию Recolor в `.claude/skills/zed-local/SKILL.md` (скрипт `recolor.py`, сдвиг тона от preview-иконки).
- После замены иконки перед сборкой: `(Get-Item crates\zed\build.rs).LastWriteTime = Get-Date` — иначе cargo может не перевстроить ресурсы.

## sccache

Установлен, включён через пользовательские env (`RUSTC_WRAPPER=sccache`, `SCCACHE_CACHE_SIZE=40G`). Кеширует release-сборки между worktree. Статистика: `sccache --show-stats`.

## Локальные коммиты в main

Иконки, `script/new-worktree.ps1`, skill и этот файл — локальные коммиты, **не пушить**. Для PR ветвиться от `origin/main` или исключать эти коммиты (cherry-pick/rebase).
