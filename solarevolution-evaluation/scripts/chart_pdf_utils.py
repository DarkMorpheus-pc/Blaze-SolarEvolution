#!/usr/bin/env python3
"""SVG chart builders and styled HTML -> publication-ready PDF exporter (via headless Chrome).

Uses only Python stdlib + google-chrome-stable --headless --print-to-pdf so every chart
is crisp vector SVG and every PDF report is clean, reproducible, and print-ready.
"""
from __future__ import annotations

import html
import re
import subprocess
import tempfile
from pathlib import Path
from typing import Any


def _esc(text: Any) -> str:
    return html.escape(str(text), quote=True)


def make_horizontal_bar_svg(
    title: str,
    subtitle: str,
    items: list[tuple[str, float, str, str]],
    unit: str = "",
    width: int = 860,
    bar_height: int = 28,
    gap: int = 12,
    max_val: float | None = None,
) -> str:
    """Create a clean vector horizontal bar chart SVG.

    items: list of (label, value, display_text, hex_color)
    """
    max_lbl = max((len(lbl) for lbl, _, _, _ in items), default=20)
    max_disp = max((len(disp) for _, _, disp, _ in items), default=10)
    label_w = max(275, min(390, int(max_lbl * 6.8) + 28))
    right_pad = max(195, min(270, int(max_disp * 7.0) + 24))
    chart_w = 390
    width = max(width, label_w + chart_w + right_pad)
    top_pad = 72
    bottom_pad = 36
    height = top_pad + len(items) * (bar_height + gap) + bottom_pad

    mv = max_val if max_val is not None else (max((v for _, v, _, _ in items), default=1.0) or 1.0)
    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" rx="10" fill="#0f172a"/>',
        f'<text x="24" y="30" fill="#f8fafc" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="16" font-weight="700">{_esc(title)}</text>',
        f'<text x="24" y="50" fill="#94a3b8" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="12">{_esc(subtitle)}</text>',
    ]
    # Grid lines at 0%, 25%, 50%, 75%, 100%
    for frac in (0.0, 0.25, 0.5, 0.75, 1.0):
        gx = label_w + chart_w * frac
        lines.append(
            f'<line x1="{gx:.1f}" y1="{top_pad - 8}" x2="{gx:.1f}" y2="{height - bottom_pad + 4}" stroke="#1e293b" stroke-width="1"/>'
        )
        gval = mv * frac
        lbl = f"{gval:.0f}" if mv >= 20 else f"{gval:.1f}"
        if frac == 1.0 and unit:
            lbl = f"{lbl} {unit}"
        lines.append(
            f'<text x="{gx:.1f}" y="{height - 12}" text-anchor="middle" fill="#64748b" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="10">{_esc(lbl)}</text>'
        )

    for idx, (label, val, disp, color) in enumerate(items):
        y = top_pad + idx * (bar_height + gap)
        bw = max(2.0, (max(0.0, val) / mv) * chart_w)
        lines.append(
            f'<text x="{label_w - 12}" y="{y + bar_height * 0.65:.1f}" text-anchor="end" fill="#e2e8f0" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="12">{_esc(label)}</text>'
        )
        lines.append(
            f'<rect x="{label_w}" y="{y}" width="{chart_w}" height="{bar_height}" rx="5" fill="#1e293b"/>'
        )
        lines.append(
            f'<rect x="{label_w}" y="{y}" width="{bw:.1f}" height="{bar_height}" rx="5" fill="{_esc(color)}"/>'
        )
        lines.append(
            f'<text x="{label_w + bw + 8:.1f}" y="{y + bar_height * 0.65:.1f}" fill="#f8fafc" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="12" font-weight="600">{_esc(disp)}</text>'
        )

    lines.append("</svg>")
    return "\n".join(lines)


