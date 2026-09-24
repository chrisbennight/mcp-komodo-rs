# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools==4.65.0", "resvg-py==0.5.0"]
# ///
"""Export the checked-in Stack artwork; no runtime or remote font dependency."""

from __future__ import annotations

import argparse
from html import escape
from pathlib import Path
import xml.etree.ElementTree as ET

import resvg_py
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = Path(__file__).resolve().parent
ASSETS = ROOT / "assets"
NS = "{http://www.w3.org/2000/svg}"
PAPER = "#F7F5F0"
INK = "#23201B"
FOREST = "#1E5F46"
COPPER = "#B66A45"
NAME = "mcp-komodo-rs"
DESCRIPTION = "Komodo operations for MCP clients"


def svg(width: int, height: int, content: str, title: str) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" '
        f'width="{width}" height="{height}" role="img">'
        f"<title>{escape(title)}</title>{content}</svg>\n"
    )


def background(width: int, height: int, color: str) -> str:
    return f'<path fill="{color}" d="M0 0H{width}V{height}H0Z"/>'


def mark(x: float, y: float, size: float, ink: str, accent: str) -> str:
    root = ET.parse(ROOT / "symbol.svg").getroot()
    content = "".join(ET.tostring(child, encoding="unicode") for child in root
                      if child.tag != NS + "title")
    content = content.replace("var(--connection, currentColor)", accent)
    content = content.replace("currentColor", ink)
    return f'<g transform="translate({x} {y}) scale({size / 128})">{content}</g>'


def lettering(text: str, x: float, y: float, size: float, color: str,
              font: TTFont) -> str:
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    cursor = 0
    paths = []
    for character in text:
        glyph = glyphs[cmap[ord(character)]]
        pen = SVGPathPen(glyphs)
        glyph.draw(pen)
        paths.append(f'<path transform="translate({cursor} 0)" d="{pen.getCommands()}"/>')
        cursor += glyph.width
    scale = size / font["head"].unitsPerEm
    return (f'<g fill="{color}" transform="translate({x} {y}) scale({scale} {-scale})">'
            + "".join(paths) + "</g>")


def exports() -> dict[str, bytes]:
    source = TTFont(ROOT / "fonts/Manrope.ttf")
    medium = instantiateVariableFont(source, {"wght": 500}, inplace=False)
    bold = instantiateVariableFont(source, {"wght": 700}, inplace=False)
    result: dict[str, bytes] = {}

    def add(name: str, width: int, height: int, content: str, title: str) -> None:
        result[name + ".svg"] = svg(width, height, content, title).encode()

    for theme, paper, ink, brand in [("light", PAPER, INK, FOREST),
                                     ("dark", INK, PAPER, PAPER)]:
        add(f"symbol-{theme}", 128, 128, mark(0, 0, 128, brand, COPPER), NAME)
        add(f"wordmark-{theme}", 720, 128,
            background(720, 128, paper) + mark(8, 8, 104, brand, COPPER)
            + lettering(NAME, 136, 84, 64, ink, bold), NAME)
        add(f"header-{theme}", 960, 240,
            background(960, 240, paper) + mark(32, 40, 144, brand, COPPER)
            + lettering(NAME, 216, 120, 78, ink, bold)
            + lettering(DESCRIPTION, 220, 168, 30, ink, medium), NAME + " — " + DESCRIPTION)
        add(f"avatar-{theme}", 256, 256,
            background(256, 256, paper) + mark(48, 42, 160, brand, COPPER), NAME)
    add("symbol-mono", 128, 128, mark(0, 0, 128, INK, INK), NAME)
    add("social-preview", 1280, 640,
        background(1280, 640, PAPER) + mark(72, 182, 220, FOREST, COPPER)
        + lettering(NAME, 344, 296, 98, INK, bold)
        + lettering(DESCRIPTION, 348, 365, 38, INK, medium), NAME + " — " + DESCRIPTION)

    symbols = ET.parse(ROOT / "icons.svg").getroot()
    for symbol in symbols:
        content = "".join(ET.tostring(child, encoding="unicode") for child in symbol)
        content = content.replace("currentColor", FOREST)
        add(symbol.attrib["id"], 24, 24, content, symbol.attrib["id"].capitalize())
    for theme, paper, ink in [("light", PAPER, INK), ("dark", INK, PAPER)]:
        pieces = [background(960, 150, paper)]
        for index, symbol in enumerate(symbols):
            content = "".join(ET.tostring(child, encoding="unicode") for child in symbol)
            content = content.replace("currentColor", ink)
            x = 120 + index * 240
            pieces.append(f'<g transform="translate({x - 24} 24) scale(2)">{content}</g>')
            label = symbol.attrib["id"].capitalize()
            pieces.append(lettering(label, x - len(label) * 6, 116, 23, ink, medium))
        add(f"capabilities-{theme}", 960, 150, "".join(pieces),
            "Stacks, deployments, builds, and operations")
    for name, size in [("social-preview", 1280), ("avatar-light", 256), ("avatar-dark", 256)]:
        result[name + ".png"] = resvg_py.svg_to_bytes(
            svg_string=result[name + ".svg"].decode(), width=size, skip_system_fonts=True)
    for theme in ["light", "dark"]:
        for size in [16, 32]:
            result[f"icon-{theme}-{size}.png"] = resvg_py.svg_to_bytes(
                svg_string=result[f"symbol-{theme}.svg"].decode(), width=size, height=size,
                skip_system_fonts=True)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Compare exports without writing")
    args = parser.parse_args()
    expected = exports()
    if args.check:
        stale = [name for name, data in expected.items()
                 if not (ASSETS / name).is_file() or (ASSETS / name).read_bytes() != data]
        if stale:
            print("Regenerate branding exports: " + ", ".join(stale))
            return 1
        print("Branding exports match their sources")
    else:
        ASSETS.mkdir(exist_ok=True)
        for name, data in expected.items():
            (ASSETS / name).write_bytes(data)
        print("Exported Stack branding assets")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
