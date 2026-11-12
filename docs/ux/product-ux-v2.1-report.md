# Product UX v2.1 — отчёт разработки

Baseline: `3ffcb30f313816ed79dd6b3ce8d64fa71b715378` (Product UX v2).
Ветка: `feat/product-ux-v2.1`. Итоговый SHA Rust-реализации:
`ceae1ca635a78910f7e997ab2b74725a8c989dfb`. SHA документационной поставки записан после её коммита в
`tmp/tui-qa/product-ux-v2.1/RESULTS.json` (самоссылка SHA в коммит невозможна). Исходная ветка и другие worktree не изменялись; merge/push не выполнялись.

**Статус: разработка и проверки разработчика выполнены. Независимая приёмка
OpenClaw NOT RUN; её вердикт отсутствует.** Этот отчёт не объявляет ACCEPT.

## Что изменилось

- В `.gitignore` этого репозитория добавлено только `/tmp/tui-qa/`. Определения
  метрик, общий scanner и правила пользовательских репозиториев не менялись.
- Transient action notice очищается при фактической смене View и начале успешной
  инспекции/поиска. Ошибки backend сохраняются отдельно. Поздняя ошибка поиска
  сохраняется в Analysis, но не открывает старое уведомление на другой вкладке.
- Symbols и Calls без выбора занимают экран маршрутами, назначением выделенного
  маршрута, доступностью, требованиями RA и следующим действием. Symbols предлагает
  поиск первым; Calls — executable, известную функцию, поиск и предыдущую функцию.
  Отсутствующая предыдущая функция — локальное Unavailable. Поиск без RA больше
  не сообщает «No symbols found» как будто выполнен успешный запрос.
- Explorer использует тот же RepositoryTree: кэширует число **прямых** каталогов,
  Rust-файлов, других файлов и symlinks при построении существующего индекса.
  Показывает ограниченный стабильный список детей и действия. Выбор не запускает
  повторного сканирования. Каталог не объявляется Rust-модулем.
- Overview ставит четыре задачи выше **Additional data**. Рейтинги, glossary,
  packages/languages и Analysis доступны после одного раскрытия; каждый рейтинг
  раскрывается отдельно. Review открывает один подходящий список, а не четыре.
  Snapshot показывает scanned files, Rust functions и review candidates; co-change
  partial обозначается отдельно и не делает полными/неполными другие измерения.
- Module Inspector сначала показывает идентичность, структуру, тип и ограничения
  связей, несколько измеренных функций и действия. `a` открывает сохранённую
  подробную диагностику; `f` — файлы. `a details` видна и в нижней строке 80×20.
  Coverage, resolution, unclassified, conditional, losses и unknown own-module
  LOC сохранены. Раскрытие сбрасывает только scroll Inspector, не Focus/Selection.
- Help для группы Git Hotspots объясняет changes/churn/co-change/candidates;
  справка Partial/Unknown больше не повторяет один раздел дважды.

Новых runtime dependencies нет; Cargo.toml/Cargo.lock не изменены. CLI JSON schema
v1, нормализованные модели и анализаторы сохранены. Новых LSP/AST/Git-запросов в
render или при обычном перемещении Selection нет.

## Проверенная разница метрик

Изолированный эксперимент использовал **один и тот же закреплённый бинарник v2**
до/после единственного изменения ignore, до добавления кода и тестов v2.1.
Сохранены полные `stats`, `maintainability`, `hotspots` JSON и список реально
существовавших QA-файлов в `tmp/tui-qa/product-ux-v2.1/{before,after}/`.

| Измерение | До ignore | После ignore | Объяснение |
|---|---:|---:|---|
| Все файлы | 858 | 270 | Исключены 588 QA-файлов |
| Исходные файлы | 194 | 95 | 88 Rust-копий и 11 Python QA-скриптов |
| Source physical LOC | 54 228 | 27 452 | Исключены 26 776 физических строк |
| Source nonblank LOC | 52 774 | 26 719 | Исключены 26 055 непустых строк |
| Проанализированные Rust-файлы | 180 | 92 | Исключены 88 Rust-копий |
| Функции | 1 789 | 904 | В исключённых Rust-копиях 885 функций |
| Median / P95 | 14 / 92 | 14 / 92 | Формула не менялась |
| Max control depth | 8 | 8 | Определение не менялось |
| Review candidates | 25 | 25 | Git настоящих файлов не изменился |

Разница source totals независимо сходится с содержимым исключённых файлов.
Root `.gitignore` сам добавил одну непустую строку к общим text totals.
`partial=false` Repository/AST и `coChangePartial=true` сохранены. QA-копии исчезли
из largest/longest/deepest rankings; настоящие пути сохранены.

