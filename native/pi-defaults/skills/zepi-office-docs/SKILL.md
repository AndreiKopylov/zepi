---
name: zepi-office-docs
description: Extract text from PDFs with the bundled pypdf, or look up shared Office rules for choosing a skill, running scripts, and preserving source documents.
---

# Office documents

## Choose a skill

| Task | Skill |
|---|---|
| Read or edit Word (`.docx`) | `zepi-docx` |
| Read or edit Excel (`.xlsx`) | `zepi-xlsx` |
| Build or edit PowerPoint (`.pptx`) | `zepi-deck-build` and `zepi-deck-design` |
| Visually verify a Word, Excel, or PowerPoint file | `zepi-office-qa` |
| Extract text from a PDF | This skill |

## Run scripts and protect the source

```powershell
if (-not $env:ZEPI_PYTHON -or -not (Test-Path -LiteralPath $env:ZEPI_PYTHON -PathType Leaf)) {
    throw "Zepi's bundled Office Python is unavailable."
}
& $env:ZEPI_PYTHON script.py
```

If the runtime is unavailable, tell the user. Do not switch to system Python or run `pip install`.
Native viewers and `zepi office render` do not require Python.

Do not resave files you only need to read. Save changes to a new path unless the user explicitly
asks to overwrite. If the workflow cannot preserve macros, protection, or other required features,
report the limitation rather than silently dropping them. Reopen the output and report what changed.

## Read PDF text

```python
from pypdf import PdfReader

for number, page in enumerate(PdfReader("report.pdf").pages, start=1):
    text = page.extract_text() or ""
    print(f"Page {number}:\n{text}")
```

`pypdf` extracts the text layer; it does not run OCR. Empty extracted text does not mean an empty
page. Report if scanned pages need OCR instead of guessing their contents.
