"""Render docs/src/*.md walkthroughs into book-style PDFs in docs/.

Supported markup (deliberately tiny):
  # Title            first line, document title
  ## Heading         section heading
  ### Subheading
  ```...```          code block (verbatim)
  ![caption](path)   image, path relative to the repo root
  **bold**, *italic*, `code` inline
  blank line         paragraph break

Usage: python3 scripts/walkthrough.py            (renders every docs/src/*.md)
       python3 scripts/walkthrough.py docs/src/m1_kuhn_cfr.md
"""

import re
import sys
from html import escape
from pathlib import Path

from reportlab.lib import colors
from reportlab.lib.enums import TA_JUSTIFY
from reportlab.lib.pagesizes import letter
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib.units import inch
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.platypus import (
    Image,
    KeepTogether,
    Paragraph,
    Preformatted,
    SimpleDocTemplate,
    Spacer,
    Table,
    TableStyle,
)

ROOT = Path(__file__).resolve().parent.parent
FONTS = Path("/usr/share/fonts/truetype/dejavu")

pdfmetrics.registerFont(TTFont("Serif", FONTS / "DejaVuSerif.ttf"))
pdfmetrics.registerFont(TTFont("Serif-Bold", FONTS / "DejaVuSerif-Bold.ttf"))
pdfmetrics.registerFont(TTFont("Serif-Italic", FONTS / "DejaVuSerif-Italic.ttf"))
pdfmetrics.registerFont(TTFont("Sans-Bold", FONTS / "DejaVuSans-Bold.ttf"))
pdfmetrics.registerFont(TTFont("Mono", FONTS / "DejaVuSansMono.ttf"))
pdfmetrics.registerFontFamily("Serif", normal="Serif", bold="Serif-Bold", italic="Serif-Italic")

INK = colors.HexColor("#1f2328")
ACCENT = colors.HexColor("#2563eb")
CODE_BG = colors.HexColor("#f4f5f7")

STYLES = {
    "title": ParagraphStyle("title", fontName="Sans-Bold", fontSize=22, leading=27, textColor=INK, spaceAfter=4),
    "subtitle": ParagraphStyle("subtitle", fontName="Serif-Italic", fontSize=11, leading=15, textColor=colors.HexColor("#57606a"), spaceAfter=18),
    "h2": ParagraphStyle("h2", fontName="Sans-Bold", fontSize=14, leading=18, textColor=ACCENT, spaceBefore=16, spaceAfter=6),
    "h3": ParagraphStyle("h3", fontName="Sans-Bold", fontSize=11.5, leading=15, textColor=INK, spaceBefore=10, spaceAfter=4),
    "body": ParagraphStyle("body", fontName="Serif", fontSize=10.5, leading=15.5, textColor=INK, alignment=TA_JUSTIFY, spaceAfter=8),
    "code": ParagraphStyle("code", fontName="Mono", fontSize=8.3, leading=10.8, textColor=INK),
    "caption": ParagraphStyle("caption", fontName="Serif-Italic", fontSize=9, leading=12, textColor=colors.HexColor("#57606a"), spaceAfter=10),
}


def inline(text: str) -> str:
    """Escape, then convert **bold**, *italic*, `code`, ^{sup} and _{sub}."""
    parts = re.split(r"(`[^`]+`)", text)
    out = []
    for p in parts:
        if p.startswith("`") and p.endswith("`") and len(p) > 1:
            out.append(f'<font name="Mono" size="9">{escape(p[1:-1])}</font>')
            continue
        p = escape(p)
        p = re.sub(r"\*\*(.+?)\*\*", r"<b>\1</b>", p)
        p = re.sub(r"(?<!\w)\*(?!\s)(.+?)(?<!\s)\*(?!\w)", r"<i>\1</i>", p)
        p = re.sub(r"\^\{(.+?)\}", r"<super>\1</super>", p)
        p = re.sub(r"_\{(.+?)\}", r"<sub>\1</sub>", p)
        out.append(p)
    return "".join(out)