Regression-тест добавляет огромную игнорируемую копию и сравнивает всё дерево,
serialized metrics и maintainability до/после; настоящие `tmp/real.rs`,
`tests/fixtures/real.rs` и вложенный `nested/tmp/tui-qa/real.rs` остаются.
Финальные JSON имеют другие чистые totals вследствие нового настоящего кода и
тестов; новые локальные коммиты также закономерно меняют Git history/candidates.
Их нельзя подменять приведённым изолированным экспериментом.

## Метрики и интерпретация

Сохранены WHAT/WHY/LIMITATIONS/NEXT для Source/Physical LOC, функций,
median/P95, depth/control-flow nodes, Git changes/churn/co-change, candidates,
imports, coverage/resolution, calls и partial/truncated/unknown.

В реальном UI проверено `Depth 8`: вложенность if/match/for/while/loop, а не
цикломатическая сложность или доказательство ошибки; предлагается открыть точный
исходник. P95 объясняется через долю длин **at or below**, imports — синтаксические
records, Calls — статические связи RA. Repository source LOC и known physical
subtree file LOC имеют разные области; exclusive inline totals остаются unknown.
Общий Quality/Risk Score не добавлен.

## Пользовательские маршруты

Проверки проведены разработчиком, знакомым с кодом и прочитавшим обязательные
документы. Это не слепое исследование новичков и не независимый UX-test OpenClaw.

| Сценарий | Результат реального PTY | Наблюдение |
|---|---|---|
| A: entrypoint → Calls → Graph → expand → source → back | PASS | RA подтвердил main, раскрыт run, точный src/ingest.rs; Esc восстановил граф |
| B/C: module → relationships → function → Calls → back | PASS | Восстановлены crate::decode, Focus и Imports после одного Esc |
| B: поиск switch_view → Inspector → Calls/source | PASS | Exact App::switch_view в src/tui/app.rs; без предположений по одному имени |
| D: Depth 8 → Details / ? | PASS | Измерение, ограничения и следующий шаг видны в приложении |
| E: Review → evidence → p → точный исходник | PASS | Коммиты/LOC/length/depth подтверждены моделью; интерфейс не объявляет дефект |

По отдельным нажатиям (ожидание/чтение не считается) execution до Graph остаётся
3 действия: Enter, Enter, g — как в v2. На контролируемом проекте расширение
соседа, исходник и возврат добавляют j, x, o, Esc (всего 7). Архитектурный маршрут
j Enter j Enter v o Enter c Esc занимает 9 действий и сохраняет прежнюю семантику.
Review → evidence — 4, до выбранного через p Inspector — 6, как в v2.
Depth через glossary теперь 12 действий вместо 10: добавилось явное раскрытие
Additional data. Это осознанная цена меньшего количества технических строк на
первом экране, а не заявка на сокращение всех маршрутов.

При обычном переходе цифрой 4 из Calls действует прежняя pane/Esc-модель Explorer:
Esc может закрыть Inspector до списка символов файла. История возвращает к Calls
в явном маршруте `o source → Esc`. Произвольные переключения вкладок не добавляются
в семантическую историю; это ограничение сохранено и показано отдельным снимком.

## Реальные терминалы и evidence

Приложение запускалось в настоящем PTY. Сохранены raw ANSI, события клавиш,
декодированные экраны и PNG из ячеек вывода работающего приложения. Это не
TestBackend и не синтетические макеты. PNG-рекордер сохраняет моноширинный текст;
полная ANSI-информация остаётся в raw log.

Проверены 80×20, 100×30, 120×35 и 160×45. Resize в одном сеансе сохраняет module
Focus/Selection/Imports. Self rust_watcher, доверенный workspace_project (два
крейта), контролируемый intake_demo (пять Rust-файлов/шесть функций, без build.rs),
пустой Cargo workspace. Недоверенные сторонние build scripts не запускались.
Отдельно проверены отсутствие RA, Git, обоих, отсутствие entrypoint и индексирование.

Curated screenshots в `tmp/tui-qa/product-ux-v2.1/screenshots/`, соответствующие
экраны в `screens/`, provenance — `capture-manifest.json`.

