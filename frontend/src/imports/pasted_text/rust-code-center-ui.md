Создай дизайн интерфейса для desktop-приложения на Tauri + React.

Название продукта:
Rust Code Command Center

Главный слоган:
Understand your Rust project as a living system.

Описание продукта:
Rust Code Command Center - это desktop-приложение для визуального анализа Rust-проекта в реальном времени.

Пользователь пишет код, а приложение показывает живую интерактивную карту связей проекта:

* workspace
* crates
* modules
* files
* structs
* enums
* traits
* impl blocks
* functions
* methods
* macros
* external crates
* function calls
* method calls
* type references
* trait implementations
* data flow
* module dependencies
* public API boundaries

Приложение должно ощущаться как командный центр написания кода, а не как обычная IDE и не как редактор диаграмм.

Это не “рисовалка графов”.
Это живой навигатор по смысловым связям Rust-кода.

Пользователь не строит схему вручную.
Приложение автоматически анализирует Rust-проект через Rust backend и rust-analyzer, а frontend показывает красивый интерактивный граф.

Главная идея:
Интерфейс должен помогать человеку с плохой памятью видеть все важные связи на экране. Если связей слишком много, пользователь должен иметь возможность быстро свернуть лишнее, сфокусироваться на нужной функции, модуле, trait или struct, закрепить важные узлы и вернуться к предыдущему контексту.

Продукт должен быть похож по ощущению на Obsidian Graph View, но намного более информативный, структурированный, красивый и приспособленный под Rust.

Общее настроение дизайна:

* Современный dark UI.
* Красиво, быстро, технологично.
* Визуально ближе к Obsidian, Linear, JetBrains Fleet, GitKraken, Graphite.
* Центр внимания - живой плавающий граф.
* Интерфейс должен выглядеть как инструмент для senior-разработчика.
* Никакой учебной игрушечности.
* Никакой перегруженной IDE-табличности.
* Много воздуха.
* Мягкие панели.
* Аккуратные линии.
* Плавные анимации.
* Быстрая реакция интерфейса.
* Сильное ощущение контроля над сложным проектом.

Технический стек frontend:

* React.
* TypeScript.
* Tauri desktop app.
* WebGL-based graph rendering.
* Reusable React components.
* Rust backend выполняет тяжелый анализ.
* rust-analyzer используется для семантического анализа Rust-кода.
* Frontend получает graph snapshot и graph diff.
* Frontend отвечает за визуализацию, фильтрацию, layout, interaction и состояние UI.
* Интерфейс должен быть рассчитан на 60 FPS при больших графах.
* Все состояния loading, updating, stale, error должны быть предусмотрены.

Размер макета:

* Основной desktop layout: 1440x900 px.
* Также предусмотреть адаптацию на 1920x1080 px.

Цветовая схема:

* Основной фон: #0B0F14.
* Панели: #111820 или #131C26.
* Поверхности второго уровня: #182230.
* Основной текст: светло-серый, почти белый.
* Вторичный текст: серо-синий.
* Акцент: cyan / blue-violet gradient.
* Ошибки: мягкий red.
* Предупреждения: amber.
* Успешные состояния: green.
* Не использовать кислотные цвета.
* Не использовать белую тему.
* Не использовать яркий Material Design.

Типографика:

* Основной UI font: Inter или Geist.
* Кодовые элементы: JetBrains Mono.
* Размеры текста должны быть аккуратные и читаемые.
* Названия узлов не должны перегружать граф.
* Для второстепенных узлов подписи скрываются.
* При hover показывается tooltip.

Основной экран:
Экран делится на пять зон:

1. TopToolbar
   Верхняя панель управления.

2. ProjectExplorer
   Левая панель проекта.

3. LiveCodeGraph
   Центральная область с живым графом.

4. InspectorPanel
   Правая панель контекста.

5. AnalysisTimeline
   Нижняя панель событий анализа.

Структура экрана:

* Левая панель шириной около 280 px.
* Правая панель шириной около 340 px.
* Нижняя панель высотой около 120 px, по умолчанию может быть свернута.
* Центральная область занимает максимум пространства.
* Панели не должны перекрывать граф.
* Граф должен быть главным объектом экрана.

TopToolbar:
Верхняя панель должна содержать:

* Название проекта.
* Статус rust-analyzer: Ready, Indexing, Error.
* Индикатор последнего анализа: Updated 2s ago.
* Переключатель режимов графа.
* Search bar с placeholder: Search symbol, file, trait, function…
* Кнопка Recenter.
* Кнопка Collapse.
* Кнопка Focus Mode.
* Кнопка Export.
* Кнопка Settings.

