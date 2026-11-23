"""Screenshot conversion (ffmpeg / PIL) and opaque-box frame analysis helpers."""
from dataclasses import dataclass
from pathlib import Path
import shutil
import subprocess
from PIL import Image, ImageChops, ImageStat


@dataclass
class FrameStats:
    width: int
    height: int
    mean_luma: float
    std_luma: float
    non_black_ratio: float
    distinct_channel_spread: float


def _to_rgb_over_black(im: Image.Image) -> Image.Image:
    if im.mode in ("RGBA", "LA") or (im.mode == "P" and "transparency" in im.info):
        rgba = im.convert("RGBA")
        bg = Image.new("RGBA", rgba.size, (0, 0, 0, 255))
        return Image.alpha_composite(bg, rgba).convert("RGB")
    return im.convert("RGB")


def convert_ppm_to_png(ppm_path: Path, png_path: Path) -> Path:
    ppm = Path(ppm_path)
    png = Path(png_path)
    png.parent.mkdir(parents=True, exist_ok=True)
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg:
        res = subprocess.run(
            [ffmpeg, "-y", "-loglevel", "error", "-i", str(ppm), str(png)],
            capture_output=True,
            text=True,
            timeout=20,
        )
        if res.returncode == 0 and png.is_file() and png.stat().st_size > 0:
            return png
    with Image.open(ppm) as im:
        _to_rgb_over_black(im).save(png, format="PNG")
    return png


def analyze_frame(image_path: Path, black_threshold: int = 10) -> FrameStats:
    with Image.open(image_path) as im:
        rgb = _to_rgb_over_black(im)
        width, height = rgb.size
        gray = rgb.convert("L")
        stat_l = ImageStat.Stat(gray)
        stat_rgb = ImageStat.Stat(rgb)
        mean_luma = float(stat_l.mean[0])
        std_luma = float(stat_l.stddev[0])

        hist = gray.histogram()
        total_px = width * height
        dark_px = sum(hist[: black_threshold + 1])
        non_black_ratio = (total_px - dark_px) / total_px if total_px else 0.0

        means = stat_rgb.mean
        distinct_channel_spread = max(means) - min(means)
        return FrameStats(
            width=width,
            height=height,
            mean_luma=mean_luma,
            std_luma=std_luma,
            non_black_ratio=non_black_ratio,
            distinct_channel_spread=float(distinct_channel_spread),
        )


def is_non_black_content(
    image_path: Path,
    min_non_black_ratio: float = 0.01,
    min_std_luma: float = 1.5,
) -> bool:
    stats = analyze_frame(image_path)
    return stats.non_black_ratio >= min_non_black_ratio and stats.std_luma >= min_std_luma


def frame_diff_ratio(img_a_path: Path, img_b_path: Path, pixel_delta: int = 12) -> float:
    with Image.open(img_a_path) as a, Image.open(img_b_path) as b:
        ra = _to_rgb_over_black(a)
        rb = _to_rgb_over_black(b)
        if ra.size != rb.size:
            rb = rb.resize(ra.size)
        diff = ImageChops.difference(ra, rb).convert("L")
        hist = diff.histogram()
        total = ra.size[0] * ra.size[1]
        unchanged = sum(hist[: pixel_delta + 1])
        return (total - unchanged) / total if total else 0.0


@dataclass
class RegionStats:
    name: str
    box: tuple[int, int, int, int]
    mean_luma: float
    std_luma: float
    non_canvas_ratio: float
    mean_rgb: tuple[float, float, float]


DEFAULT_ANACONDA_UI_REGIONS: dict[str, tuple[int, int, int, int]] = {
    "left_sidebar": (140, 170, 330, 660),
    "outer_sidebar": (140, 170, 255, 660),
    "top_banner": (220, 100, 1050, 155),
    "center_content": (380, 220, 900, 620),
}


def analyze_ui_regions(
    image_path: Path,
    regions: dict[str, tuple[int, int, int, int]] | None = None,
    canvas_rgb: tuple[int, int, int] = (41, 41, 41),
    tolerance: int = 8,
) -> dict[str, RegionStats]:
    """Compute luma variance and non-canvas pixel ratios across UI regions using PIL only."""
    target_regions = regions or DEFAULT_ANACONDA_UI_REGIONS
    results: dict[str, RegionStats] = {}
    with Image.open(image_path) as im:
        rgb = _to_rgb_over_black(im)
        width, height = rgb.size
        for name, (x0, y0, x1, y1) in target_regions.items():
            cx0 = max(0, min(width, x0))
            cy0 = max(0, min(height, y0))
            cx1 = max(0, min(width, x1))
            cy1 = max(0, min(height, y1))
            if cx1 <= cx0 or cy1 <= cy0:
                raise ValueError(f"Empty UI region {name!r}: {(x0, y0, x1, y1)} for image {width}x{height}")
            crop = rgb.crop((cx0, cy0, cx1, cy1))
            gray = crop.convert("L")
            stat_l = ImageStat.Stat(gray)
            stat_rgb = ImageStat.Stat(crop)

            bg = Image.new("RGB", crop.size, canvas_rgb)
            diff = ImageChops.difference(crop, bg)
            dr, dg, db = diff.split()
            max_diff = ImageChops.lighter(ImageChops.lighter(dr, dg), db)
            hist = max_diff.histogram()
            total = crop.size[0] * crop.size[1]
            canvas_px = sum(hist[: tolerance + 1])
            non_canvas_ratio = (total - canvas_px) / total if total else 0.0

            results[name] = RegionStats(
                name=name,
                box=(cx0, cy0, cx1, cy1),
                mean_luma=float(stat_l.mean[0]),
                std_luma=float(stat_l.stddev[0]),
                non_canvas_ratio=float(non_canvas_ratio),
                mean_rgb=(float(stat_rgb.mean[0]), float(stat_rgb.mean[1]), float(stat_rgb.mean[2])),
            )
    return results