| Экран | Главное / понимание / следующий шаг | Оценка |
|---|---|---|
| 01 Overview | Четыре исследовательских задачи; Enter начинает | Рейтинги не вытесняют задачи |
| 02 Выбранный маршрут | Назначение и доступность architecture | Подсказки соответствуют переходу |
| 03 Review evidence | Почему включён файл; p/Enter | Метрики не представлены вердиктом |
| 04 Symbols empty | Выбор маршрута, purpose/RA/Next | Вместо пустого Inspector |
| 05 Symbol Inspector | Точный callable и исходник | Существующая полная инспекция сохранена |
| 06 Calls empty | Executable/known/search/previous | Unavailable локально, не глобальная ошибка |
| 07 Directory | Прямые дети/масштаб/ограниченный список | Данных больше, повторного scan нет |
| 08 Rust file | Metadata и реальные RA symbols | Enter инспектирует точный символ |
| 09 Module primary | Identity/structure/relationships/review/actions | Raw diagnostics убраны из summary |
| 10 Module details | Сохранённые технические измерения | Длинный scrollable режим; unknown сохранён |
| 11 Calls → Explorer → Esc | Прежние file/Inspector pane semantics | Не обещает произвольный tab-history |
| 12 80×20 Overview | Все задачи, status, действие и back | Без наложения текста |
| 13–14 Depth | Краткое измерение и полный glossary | Формула/ограничения/Next доступны |
| 15–16 Graph / module return | Разделение Selection/Focus и возврат | Исходный модуль/режим восстановлены |
| 17–19 Indexing / no RA / small module | Доступные альтернативы; a details | Source/architecture остаются доступны |
| 20 Source → Calls | Явный маршрут возврата | Граф восстановлен |

Первый дополнительный recorder ошибочно ожидал слово FILE после открытия
Inspector и был прерван. Его raw session оставлен для аудита; итоговые результаты
используют исправленное ожидание выбранной вкладки Explorer. Это ошибка сценария,
а не основание объявить приложение сломанным или скрыть неудачный прогон.

## Проверки и performance

Обязательные fmt, clippy `--all-targets -- -D warnings`, cargo test, debug/release
build, diff-check выполнены. Итог: 184 unit/state passed, 14 explicitly ignored;
12 CLI integration passed. Ignore regression, уведомления/late search failure,
empty states, directory scope/resize, module diagnostics/unknown, exact navigation,
Focus/Selection, Graph/List/history проверены. Игнорируемые ручные performance-тесты
не выдаются за выполненные; реальные PTY-замеры сохранены отдельно.

QA Harness: home (с resize и depth), execution, architecture, search, review,
unavailable — PASS. Дополнительные сеансы и performance хранят выходной код,
бинарник и SHA-256. `RESULTS.json` содержит сводку и фактические замеры;
`ACTIONS.jsonl` — реальные события.

Performance сравнивается с бинарником именно `3ffcb30f`, на одном intake_demo,
120×35, три запуска каждой версии в смешанном порядке. Измеряется первый **полный**
декодированный кадр, не первые ANSI-байты. Navigation/tab timing включает доставку
PTY, обработку и декодирование; это не внутреннее время render. RA и планировщик
создают шум; небольшой пилот не доказывает ускорение. Числа и диапазоны представлены
в QA REPORT/RESULTS, чтобы не дублировать изменяемые результаты в двух местах.
Признаков регрессии на порядок величины нет; строгий performance budget этим
экспериментом не доказан. Во время indexing остальные маршруты доступны, завершение
сеансов проверено. В rendering не добавлен синхронный анализ.

## Ограничения и handoff

- Независимый вердикт OpenClaw **NOT RUN**. OpenClaw должен повторить A–E, оценить
  понимание без README и записать ACCEPT / ACCEPT WITH ISSUES / REJECT самостоятельно.
- Нет доказательства улучшения понимания у незнакомых людей: это developer QA.
- На 80×20 длинный Inspector требует Tab/j/k; source preview может быть ниже metadata.
  Подробная диагностика намеренно не умещается целиком на первом экране.
- Карта остаётся ограниченным синтаксическим графом, Calls — статическими RA calls.
  Нет макрораскрытия/cfg evaluation и гарантированного runtime trace.
- AI benchmark не повторялся в этой UI-итерации. Экономия токенов/превосходство wt
  не заявляются; фактической token telemetry в этом прогоне нет.

Изменённые файлы и итоговые SHA перечислены в QA REPORT/RESULTS. Независимая приёмка
остаётся отдельным завершающим этапом, а не следствием зелёных unit-тестов.


## Изменённые файлы

- Ignore/regression: `.gitignore`, `src/repository.rs`.
- UI state/input/help: `src/tui/app.rs`, `event.rs`, `help.rs`, `ui.rs`.
- Exploration starts: `src/tui/journey.rs`, `journey/actions.rs`, `dispatch.rs`,
  `render.rs`, `start.rs`, `tests.rs`; `src/tui/ui/symbols.rs`.
- Directory: `src/tui/explorer.rs`, `src/tui/ui/explorer.rs`.
- Overview: `src/tui/overview/{analysis,details,render,state}.rs`.
- Modules: `src/tui/module_map/{inspector,tests}.rs`.
- Regression/state/render: `src/tui/{product_tests,navigation_tests}.rs`,
  `src/tui/map/tests.rs`.
- Docs/replay: `README.md`, этот отчёт, `docs/ux/scenarios/{home,execution}.json`,
  `scripts/ux_qa.sh`. QA_OUTPUT по умолчанию перенесён в игнорируемый QA-каталог.