Graph mode switcher:
В верхней панели должен быть явный переключатель режимов:

* Macro
* Meso
* Micro
* Call Flow
* Data Flow
* Traits & Impl

Текущий режим должен быть визуально очевиден.

LiveCodeGraph:
Центральная область - главный элемент приложения.

Граф должен быть force-directed, похожий по ощущению на Obsidian Graph View, но более структурированный и информативный.

Граф должен выглядеть как плавающая data flow диаграмма, адаптированная под Rust.

Узлы должны мягко светиться.
Связи должны быть направленными.
Активные связи должны подсвечиваться.
Неактивные связи должны приглушаться.
Граф не должен превращаться в хаотичную “лапшу”.

Нужны:

* edge filtering
* node filtering
* depth filtering
* edge bundling visual style
* fade inactive links
* collapsible groups
* focus mode
* minimap
* breadcrumbs
* hover tooltips
* pinning
* bookmarks
* focus history

Типы узлов:

* File
* Module
* Struct
* Enum
* Trait
* Impl
* Function
* Method
* Macro
* ExternalCrate

Визуальный стиль узлов:

* File: прямоугольник с закруглением.
* Module: крупный контейнер или hex-like node.
* Struct: rounded rectangle.
* Enum: diamond-like или pill shape.
* Trait: outline node.
* Impl: маленький связующий node.
* Function: круглый node.
* Method: круглый node с маленькой точкой.
* Macro: node с иконкой молнии.
* ExternalCrate: приглушенный node с пунктирной обводкой.

Типы связей:

* Contains
* Uses
* Calls
* Implements
* TypeReference
* DataFlow
* ModDeclaration
* ExternalDependency

Визуальный стиль связей:

* Contains: тонкая нейтральная линия.
* Calls: яркая направленная линия.
* Implements: пунктирная линия.
* TypeReference: мягкая синяя линия.
* DataFlow: более толстая линия с subtle animated pulse.
* Uses: серо-синяя линия.
* ModDeclaration: тонкая направленная линия.
* ExternalDependency: приглушенная линия.

Поведение графа:

* Hover по узлу подсвечивает все прямые связи.
* Click по узлу открывает подробности справа.
* Double click фокусирует подграф вокруг узла.
* Узлы можно pin-ить.
* Группы можно сворачивать.
* Можно скрывать типы узлов.
* Можно скрывать типы связей.
* Можно переключать глубину отображения: Depth 1, Depth 2, Depth 3, Full.
* При выборе функции можно показать Who calls this.
* При выборе функции можно показать What this calls.
* При выборе функции можно показать Related types.
* При выборе функции можно показать Trait bounds.
* При выборе функции можно показать Data dependencies.
* При перегрузе графа показывать suggestion card: Graph is dense. Reduce visual noise?
* В правом нижнем углу центральной области нужна MiniMap.

Три уровня отображения графа:

1. Macro View

Purpose:
Показать высокоуровневую архитектуру проекта.

Visible entities:

* Workspace
* Crates
* Modules
* Files
* External crates
* Public API boundaries

Main use:
Пользователь понимает структуру проекта, границы модулей и направление зависимостей.

Visual style:

* Крупные сгруппированные узлы.
* Modules показываются как мягкие контейнеры.
* Files показываются как меньшие узлы внутри module groups.
* External crates приглушены и расположены ближе к внешней границе графа.
* Labels видны только для важных узлов и текущего focus context.

2. Meso View

Purpose:
Показать основную семантическую структуру внутри modules и files.

Visible entities:

* Structs
* Enums
* Traits
* Impl blocks
* Functions
* Methods
* Type references

Main use:
Пользователь понимает, как Rust-сущности связаны внутри module или crate.

Visual style:

* Structs, enums и traits визуально различаются.
* Traits используют outline nodes.
* Impl blocks работают как connector nodes между types, traits и methods.
* Functions и methods меньше по размеру.
* Связанные entities группируются рядом.
* Выбранный module или file остается визуально в центре.

3. Micro View

Purpose:
Показать детальное локальное поведение кода вокруг выбранного symbol.

Visible entities:

* Function calls
* Method calls
* Input types
* Return types
* Result / Option flow
* Async boundaries
* Unsafe blocks
* Trait bounds
* Data flow edges
* Error propagation
* Local dependencies

Main use:
Пользователь понимает, от чего зависит конкретная function, method, trait или type.

Visual style:

