# Локальная разработка Zed на Windows (личная шпаргалка)

> Подробности для Claude — в `.claude/skills/zed-local/SKILL.md`.

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

Несколько экземпляров Zed работают одновременно и независимо (dev-канал не имеет single-instance блокировки). Первая сборка нового worktree — почти холодная, ~35–40 мин (sccache попадает в кеш лишь на ~27%: workspace-крейты привязаны к абсолютному пути); дальше пересборки в worktree быстрые. Удаление: `git worktree remove --force ..\zed-фича1` + удалить data-dir (в git включён `core.longpaths`, без него удаление падает на глубоких путях `target\`).

## Иконки

- Цвета официальных каналов: чёрный = stable, синий = preview, тёмно-фиолетовый = nightly, серый = обычный dev. Локальная сборка — **малиновый**.
- Перекрасить (например, свой цвет для worktree): см. секцию Recolor в `.claude/skills/zed-local/SKILL.md` (скрипт `recolor.py`, сдвиг тона от preview-иконки).
- После замены иконки перед сборкой: `(Get-Item crates\zed\build.rs).LastWriteTime = Get-Date` — иначе cargo может не перевстроить ресурсы.

## sccache

Установлен, включён через пользовательские env (`RUSTC_WRAPPER=sccache`, `SCCACHE_CACHE_SIZE=40G`). Реально помогает при пересборке после `cargo clean` в той же директории и на зависимостях из registry; между worktree выигрыш скромный (~27%). Статистика: `sccache --show-stats`.

## Ветки и форк

Это личный экспериментальный форк `zharinov-nikita/zed-experimental`; PR в оригинальный Zed не планируются. Иконки, `script/new-worktree.ps1`, skill и этот файл коммитятся в ветку `zed-experimental` и пушатся в форк. `main` — чистое зеркало апстрима, свои коммиты туда не добавлять.
