# Сборка Zepi на Windows

## Что нужно

- Windows 10/11 x64, git с `core.longpaths true`.
- **MSVC v143** (Visual Studio Build Tools 2022) + Windows SDK 10.0.26100, включая
  Spectre-библиотеки. Нагрузка C++ не обязательна: `win-build.cmd` выставляет
  `INCLUDE`/`LIB`/`PATH` вручную.
- **Rust 1.98.1** (`RUSTUP_TOOLCHAIN` в `win-build.cmd`).
- **Node >= 22.19**, **Python 3.10+** (нужен pip для загрузки колёс python-слоя),
  **cmake** и **nasm** в `PATH` (удобно положить в `D:\AgentSandbox\tools\`).

## Цепочка

```powershell
npm ci                                   # tsx/typescript для адаптера
npm ci --prefix packages/acp --ignore-scripts
python scripts\native.py prepare         # тарбол Zed по пину native/zed.json + native/overlay
win-build.cmd                            # cargo build -p zed --bin zepi --profile release-fast
```

- `native.py prepare` распаковывает Zed в `.local\upstream\zed`, копирует поверх
  `native/overlay/**` и удаляет пути из `native/removed-paths.json`. **После любой правки в
  оверлее prepare обязателен** — cargo собирает из `.local\upstream\zed`.
- `win-build.cmd` собирает из `.local\upstream\zed`, target — `..\zepi-build` (вне репозитория).
  Профиль `release-fast`: lto выключен, debug=`full`, отсюда ~131 ГБ target и exe ~374 МБ.
- WebRTC-ассет (win-x64) качается вручную по пину из `native/assets.json` в
  `.local\upstream\zed\.webrtc-sys\webrtc-0001d84-4\win-x64-release`.

## Запуск и деплой

```powershell
python scripts\win-run.py                    # запуск с cwd = upstream (грамматики tree-sitter)
powershell -File scripts\deploy-bundle.ps1   # положить сборку в бандл и перезапустить
```

Бандл — это папка вида `C:\Llm\zepi` с `zepi.exe`, `packages`, `node_modules`, `native` и
`deps\{node,python,browsers}`. Состояние он не несёт: оно в `%LOCALAPPDATA%\Zepi`.

## Payload (deps)

```powershell
# Python 3.13 + python-docx/openpyxl/python-pptx/pypdf/Pillow
powershell -File scripts\prepare-python.ps1            # -> .local\payload\python

# Node 24 (node.exe + npm.cmd) — распаковать zip с nodejs.org в .local\payload\node
# Chromium для агента
node scripts\setup\browser.mjs                        # -> .local\browsers, затем копия в payload
```

## Установщик

```powershell
powershell -File scripts\build-installer.ps1 -Version 0.0.1-beta1
# dist\zepi-0.0.1-beta1-setup.exe
```

Скрипт собирает staging из трёх источников (репозиторий, `zepi-build\release-fast\zepi.exe`,
`.local\payload`) и запускает Inno Setup (`installer/zepi.iss`). Машинных путей в скриптах нет:
всё вычисляется от расположения репозитория. Staging удаляется после сборки (`-KeepStaging` —
оставить).

## Тесты

```powershell
npm run test:acp        # ACP-адаптер
npm run test:runtime    # рантайм pi: расширения, настройки, браузер
scripts\win-check.cmd   # cargo check -p zed --bin zepi
```

## Частые грабли

- Правка в `native/overlay` без `prepare` → в exe уезжает старая копия.
- `LNK1318 PDB LIMIT` — кончилось место; `os error 5` при линке — запущен `zepi.exe`.
- Полная пересборка с vendor-крейтами на 6 параллельных rustc может упасть по памяти:
  `ZEPI_JOBS=3`.
- `deploy-bundle.ps1` не запустит новую сборку, если старый процесс не завершился
  (single-instance отдаст окно ему) — закрыть вручную.