* Selected node находится в центре.
* Direct incoming и outgoing connections подсвечены.
* Data flow edges толще и имеют subtle animated movement.
* Менее важные nodes уходят в background.
* Интерфейс избегает visual noise и сохраняет readability.

Focus Bubble:
Добавить специальный режим взаимодействия Focus Bubble.

Purpose:
Помочь пользователю с плохой памятью держать на экране только нужный контекст кода.

Как работает:
Когда пользователь выбирает symbol, приложение создает сфокусированный визуальный bubble вокруг него.

Для выбранной function или method Focus Bubble показывает:

* Selected function или method в центре.
* Functions that call it.
* Functions and methods it calls.
* Input types.
* Return type.
* Related structs and enums.
* Related traits and impl blocks.
* Error paths, если function возвращает Result.
* Async boundary, если function async.
* Unsafe marker, если unsafe involved.
* File and module context.

Для выбранного struct Focus Bubble показывает:

* Fields.
* Impl blocks.
* Methods.
* Trait implementations.
* Where the struct is used.
* Constructors and factory functions, если detected.
* Serialization или database related traits, если detected.

Для выбранного trait Focus Bubble показывает:

* Required methods.
* Provided methods.
* Implementors.
* Where the trait is used as a bound.
* Where it is used as a trait object.
* Related generic constraints.

Focus Bubble behavior:

* Все вне bubble затемняется.
* Пользователь выбирает depth: 1, 2, 3 или Full.
* Пользователь может pin-ить nodes внутри bubble.
* Пользователь может collapse branches внутри bubble.
* Пользователь может вручную expand hidden branches.
* Пользователь может lock bubble while navigating code.
* Пользователь может создать несколько pinned bubbles для сравнения.
* Пользователь может вернуться к прошлым bubbles через focus history.

Focus Bubble controls:

* Create Focus Bubble
* Expand Depth
* Collapse Noise
* Pin Context
* Hide External
* Show Data Flow
* Show Callers
* Show Callees
* Explain This Subgraph
* Why Is This Connected?

UX для плохой памяти:
Интерфейс должен активно помогать пользователю не запоминать все вручную.

Добавить:

* Breadcrumb текущего focus.
* You are here marker.
* Focus history.
* Pinned symbols.
* Bookmarked nodes.
* Sticky notes attached to nodes.
* Recent focus list.
* Recently changed symbols.
* Current file context.
* Current module context.
* One-click collapse of unrelated graph areas.
* One-click restore of previous graph state.

ProjectExplorer:
Левая панель шириной около 280 px.

Секции:

* Workspace
* Crates
* Modules
* Recently touched
* Bookmarks
* Hotspots

В дереве проекта показывать:

* src/
* modules
* files
* symbols

У каждого файла должны быть маленькие badges:

* functions count
* links count
* diagnostics count
* complexity indicator

Кнопки:

* Focus current file
* Show only current crate
* Hide external crates
* Collapse all
* Expand all

ProjectExplorer должен помогать быстро перейти от дерева проекта к графу.

InspectorPanel:
Правая панель шириной около 340 px.

Когда ничего не выбрано, показывать project overview:

* Nodes count
* Edges count
* Crates count
* Hotspots
* Last analysis
* Top connected modules
* Recent changes
* Analyzer status

Когда выбран узел, показывать карточку selected element:

* Name
* Kind
* File path
* Line number
* Signature
* Visibility: pub, pub(crate), private
* Rust attributes
* Generic params
* Trait bounds
* Incoming links
* Outgoing links
* Related symbols

Для function показывать:

* Calls
* Called by
* Input types
* Output type
* Possible errors / Result type
* Async badge
* Unsafe badge
* Generic badge
* Current module
* Current file
* Open in editor action
* Create Focus Bubble action

Для struct показывать:

* Fields
* Impl blocks
* Trait implementations
* Methods
* Used by
* Constructors
* Related modules

Для trait показывать:

* Required methods
* Provided methods
* Implementors
* Where used
* Object safety indicator
* Generic bounds
* Trait object usages

Для module показывать:

* Children
* Imports
* Exports
* Internal density
* External dependencies
* Public API
* Private symbols

InspectorPanel должен быть визуально похож на command palette + inspector.
Использовать карточки, badges и компактные списки.

AnalysisTimeline:
Нижняя панель высотой около 120 px.
По умолчанию может быть свернута.

Показывает:

* rust-analyzer indexing status
* file change events
* graph update events
* errors
* warnings
* unresolved symbols
* slow analysis notices
* stale graph warnings

Фильтры:

* All
* Errors
* Warnings
* Analyzer
* Graph

