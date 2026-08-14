@.rules

# Личный экспериментальный форк

Это личный экспериментальный форк Zed (github.com/zharinov-nikita/zed-experimental). PR в zed-industries/zed НЕ планируются.

## Ветки

- `main` — чистое зеркало апстрима zed-industries/zed. Свои коммиты сюда не добавлять; обновляется через «Sync fork» на GitHub + `git pull`.
- `zed-experimental` — основная ветка разработки. Все эксперименты и локальные коммиты («Local: …») идут сюда и пушатся в `origin` (форк).
- Подтянуть свежий Zed: обновить `main`, затем rebase/merge в `zed-experimental`.

Локальная шпаргалка по сборке и worktree: `LOCAL_DEV.md`.
