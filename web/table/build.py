"""Inline the scripts, the exported blueprint and the preflop table into index.html."""
from pathlib import Path

here = Path(__file__).parent
page = (here / "page.html").read_text()
page = page.replace("/*CORE*/", (here / "core.js").read_text())
page = page.replace("/*ENGINE*/", (here / "engine.js").read_text())
page = page.replace("/*BPJS*/", (here / "blueprint.js").read_text())
page = page.replace("/*BLUEPRINT*/", (here / "blueprint.json").read_text().strip())
page = page.replace("/*HU*/", (here / "hu.js").read_text())
page = page.replace("/*PREFLOP*/", (here.parent.parent / "results" / "preflop_table.json").read_text().strip())
# A standalone document, so it renders in standards mode when opened directly or served by GitHub Pages.
head = '<!doctype html>\n<html lang="en">\n<meta charset="utf-8">\n<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">\n'
page = head + page
(here / "index.html").write_text(page)
print(f"wrote {here / 'index.html'} ({len(page) // 1024} KB)")