AnalysisTimeline не должна занимать слишком много внимания.
Она нужна как technical activity log.

Floating FilterBar:
Поверх графа нужна компактная панель фильтров.

Фильтры:

* Node types
* Edge types
* Visibility
* Crate boundary
* Only current file
* Only public API
* Hide tests
* Hide external crates
* Hide generated / macro nodes
* Depth 1
* Depth 2
* Depth 3
* Full

Dense graph handling:
Если граф становится слишком плотным, приложение не должно показывать хаос.

Dense graph state показывает мягкую suggestion card:

Text:
Graph is dense. Reduce visual noise?

Actions:

* Collapse modules
* Show current crate only
* Hide external crates
* Hide tests
* Hide private symbols
* Show only Depth 2
* Switch to Focus Bubble

Граф должен поддерживать:

* Collapsible module groups.
* Collapsible file groups.
* Collapsible trait implementation groups.
* Edge filtering.
* Node type filtering.
* Depth filtering.
* Current file only mode.
* Public API only mode.
* Tests hidden mode.
* External crates hidden mode.

Graph modes:

1. Architecture Mode
   Показывает high-level modules, files, types и dependencies.

2. Call Flow Mode
   Показывает functions, methods и calls.

3. Data Flow Mode
   Показывает движение данных между functions, structs и modules.
   Главный визуальный эффект - animated flowing edges.

4. Traits & Impl Mode
   Показывает traits, impl blocks, implementors и generic constraints.

5. Modules Mode
   Показывает module tree и use dependencies.

6. Macro View
   Показывает workspace, crates, modules, files и external crates.

7. Meso View
   Показывает structs, enums, traits, impls, functions и methods.

8. Micro View
   Показывает локальный контекст вокруг selected symbol.

Состояния приложения:

1. Empty State
   Пользователь еще не открыл проект.

Показать большую центральную карточку:
Open Rust workspace
Connect to rust-analyzer
View live code graph

Визуально:

* Темный фон.
* Красивый abstract graph preview.
* Большая primary button: Open Rust Workspace.
* Secondary action: Open Recent Project.

2. Indexing State
   Показывается во время индексации.

Text:
Indexing workspace with rust-analyzer…

Визуально:

* Красивая анимация появления узлов.
* Skeleton panels.
* Analyzer status в TopToolbar.

3. Normal State
   Граф активен.
   Панели заполнены.
   Можно выбирать nodes, фильтровать, создавать Focus Bubble.

4. Dense Graph State
   Если слишком много связей, показывается suggestion card.
   Граф остается видимым, но интерфейс предлагает снизить шум.

5. Error State
   Если rust-analyzer недоступен или проект не может быть проанализирован, интерфейс не должен ломаться.

Text:
rust-analyzer is unavailable.
Using syntax graph fallback.

Показать actions:

* Retry
* Open Settings
* Use Syntax Graph
* View Logs

Fallback mode должен выглядеть аккуратно и понятно.

React component requirements:
Спроектировать интерфейс как набор reusable React components.

Нужные компоненты:

1. AppShell
   Общий layout приложения.

2. TopToolbar
   Project name, analyzer status, graph mode switcher, search, recenter, collapse, focus mode, export, settings.

3. ProjectExplorer
   Левая панель с workspace, crates, modules, files, symbols, recent changes, bookmarks и hotspots.

4. LiveCodeGraph
   Центральная graph area.

5. GraphNode
   Reusable node component с variants:

* File
* Module
* Struct
* Enum
* Trait
* Impl
* Function
* Method
* Macro
* ExternalCrate

6. GraphEdge
   Reusable edge component с variants:

* Contains
* Uses
* Calls
* Implements
* TypeReference
* DataFlow
* ModDeclaration
* ExternalDependency

7. FocusBubbleOverlay
   Visual bubble вокруг selected context.

8. InspectorPanel
   Правая панель с selected symbol details.

9. SymbolCard
   Карточка для functions, structs, traits, impls и modules.

10. FilterBar
    Floating graph filter controls.

11. MiniMap
    Маленькая карта графа в bottom-right части graph area.

12. AnalysisTimeline
    Нижняя панель с analyzer events, graph updates, warnings и errors.

13. DenseGraphSuggestion
    Карточка, которая появляется при перегруженном графе.

14. EmptyState
    Состояние без открытого Rust workspace.

15. IndexingState
    Состояние во время rust-analyzer indexing.

16. ErrorState
    Состояние ошибки rust-analyzer или анализа проекта.

