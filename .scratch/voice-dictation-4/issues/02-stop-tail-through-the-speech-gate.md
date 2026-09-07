# 02 — Хвост при остановке через Speech Gate

Спецификация: `.scratch/voice-dictation-4/spec.md`. Словарь: `CONTEXT.md`. Решение: `docs/adr/0002-speech-gate-decides-whether-to-decode.md`.

**What to build:** При остановке Dictation Session хвост режется по концу речи, который сообщил Speech Gate, и только он декодируется; «Thank you» на тишине не из чего расти. Повторное декодирование хвостовых сегментов с гейтом no-speech и трюк с `logprob_thold` из итерации 3 удаляются: у движка одно решение о тишине, а на хвост действует только защита от Decoder Loop. Пример `no_speech_probe` остаётся для будущих проверок, а `transcribe_wav` показывает хвост так, как его теперь видит движок. `LOCAL_DEV.md` больше не описывает гейт no-speech.

**Blocked by:** 01 — Speech Gate в цикле распознавания.

**Status:** done

- [x] Тесты `recognition_loop` про хвост зелёные: остановка посреди фразы сохраняет хвост, итоговый текст совпадает с распознаванием всего файла, после последней цифры ничего нет.
- [x] Кода гейта no-speech и `logprob_thold` в движке нет; примеры собираются.
- [x] Ручная проверка: несколько остановок после паузы в тишине без «Thank you» и подобных фраз при выключенном Post-processing.
- [x] `./script/clippy` и `cargo test -p dictation` зелёные, `LOCAL_DEV.md` обновлён.