def code_block(src: str):
    pre = Preformatted(src.rstrip("\n"), STYLES["code"])
    t = Table([[pre]], colWidths=[6.5 * inch])
    t.setStyle(
        TableStyle(
            [
                ("BACKGROUND", (0, 0), (-1, -1), CODE_BG),
                ("LEFTPADDING", (0, 0), (-1, -1), 8),
                ("RIGHTPADDING", (0, 0), (-1, -1), 8),
                ("TOPPADDING", (0, 0), (-1, -1), 6),
                ("BOTTOMPADDING", (0, 0), (-1, -1), 6),
                ("LINEBEFORE", (0, 0), (0, -1), 2, ACCENT),
            ]
        )
    )
    return [t, Spacer(1, 9)]


def table(rows):
    cell = ParagraphStyle("cell", parent=STYLES["body"], fontSize=9.5, leading=12, spaceAfter=0, alignment=0)
    head = ParagraphStyle("head", parent=cell, fontName="Serif-Bold")
    data = [[Paragraph(inline(c), head if r == 0 else cell) for c in row] for r, row in enumerate(rows)]
    t = Table(data, colWidths=[6.5 * inch / len(rows[0])] * len(rows[0]), hAlign="LEFT")
    t.setStyle(TableStyle([
        ("LINEBELOW", (0, 0), (-1, 0), 0.8, INK),
        ("LINEBELOW", (0, -1), (-1, -1), 0.5, colors.HexColor("#d0d7de")),
        ("ROWBACKGROUNDS", (0, 1), (-1, -1), [colors.white, CODE_BG]),
        ("TOPPADDING", (0, 0), (-1, -1), 4),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 4),
    ]))
    return [t, Spacer(1, 10)]


def image(path: str, caption: str):
    img_path = ROOT / path
    from reportlab.lib.utils import ImageReader

    w, h = ImageReader(str(img_path)).getSize()
    width = 6.5 * inch
    flow = [Image(str(img_path), width=width, height=width * h / w)]
    if caption:
        flow.append(Paragraph(inline(caption), STYLES["caption"]))
    return [KeepTogether(flow)]


def render(src: Path) -> Path:
    lines = src.read_text().splitlines()
    story = []
    title = lines[0].lstrip("# ").strip()
    story.append(Paragraph(inline(title), STYLES["title"]))
    story.append(Paragraph("holdem-cfr walkthrough", STYLES["subtitle"]))

    para: list[str] = []

    def flush():
        if para:
            story.append(Paragraph(inline(" ".join(para)), STYLES["body"]))
            para.clear()

    i = 1
    while i < len(lines):
        line = lines[i]
        if line.startswith("```"):
            flush()
            block = []
            i += 1
            while i < len(lines) and not lines[i].startswith("```"):
                block.append(lines[i])
                i += 1
            story.extend(code_block("\n".join(block)))
        elif line.lstrip().startswith("|"):
            flush()
            rows = []
            while i < len(lines) and lines[i].lstrip().startswith("|"):
                cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                if not all(set(c) <= set("-: ") for c in cells):
                    rows.append(cells)
                i += 1
            story.extend(table(rows))
            continue
        elif line.startswith("### "):
            flush()
            story.append(Paragraph(inline(line[4:]), STYLES["h3"]))
        elif line.startswith("## "):
            flush()
            story.append(Paragraph(inline(line[3:]), STYLES["h2"]))
        elif m := re.match(r"!\[(.*)\]\((.+)\)", line.strip()):
            flush()
            story.extend(image(m.group(2), m.group(1)))
        elif not line.strip():
            flush()
        else:
            para.append(line.strip())
        i += 1
    flush()

    out = ROOT / "docs" / (src.stem + ".pdf")

    def footer(canvas, doc):
        canvas.saveState()
        canvas.setFont("Serif-Italic", 8.5)
        canvas.setFillColor(colors.HexColor("#8c959f"))
        canvas.drawString(inch, 0.6 * inch, title)
        canvas.drawRightString(letter[0] - inch, 0.6 * inch, str(doc.page))
        canvas.restoreState()

    doc = SimpleDocTemplate(
        str(out), pagesize=letter, leftMargin=inch, rightMargin=inch,
        topMargin=0.9 * inch, bottomMargin=0.9 * inch, title=title, author="Tave La Rocque",
    )
    doc.build(story, onFirstPage=footer, onLaterPages=footer)
    return out


if __name__ == "__main__":
    targets = [Path(a) for a in sys.argv[1:]] or sorted((ROOT / "docs" / "src").glob("*.md"))
    for t in targets:
        print(f"wrote {render(t).relative_to(ROOT)}")