def make_grouped_comparison_svg(
    title: str,
    subtitle: str,
    groups: list[dict[str, Any]],
    series_a_name: str,
    series_b_name: str,
    color_a: str = "#f97316",
    color_b: str = "#38bdf8",
    width: int = 860,
) -> str:
    """Create a normalized side-by-side horizontal comparison bar chart where each metric scales to its pair max."""
    max_lbl = max((len(str(g["label"])) for g in groups), default=20)
    max_disp = max((max(len(str(g["disp_a"])), len(str(g["disp_b"]))) for g in groups), default=12)
    label_w = max(260, min(390, int(max_lbl * 7.1) + 28))
    right_pad = max(150, min(240, int(max_disp * 7.0) + 24))
    chart_w = 450
    width = max(width, label_w + chart_w + right_pad)
    top_pad = 86
    row_h = 54
    bottom_pad = 28
    height = top_pad + len(groups) * row_h + bottom_pad

    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" rx="10" fill="#0f172a"/>',
        f'<text x="24" y="30" fill="#f8fafc" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="16" font-weight="700">{_esc(title)}</text>',
        f'<text x="24" y="49" fill="#94a3b8" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="12">{_esc(subtitle)}</text>',
        f'<rect x="24" y="60" width="12" height="12" rx="3" fill="{color_a}"/>',
        f'<text x="42" y="70" fill="#e2e8f0" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="11">{_esc(series_a_name)}</text>',
        f'<rect x="250" y="60" width="12" height="12" rx="3" fill="{color_b}"/>',
        f'<text x="268" y="70" fill="#e2e8f0" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="11">{_esc(series_b_name)}</text>',
    ]

    for idx, g in enumerate(groups):
        y = top_pad + idx * row_h
        va = float(g["a"])
        vb = float(g["b"])
        mv = max(va, vb, 1e-6) * 1.05
        wa = max(2.0, (va / mv) * chart_w)
        wb = max(2.0, (vb / mv) * chart_w)
        unit = g.get("unit", "")
        direction = g.get("direction", "")
        dir_note = " (higher=better)" if direction == "HIB" else (" (lower=better)" if direction == "LIB" else "")
        lines.append(
            f'<text x="{label_w - 12}" y="{y + 18}" text-anchor="end" fill="#f1f5f9" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="12" font-weight="600">{_esc(g["label"])}</text>'
        )
        lines.append(
            f'<text x="{label_w - 12}" y="{y + 34}" text-anchor="end" fill="#94a3b8" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="10">{_esc(unit + dir_note)}</text>'
        )
        lines.append(f'<rect x="{label_w}" y="{y + 4}" width="{chart_w}" height="16" rx="4" fill="#1e293b"/>')
        lines.append(f'<rect x="{label_w}" y="{y + 4}" width="{wa:.1f}" height="16" rx="4" fill="{color_a}"/>')
        lines.append(
            f'<text x="{label_w + wa + 8:.1f}" y="{y + 16}" fill="#fed7aa" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="11" font-weight="600">{_esc(g["disp_a"])}</text>'
        )
        lines.append(f'<rect x="{label_w}" y="{y + 24}" width="{chart_w}" height="16" rx="4" fill="#1e293b"/>')
        lines.append(f'<rect x="{label_w}" y="{y + 24}" width="{wb:.1f}" height="16" rx="4" fill="{color_b}"/>')
        lines.append(
            f'<text x="{label_w + wb + 8:.1f}" y="{y + 36}" fill="#bae6fd" font-family="Inter,DejaVu Sans,Arial,sans-serif" font-size="11" font-weight="600">{_esc(g["disp_b"])}</text>'
        )

    lines.append("</svg>")
    return "\n".join(lines)