17. FocusHistoryPanel
    Панель истории фокуса.

18. BookmarkList
    Список закрепленных symbols.

19. StickyNote
    Заметка, прикрепленная к узлу.

20. BreadcrumbBar
    Путь текущего focus context.

Mini components:
Также сделать отдельные мини-компоненты:

* Node styles
* Edge legend
* Inspector cards
* Filter chips
* Status badges
* Search bar
* Graph mode switcher
* Tooltip
* Context menu
* Collapse button
* Pin button
* Bookmark button
* Analyzer status badge
* Complexity badge
* Visibility badge
* Async badge
* Unsafe badge
* Generic badge

Context menu для node:
При right click по node показать:

* Focus here
* Create Focus Bubble
* Pin node
* Bookmark
* Hide unrelated
* Show callers
* Show callees
* Show data flow
* Show trait context
* Explain this subgraph
* Why is this connected?
* Open in editor

Search:
Search bar должен искать:

* symbols
* files
* modules
* traits
* functions
* methods
* external crates

Search results должны показываться как command palette:

* symbol name
* kind
* file path
* module
* badges
* action to focus in graph

Hotspots:
В интерфейсе нужна секция Hotspots.

Hotspot - это место в проекте, где много связей или высокая сложность.

Показывать:

* top connected modules
* top called functions
* files with many diagnostics
* dense trait areas
* large impl blocks
* possible architecture bottlenecks

Hotspots должны помогать пользователю понять, где проект сложный.

Visual priority:

* Граф главный.
* Панели поддерживают граф.
* Панели не должны конкурировать с графом.
* Фильтры должны быть доступны, но не должны занимать много места.
* Inspector должен давать быстрый смысловой контекст.
* ProjectExplorer должен быстро переводить от файлов к графу.
* Timeline должен быть технической нижней панелью, а не главным элементом.

Визуальный стиль главного экрана:

* Темный фон.
* В центре большой красивый интерактивный граф.
* Узлы мягко светятся.
* Связи имеют направление.
* Активные связи подсвечиваются.
* Неактивные связи приглушаются.
* Панели имеют border 1px.
* Панели имеют soft shadow.
* Radius 12-18 px.
* Иконки минималистичные.
* Нет белой темы.
* Нет яркого Material Design.
* Нет перегруза текстом.
* Названия узлов видны только для важных или выбранных узлов.
* При hover появляется tooltip.

Figma deliverables:
Сделать в Figma следующие экраны:

1. Main screen
   Нормальное состояние приложения с открытым Rust-проектом и активным графом.

2. Selected function state
   Показать выбранную function в графе, подсвеченные callers, callees, related types и правую InspectorPanel.

3. Selected trait state
   Показать выбранный trait, implementors, required methods, trait bounds и InspectorPanel.

4. Focus Bubble state
   Показать центральный bubble вокруг выбранной function или struct.
   Вне bubble все затемнено.
   Внутри bubble видны только релевантные связи.

5. Dense graph state
   Показать перегруженный граф и suggestion card с actions для уменьшения visual noise.

6. Empty state
   Показать экран без открытого проекта.

7. Indexing state
   Показать rust-analyzer indexing.

8. Error state
   Показать fallback, если rust-analyzer недоступен.

9. Settings modal
   Показать настройки анализа, graph rendering, filters и performance.

10. Command palette search
    Показать поиск symbols и быстрый переход к node.

Figma components:
Сделать компоненты:

* AppShell
* TopToolbar
* ProjectExplorer
* LiveCodeGraph
* GraphNode variants
* GraphEdge variants
* FocusBubbleOverlay
* InspectorPanel
* SymbolCard
* FilterBar
* MiniMap
* AnalysisTimeline
* DenseGraphSuggestion
* EmptyState
* IndexingState
* ErrorState
* BreadcrumbBar
* SearchCommandPalette
* StatusBadge
* VisibilityBadge
* AsyncBadge
* UnsafeBadge
* GenericBadge
* ComplexityBadge
* FilterChip
* Tooltip
* ContextMenu

Главное ощущение:
Пользователь должен открыть Rust-проект и почувствовать, что он видит живую систему кода.
Не просто файлы.
Не просто дерево.
Не просто граф.
А понятную визуальную карту, где видно:

* где я сейчас
* кто с кем связан
* что вызывает что
* какие типы задействованы
* какие traits реализуются
* куда течет data flow
* какие места сложные
* что можно свернуть
* что можно закрепить
* куда перейти дальше

Приложение должно давать ощущение:
“Я управляю сложным Rust-проектом с одного экрана.”
