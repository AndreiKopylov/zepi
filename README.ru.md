# Zepi

[English](README.md) | [Русский](README.ru.md)

**Windows-воркспейс для кода и офисных документов с агентом внутри.**

Zepi соединяет редактор [Zed](https://github.com/zed-industries/zed) и агента
[pi](https://github.com/badlogic/pi-mono) через протокол ACP. В одном окне: код, терминал, git,
чат с моделью и просмотр/правка `.pptx`, `.docx`, `.xlsx` без установленного Microsoft Office.

> Статус: **бета (0.0.1)**. Собрано на Zed v1.28.1 и pi 1.5.2.

## Возможности

| Область         | Что умеет                                                                                                                                    |
|:--------------- |:-------------------------------------------------------------------------------------------------------------------------------------------- |
| **Код**         | Полноценный редактор Zed: LSP, git, терминал, панели, темы, мультикурсор, командная палитра                                                  |
| **Агент**       | Чат с моделями провайдера, инструменты (файлы, поиск, PowerShell, codemode), субагенты, сессии (`/fork`, `/export`, `/compact`), MCP, скиллы |
| **Презентации** | `.pptx`: лента слайдов с зумом и поиском, правка текста, отправка слайда или области агенту                                                  |
| **Документы**   | `.docx`: постраничный просмотр, поиск, правка, отправка фрагмента агенту                                                                     |
| **Таблицы**     | `.xlsx`: листы, формулы, правка ячеек с сохранением на диск                                                                                  |
| **Рендер**      | `zepi office render` превращает pptx/docx в PNG тем же движком, что рисует вьюер — без LibreOffice и Office                                  |

Агент работает с документами сам: в комплекте скиллы `zepi-office-docs`, `zepi-docx`, `zepi-xlsx`,
`zepi-deck-build`, `zepi-deck-design`, `zepi-office-qa` и встроенный Python 3.13 с python-docx,
openpyxl, python-pptx, pypdf и Pillow. Ставить ничего не нужно.

## Установка

1. Скачать последний установщик (`zepi-<version>-setup.exe`) со страницы [Releases](../../releases).
2. Запустить, подтвердить UAC. Программа ставится в `C:\Program Files\Zepi` и появляется в
   «Параметры → Приложения».
3. Выбрать компоненты:
   - **Zepi** — редактор и агент (обязательно);
   - **Python для документов Office** — нужен для `.docx`/`.xlsx`/`.pptx` (56 МБ).
4. Указать провайдера моделей: в настройках агента (`Settings → Agent`) или в
   `%LOCALAPPDATA%\Zepi\pi\models.json`. Готовых провайдеров в поставке нет — адрес и ключ каждый
   задаёт свои.

Настройки, сессии и скиллы живут в `%LOCALAPPDATA%\Zepi` и не затрагиваются обновлением.

### Требования

- Windows 10/11 x64.
- Провайдер моделей (любой OpenAI-совместимый endpoint или один из встроенных в pi).
- Для браузерных инструментов агента: системный Chrome или Edge, либо установка закреплённого Chromium по требованию со страницы `Browser use`.
- Системный Node.js, Python и git **не нужны** — всё в поставке.

## Как это устроено

```
zepi.exe                 редактор + хост агента (Rust: Zed плюс оверлей Zepi)
packages/, node_modules/ ACP-адаптер и pi
deps/node/               Node 24 для агента
deps/python/             CPython 3.13 с документными библиотеками
native/                  внешний вид, дефолты pi, скиллы
```

Состояние — в `%LOCALAPPDATA%\Zepi\{zed,pi}`, картинки вьюеров и рендеры — в
`%LOCALAPPDATA%\Zepi\{temp,render}` (удаляются через 7 дней).

## Сборка из исходников

Кратко (подробности — в [`docs/BUILD.md`](docs/BUILD.md)):

```powershell
npm ci
python scripts\native.py prepare   # тарбол Zed по пину native/zed.json + оверлей Zepi
win-build.cmd                      # cargo build -p zed --bin zepi --profile release-fast
python scripts\win-run.py          # запуск собранного exe
```

Нужны MSVC v143 + Windows SDK, Rust 1.98.1, Node >= 22, Python 3.10+, cmake и nasm.

## Ограничения беты

- Установщик не подписан — SmartScreen предупредит о «неизвестном издателе».
- Автообновления нет: новая версия ставится поверх, данные сохраняются.
- `.xlsx` не имеет растрового рендера: визуальная проверка таблиц не поддерживается, значения
  проверяются программно.
- Модели не поставляются: без ключа агент не отвечает.

## Благодарности

- [Zed Industries](https://github.com/zed-industries/zed) — редактор и платформа gpui.
- [tony3liu](https://github.com/tony3liu/eido) — eido: первая связка Zed + pi на Windows,
  от которой Zepi ведёт происхождение.
- [Mario Zechner](https://github.com/badlogic/pi-mono) — агент pi и протокол ACP.
- [storytold](https://github.com/storytold) — gridcraft, deckcraft, wordcraft (движки документов).
- Сообщества Node.js, Playwright и всех зависимостей из `Cargo.toml`/`package.json`.

## Лицензии

Код репозитория — **GPL-3.0-or-later** (`LICENSE`): оверлей производен от Zed (Apache-2.0 или
GPL-3.0-or-later, выбран GPL) и от eido. Пакеты pi — GPL-3.0-or-later. Вендорные
gridcraft/deckcraft/wordcraft остаются на MIT OR Apache-2.0, тексты и атрибуция — в их каталогах,
сводка — в `NOTICE`. Товарные знаки принадлежат владельцам.
