# zepi это harness ide для windows: zed + pi + office docs

Windows-порт и развитие [zepi](https://github.com/tony3liu/eido): связка редактора [Zed](https://github.com/zed-industries/zed) и агента [pi ](https://github.com/badlogic/pi-mono)через ACP

Также добавлен рендеринг офисных документов на вендорных «крафтах» ([gridcraft / deckcraft / wordcraft](https://github.com/storytold)).
Добавлены встроенные навыки(skills) агента для работы с ними, в zed добавлены интерфейсы для изменения документов с помощью агента.

## Разделение ролей

- **Zed** — редактор, LSP, git, терминал, панели, темы, воркспейс, просмотр документов.
- **pi** — агент, чат с моделями провайдера, инструменты (файлы, bash, поиск), browser-агент
  (Chromium через Playwright), субагенты, сессии (`/fork`, `/export`, `/import`, `/compact`),
  MCP-расширения и навыки(skills) агента.

## Возможности

| Назначение  | Что умеет                                                                                               |
|:----------- |:------------------------------------------------------------------------------------------------------- |
| Код         | Полноценный Zed                                                                                         |
| Документы   | `.xlsx`: просмотр и частично редактирование; `.docx`: просмотр, поиск. Инструменты для отправки агенту. |
| Презентации | `.pptx`: лента слайдов с зумом, поиск, отправка слайда или текста агенту.                               |

## Дистрибутив

| Путь                         | Назначение                                                       |
|:---------------------------- |:---------------------------------------------------------------- |
| `zepi.exe`                   | редактор + агент-хост                                            |
| `packages/`, `node_modules/` | pi-слой (ACP-адаптер, рантайм, browser-агент)                    |
| `deps/`                      | зависимости необходимые для работы                               |
| `native/`                    | внешний вид, дефолты pi (`models.json`, `settings.json`), skills |

Состояние (настройки Zed, сессии и журналы агента, базы) живёт в`%LOCALAPPDATA%\Zepi\{zed,pi}`, вне папки дистрибутива.

Изначальное состояние провайдеров, моделей и настроек pi сидятся из `native/pi-defaults`

## Сборка из исходников

Требования: Windows 10/11 x64, MSVC v143 + Windows SDK 10.0.26100 + Spectre-libs v143
(отдельными компонентами Build Tools), Rust 1.98.1, Node >= 22.19, Python 3.10+,
cmake и nasm в PATH, git с `core.longpaths true`.

```bat
npm ci
python scripts\native.py prepare   :: тарбол Zed по пину native/zed.json + оверлей
win-build.cmd                      :: cargo build -p zed --bin zepi --profile release-fast
scripts\win-run.py                 :: запуск: сидит конфиги, поднимает release-fast exe
```

Подробности для разработчиков и агентные инструкции в `AGENTS.md`.

## Благодарности и ссылки

- [Zed Industries](https://github.com/zed-industries/zed) — редактор и gpui-платформа.
- [tony3liu](https://github.com/tony3liu/eido) — eido: исходная связка Zed + pi и eido-оверлей.
- [Mario Zechner (badlogic)](https://github.com/badlogic/pi-mono) — агент pi, ACP-протокол.
- [storytold](https://github.com/storytold) — gridcraft (xlsx), deckcraft (pptx), wordcraft (docx).
- Сообщества Node.js, Playwright, rust-image и всех зависимостей `Cargo.toml`/`package.json`.

## Лицензии

Код репозитория — GPL-3.0-or-later (файл `LICENSE`): оверлей производен от Zed
(двойная лицензия Apache-2.0 / GPL-3.0-or-later, выбран GPL) и zepi (GPL-3.0-or-later),
пакеты pi — GPL-3.0-or-later. Вендорные gridcraft/deckcraft/wordcraft сохраняют свои
лицензии MIT OR Apache-2.0 (тексты и атрибуция в их каталогах). Товарные знаки принадлежат
владельцам.