def _regions_within_image(
    image_path: Path,
    regions: dict[str, tuple[int, int, int, int]],
) -> bool:
    with Image.open(image_path) as im:
        width, height = im.size
    return bool(regions) and all(
        0 <= x0 < x1 <= width and 0 <= y0 < y1 <= height
        for x0, y0, x1, y1 in regions.values()
    )


def _region_chroma_spread(reg: RegionStats) -> float:
    return max(reg.mean_rgb) - min(reg.mean_rgb)


def is_anaconda_webui_rendered(
    image_path: Path,
    regions: dict[str, tuple[int, int, int, int]] | None = None,
) -> tuple[bool, dict[str, RegionStats]]:
    """Verify rendered Anaconda WebUI inside the centered 82% (~1050x656) floating window.

    Rejects blank #292929 Firefox canvases, #292929 + solar-welcome overlap frames,
    SolarUI desktop/wallpaper captures, and undersized (e.g. 720x400) frames.
    """
    target_regions = regions or DEFAULT_ANACONDA_UI_REGIONS
    if not _regions_within_image(image_path, target_regions):
        return False, {}

    stats = analyze_ui_regions(image_path, regions=target_regions)
    outer = stats.get("outer_sidebar")
    sidebar = stats["left_sidebar"]
    banner = stats["top_banner"]
    center = stats["center_content"]

    outer_ok = (
        True
        if outer is None
        else (
            outer.std_luma >= 5.0
            and 0.03 <= outer.non_canvas_ratio <= 0.45
            and outer.mean_luma <= 80.0
        )
    )
    sidebar_ok = (
        sidebar.std_luma >= 5.0
        and 0.03 <= sidebar.non_canvas_ratio <= 0.45
        and sidebar.mean_luma <= 80.0
    )
    banner_blue_delta = banner.mean_rgb[2] - banner.mean_rgb[0]
    banner_ok = (
        banner.std_luma >= 8.0
        and banner.non_canvas_ratio >= 0.35
        and banner.mean_luma >= 80.0
        and banner_blue_delta >= 50.0
    )
    center_ok = (
        center.std_luma >= 4.0
        and 0.015 <= center.non_canvas_ratio <= 0.75
        and center.mean_luma <= 95.0
    )
    return (outer_ok and sidebar_ok and banner_ok and center_ok), stats


DEFAULT_FIREHUB_UI_REGIONS: dict[str, tuple[int, int, int, int]] = {
    "interior": (180, 140, 1100, 720),
    "header_search": (160, 105, 1120, 225),
    "outer_left": (160, 110, 255, 740),
    "outer_right": (1025, 110, 1120, 740),
    "editors_choice": (160, 380, 1120, 500),
    "categories": (160, 560, 1120, 745),
}


def is_firehub_rendered(
    image_path: Path,
    regions: dict[str, tuple[int, int, int, int]] | None = None,
) -> tuple[bool, dict[str, RegionStats]]:
    """Verify rendered FireHub storefront inside the centered 75%x80% floating window.

    Rejects unrendered #0d0f12/#0e1014 Electron canvases (even with cursor or
    solar-welcome overlap), grayscale/zero-chroma monochrome decoys, SolarUI
    desktop/wallpaper frames, and undersized (e.g. 720x400) frames.
    """
    target_regions = regions or DEFAULT_FIREHUB_UI_REGIONS
    if not _regions_within_image(image_path, target_regions):
        return False, {}

    stats = analyze_ui_regions(
        image_path,
        regions=target_regions,
        canvas_rgb=(14, 16, 20),
        tolerance=8,
    )
    interior = stats["interior"]
    header = stats["header_search"]
    outer_l = stats["outer_left"]
    outer_r = stats["outer_right"]
    cards = stats["editors_choice"]
    cats = stats["categories"]

    interior_ok = (
        interior.std_luma >= 12.0
        and 0.15 <= interior.non_canvas_ratio <= 0.65
        and interior.mean_luma <= 45.0
    )
    header_ok = (
        header.std_luma >= 5.0
        and 0.015 <= header.non_canvas_ratio <= 0.35
        and header.mean_luma <= 35.0
    )
    outer_l_ok = (
        outer_l.std_luma >= 12.0
        and 0.15 <= outer_l.non_canvas_ratio <= 0.65
        and outer_l.mean_luma <= 45.0
    )
    outer_r_ok = (
        outer_r.std_luma >= 12.0
        and 0.15 <= outer_r.non_canvas_ratio <= 0.65
        and outer_r.mean_luma <= 45.0
    )
    cards_ok = (
        cards.std_luma >= 18.0
        and 0.25 <= cards.non_canvas_ratio <= 0.85
        and cards.mean_luma <= 80.0
    )
    cats_ok = (
        cats.std_luma >= 3.0
        and cats.non_canvas_ratio >= 0.40
        and cats.mean_luma <= 35.0
    )
    chroma_ok = (
        _region_chroma_spread(interior) >= 2.0
        and _region_chroma_spread(header) >= 2.0
    )
    return (
        interior_ok
        and header_ok
        and outer_l_ok
        and outer_r_ok
        and cards_ok
        and cats_ok
        and chroma_ok
    ), stats

