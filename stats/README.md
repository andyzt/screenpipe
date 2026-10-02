# Screenpipe в Traction

Independent Screenpipe module for the shared Traction hub. The UI is English,
uses MultiTool's typography/theme/chart library, and opens with charts before
summary numbers. Seven metric sections: Product, Core loop, Client health,
Models & routing, Spend, Errors & latency, Features. Product plans, competitor
research and build documentation are outside the dashboard.

Periods: **Yesterday**, 1 day, 7 days, 30 days. Yesterday is the complete previous
Moscow calendar date; the journal separately uses its existing 04:00 day boundary.
Daily DAU/WAU/MAU, useful actions, feedback, errors, LLM latency/cost and resource
series are calculated on the server. Version/OS and events/installations breakdowns
are interactive. Missing measurements remain blank; zero-priced attempts remain
zero. No synthetic production data is used.

Product analysis: [PRODUCT_REVIEW.md](PRODUCT_REVIEW.md). Team decision draft:
[USE_CASE_OPTIONS.md](USE_CASE_OPTIONS.md). App and statistics releases are independent.

## Запуск

Python 3.10+, только stdlib. Для сервера требуется SQLite **3.51.3+**
(исправление WAL reset). На сервере со старой системной SQLite соберите
изолированную библиотеку из официального исходника (сверив SHA3) и укажите её
через `LD_LIBRARY_PATH`; системную SQLite не заменяйте.

```bash
STATS_PORT=9913 STATS_DB=/tmp/screenpipe-stats/events.db python3 stats/server.py
python3 -m pytest stats/test_stats.py -q
```

Дашборд: `http://127.0.0.1:9913/`. Все его внутренние URL относительные.
Прод: `https://stats.multitool.works/p/screenpipe/`, за авторизацией хаба.
Стандартные `/health`, `/summary?days=1`, `/summary-batch?days=1,7,30`,
`/period-snapshot`, `/stats-telemetry`; продуктовые `/product?days=7`, `/product?days=yesterday`, `/catalog` (legacy definitions).

## Откуда появятся данные

В доработанной сборке: Настройки → Журнал → Traction. Сбор выключен по умолчанию.
Администратор создаёт случайный ID установки и индивидуальный credential командой
`server.py --provision <opaque-install-id>` в окружении сервиса. Она печатает
ключ один раз: передать его владельцу приватно, не коммитить/не отправлять в чат.
URL, ID и этот credential вводятся в настройках приложения. Общий серверный
`STATS_INGEST_TOKEN` запрещено включать в клиентскую сборку.

**Уже установленные сборки эти изменения не содержат.** Развёртывание дашборда само
по себе не обновляет приложение. Нужна новая сборка и проверка native HTTP.
Подключены явные UI-действия журнала и результаты записывающих API-операций.
Native capture/LLM/MCP/first-run отправители ещё не реализованы; их метрики
обозначены как непокрытые. `measure_client.py` — ручной локальный сборщик,
он ничего не отправляет на сервер.

## Контракт и защита

`schema.py` — единственный allowlist событий и свойств. POST `/events`:
`{"events":[{"event_id":"<UUID>","device_id":"<opaque-install-id>","ts":0,"name":"journal_opened","properties":{"view":"day"}}]}`.
Вместо 0 — реальные UTC epoch seconds. Заголовок `X-Ingest-Token`.
До 100 событий / 64 КиБ, до 600 событий в минуту на credential. Окно backfill
35 суток. Повтор UUID идемпотентен. Чужая установка запрещена. Неизвестное поле,
текст экрана, произвольный URL и сырая ошибка отвергают весь batch.
Клиентская очередь — 100 событий, TTL 7 дней, HTTPS (HTTP только localhost),
retry до 60 с, timeout 5 с, отключение согласия удаляет очередь.

POST `/delete` с `{"device_id":"..."}` и credential этой установки либо серверным
ключом отзывает ключ, удаляет raw и ставит удаление derived в упорядоченную очередь.
Обновление derived асинхронное. Отказ от сбора не удаляет серверную историю.
Прямой API оператора не предназначен для открытой регистрации клиентов.

Идентичность = установка, не человек. DAU = явные действия; фоновый захват не
делает пользователя активным. Окна хаба — московские календарные даты. Журнал
с началом дня в 04:00 использует другую продуктовую ось времени. Определения
активации, D7 и сессии опубликованы в `/catalog`; нулевой знаменатель — `null`.

Core-метрики используют materialized journal из Traction; чтение dashboard
не сканирует raw. Продуктовый кэш пересчитывается раз в 15 с по retained raw.
Это пилотная реализация. Известные ограничения, которые надо закрыть до
расширения пилота (ревью от 02.10.2026):

- продуктовый кэш каждые 15 с перечитывает все raw-события, а старт сервиса
  делает то же самое: при сотнях тысяч событий это секунды CPU и сотни МБ
  памяти, при лимите памяти сервис уйдёт в перезапуски. Нужны инкрементальные
  агрегаты по дням, срок хранения raw и лимит событий на установку;
- автоматической raw retention нет; таблица прогонов метрик растёт без очистки;
- после постоянной ошибки воркер крутится без паузы и может потерять элемент
  очереди удаления до перезапуска;
- ключ установки передаётся в `X-Ingest-Token`: убедитесь, что логи прокси его
  не пишут, или перейдите на `Authorization: Bearer`;
- в настройках клиента нет кнопки «удалить мои данные» (сервер её поддерживает).

Не добавлять сбор содержимого.

## Эксплуатация

Отдельный сервис с собственными данными и runtime, слушает только loopback и
стоит за авторизацией хаба Traction. Запускайте его под отдельным пользователем
с ограничениями памяти и CPU; общий серверный токен хранится только в
окружении сервиса. Ротация индивидуального ключа не требует замены общего.
Публичный `/health` отдаёт только живость и версию; телеметрия обработки —
`/stats-telemetry` за авторизацией. На сервере не хранятся экранные записи,
карточки, тексты целей, окна и API-ключи LLM. Детали выкладки (хост, пути,
юнит systemd) живут в репозитории хаба, не здесь. Публикация desktop-приложения —
отдельно, человеком.
