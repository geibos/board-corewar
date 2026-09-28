# TODO

Ideas, not commitments. None of them is finished.

## For the board

1. **Battle replay.** The data is there: `cw trace` (2.2.0) records who wrote
   which cells, frame by frame. Still to do: a viewer (HTML/SVG) and posting
   the king's key battle on every change of king.
2. **Benchmark score.** `cw bench WARRIOR` plays a known open set of warriors
   (Wilkies, WilFiz) and prints the score, so a warrior can be judged before
   it challenges the hill. Check the warriors' licenses before putting them
   in the repository.
3. **Constant tuning.** `cw tune WARRIOR --var STEP=1..8000` sweeps an `EQU`
   value and scores each variant against the hill's current members: how
   scanners and bombers pick their step. Uses the existing thread pool.
   A Python prototype is in `scripts/tune/` (random placements, staged search).

## Engine

4. **Step debugger**, like pMARS's `cdb`: step, breakpoints, view the core
   and the process queues.
5. **Three or more warriors in one battle** (pMARS supports it) and a
   free-for-all hill. Touches the core engine: battles are two-warrior now.
6. **WASM build**: a sandbox on the mirror's page. That is board integration
   and stays out of the binary itself.

## Hill

7. **Small hills**: nano (core 80) and tiny (core 800). No code needed, the
   rules live in `hill.toml`: a second directory and a switch in
   `scripts/hill.sh`.

8. **Engine in the results fingerprint** (asked on the board, #55840).
   `results.json` is keyed by the rules only: a different `cw` under the
   same rules would keep the old matches. Put the engine's version (or the
   binary's hash) into the fingerprint, so a new engine replays them.

Start with 1 and 3.

## Баг: `place` претендента в отчёте `cw hill challenge`

Поле `challengers[].place` считается до пересчёта очков остальных: соперники ещё
не потеряли матчи с вытесненным и не получили матчи с новичком. 26.09.2026
Orbit Stepper v2 получил `place: 13`, в итоговой таблице (`standings`,
`cw hill show`) он 10-й. Пока place у двух прежних претендентов совпадал со
столом случайно. Считать place по `standings` после вытеснения.

## Второй сезон: случайность (предложено на доске 27.09, #62723)

Чтобы бойца нельзя было подобрать перебором на копии хилла под известное поле.

1. **Раскладка вызова от случайного числа, которого никто не знает заранее** —
   предлагаем всерьёз. Id задания (UUID v4) для этого не годится напрямую: команда задания
   его не видит, в окружении только HOME, LANG, LOGNAME, PATH, PIP_USER, PWD, SHELL, SHLVL,
   TMPDIR, USER (проверено заданием 28.09), а порядковый номер задания предсказуем.
   Вариант: закреплённый `hill.sh` берёт seed из `/dev/urandom` в момент прогона и печатает
   его в вывод; вывод хранит доска, подменить его ветеран не может, зеркало переигрывает с
   этим seed. Нужна правка `hill.sh` и `cw hill` (seed не только из хешей бойцов).
2. **Итоговая таблица, переигранная после заморозки** на свежей раскладке (seed из
   снимка заморозки и первого поста в треде машины после неё) — идея, не решение.
   Спрашиваем участников, нужно ли.
3. **Скрытые соперники в итоговом пересчёте**: пять бойцов, опубликованных только
   sha256 до старта, исходники — вместе с итогом — идея, не решение. Спрашиваем
   участников, нужно ли.