def markdown_to_html(md_text: str, title: str, base_dir: Path) -> str:
    """Convert clean GitHub-style technical Markdown (headings, tables, lists, code, SVGs, images) into print-styled HTML."""
    lines = md_text.splitlines()
    out: list[str] = []
    in_code = False
    code_buf: list[str] = []
    in_ul = False
    in_ol = False
    in_table = False
    table_rows: list[list[str]] = []

    def close_lists() -> None:
        nonlocal in_ul, in_ol
        if in_ul:
            out.append("</ul>")
            in_ul = False
        if in_ol:
            out.append("</ol>")
            in_ol = False

    def flush_table() -> None:
        nonlocal in_table, table_rows
        if not in_table:
            return
        if len(table_rows) >= 2:
            out.append("<table><thead><tr>")
            for cell in table_rows[0]:
                out.append(f"<th>{inline_fmt(cell)}</th>")
            out.append("</tr></thead><tbody>")
            for row in table_rows[2:]:
                out.append("<tr>")
                for cell in row:
                    out.append(f"<td>{inline_fmt(cell)}</td>")
                out.append("</tr>")
            out.append("</tbody></table>")
        table_rows = []
        in_table = False

    def inline_fmt(s: str) -> str:
        # Handle inline images ![alt](path)
        def img_repl(m: re.Match[str]) -> str:
            alt, rel = m.group(1), m.group(2)
            p = (base_dir / rel).resolve()
            if p.suffix.lower() == ".svg" and p.is_file():
                return f'<div class="svg-wrap">{p.read_text(encoding="utf-8", errors="replace")}</div>'
            return f'<figure><img src="file://{_esc(p)}" alt="{_esc(alt)}"/><figcaption>{_esc(alt)}</figcaption></figure>'

        if s.strip().startswith("![") and "](" in s:
            return re.sub(r"!\[([^\]]*)\]\(([^)]+)\)", img_repl, s)

        esc = _esc(s)
        esc = re.sub(r"`([^`]+)`", r"<code>\1</code>", esc)
        esc = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", esc)
        esc = re.sub(r"\*([^*]+)\*", r"<em>\1</em>", esc)
        esc = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r'<a href="\2">\1</a>', esc)
        return esc

    for raw_ln in lines:
        if raw_ln.strip().startswith("```"):
            flush_table()
            close_lists()
            if not in_code:
                in_code = True
                code_buf = []
            else:
                out.append("<pre><code>" + _esc("\n".join(code_buf)) + "</code></pre>")
                in_code = False
                code_buf = []
            continue
        if in_code:
            code_buf.append(raw_ln)
            continue

        stripped = raw_ln.strip()
        if not stripped:
            flush_table()
            close_lists()
            continue

        if stripped.startswith("|") and stripped.endswith("|"):
            close_lists()
            in_table = True
            cells = [c.strip() for c in stripped.strip("|").split("|")]
            table_rows.append(cells)
            continue
        else:
            flush_table()

        if stripped == "---":
            close_lists()
            out.append("<hr/>")
            continue

        m_h = re.match(r"^(#{1,4})\s+(.*)$", stripped)
        if m_h:
            close_lists()
            lvl = len(m_h.group(1))
            out.append(f"<h{lvl}>{inline_fmt(m_h.group(2))}</h{lvl}>")
            continue

        if stripped.startswith("> "):
            close_lists()
            out.append(f"<blockquote>{inline_fmt(stripped[2:])}</blockquote>")
            continue

        m_ul = re.match(r"^[-*]\s+(.*)$", stripped)
        if m_ul:
            if in_ol:
                out.append("</ol>")
                in_ol = False
            if not in_ul:
                out.append("<ul>")
                in_ul = True
            out.append(f"<li>{inline_fmt(m_ul.group(1))}</li>")
            continue

        m_ol = re.match(r"^\d+\.\s+(.*)$", stripped)
        if m_ol:
            if in_ul:
                out.append("</ul>")
                in_ul = False
            if not in_ol:
                out.append("<ol>")
                in_ol = True
            out.append(f"<li>{inline_fmt(m_ol.group(1))}</li>")
            continue

        close_lists()
        if stripped.startswith("![") and "](" in stripped:
            out.append(inline_fmt(stripped))
        else:
            out.append(f"<p>{inline_fmt(stripped)}</p>")

    flush_table()
    close_lists()
    if in_code:
        out.append("<pre><code>" + _esc("\n".join(code_buf)) + "</code></pre>")

    css = """
    @page { size: A4; margin: 16mm 14mm 18mm 14mm; }
    body { font-family: "Inter", "DejaVu Sans", "Liberation Sans", Arial, sans-serif; color: #0f172a; line-height: 1.5; font-size: 10pt; margin: 0; }
    h1 { font-size: 20pt; color: #0f172a; border-bottom: 3px solid #f97316; padding-bottom: 6px; margin-top: 0; }
    h2 { font-size: 13.5pt; color: #1e293b; border-bottom: 1px solid #cbd5e1; padding-bottom: 3px; margin-top: 20px; page-break-after: avoid; }
    h3 { font-size: 11pt; color: #334155; margin-top: 14px; page-break-after: avoid; }
    p { margin: 7px 0; }
    blockquote { background: #f8fafc; border-left: 4px solid #f97316; padding: 8px 12px; margin: 10px 0; color: #334155; font-size: 9.5pt; }
    table { width: 100%; border-collapse: collapse; margin: 10px 0 14px 0; font-size: 8.6pt; page-break-inside: auto; }
    tr { page-break-inside: avoid; page-break-after: auto; }
    th { background: #0f172a; color: #f8fafc; text-align: left; padding: 6px 8px; font-weight: 600; }
    td { border-bottom: 1px solid #e2e8f0; padding: 5px 8px; vertical-align: top; }
    tbody tr:nth-child(even) { background: #f8fafc; }
    code { font-family: "JetBrains Mono", "DejaVu Sans Mono", monospace; background: #f1f5f9; padding: 1px 4px; border-radius: 3px; font-size: 8.5pt; color: #0f172a; }
    pre { background: #0f172a; color: #e2e8f0; padding: 10px 12px; border-radius: 6px; overflow-x: auto; font-size: 8pt; line-height: 1.4; page-break-inside: avoid; }
    pre code { background: transparent; color: inherit; padding: 0; }
    ul, ol { margin: 6px 0 10px 20px; padding: 0; }
    li { margin: 3px 0; }
    hr { border: 0; border-top: 1px solid #cbd5e1; margin: 16px 0; }
    .svg-wrap { margin: 12px 0; text-align: center; page-break-inside: avoid; }
    .svg-wrap svg { max-width: 100%; height: auto; }
    figure { margin: 12px 0; text-align: center; page-break-inside: avoid; }
    figure img { max-width: 96%; max-height: 360px; border: 1px solid #cbd5e1; border-radius: 6px; }
    figcaption { font-size: 8.5pt; color: #475569; margin-top: 4px; }
    """
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<title>{_esc(title)}</title>
<style>{css}</style>
</head>
<body>
{"".join(out)}
</body>
</html>
"""


def render_markdown_to_pdf(md_path: Path, pdf_path: Path, title: str) -> None:
    html_str = markdown_to_html(md_path.read_text(encoding="utf-8"), title=title, base_dir=md_path.parent)
    with tempfile.NamedTemporaryFile(mode="w", suffix=".html", encoding="utf-8", delete=False) as tf:
        tf.write(html_str)
        tmp_html = Path(tf.name)
    try:
        subprocess.run(
            [
                "google-chrome-stable",
                "--headless",
                "--no-sandbox",
                "--disable-gpu",
                "--no-pdf-header-footer",
                f"--print-to-pdf={pdf_path}",
                f"file://{tmp_html}",
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=90,
        )
    finally:
        tmp_html.unlink(missing_ok=True)
