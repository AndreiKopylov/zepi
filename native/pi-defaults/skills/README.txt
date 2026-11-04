Skills in this folder ship with Zepi. Their names carry the "zepi-" prefix so they never
collide with your own skills.

On start Zepi copies them to %LOCALAPPDATA%\Zepi\pi\skills\zepi (or $PI_CODING_AGENT_DIR/skills/zepi).
The whole "zepi" folder is replaced whenever the bundled content changes, so edits made
inside it are lost on the next install or update.

Bundled:
  zepi-office-docs  runtime overview: which libraries ship, how to read and save, what to verify
  zepi-docx         Word: styles, tables, headers, find-and-replace across runs, templates, traps
  zepi-xlsx         Excel: formulas vs cached values, styles, widths, charts, the data_only trap
  zepi-deck-design  design rules: grid, type scale, colour, slide compositions, charts, tables
  zepi-deck-build   building decks with python-pptx: template layouts, tables, native charts, Gantt
  zepi-office-qa    render with `zepi office render` and check the result before reporting it done

Put your own skills next to it, in %LOCALAPPDATA%\Zepi\pi\skills\<name>\SKILL.md.
Those are never touched by Zepi.
