"""Regenerate KXToDo app icons from logo.png.

Drop-in workflow: replace logo.png at the project root, then run
    python scripts/make-icon.py
(or just repackage with scripts/package.ps1, which calls this automatically when
Python + Pillow are available). The in-app titlebar icon is bundled directly
from logo.png by Vite, so it always tracks the latest logo.png.
"""
import os

from PIL import Image

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
SRC = os.path.join(ROOT, "logo.png")
ICON_DIR = os.path.join(ROOT, "src-tauri", "icons")


def squared(image, size):
    """Fit the logo onto a transparent square canvas of the given size."""
    image = image.convert("RGBA")
    w, h = image.size
    scale = size / max(w, h)
    new_w, new_h = max(1, round(w * scale)), max(1, round(h * scale))
    resized = image.resize((new_w, new_h), Image.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.paste(resized, ((size - new_w) // 2, (size - new_h) // 2), resized)
    return canvas


def main():
    if not os.path.exists(SRC):
        raise SystemExit("logo.png not found at " + SRC)
    os.makedirs(ICON_DIR, exist_ok=True)
    logo = Image.open(SRC)

    base = squared(logo, 1024)
    png_targets = {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon.png": 512,
    }
    for name, size in png_targets.items():
        base.resize((size, size), Image.LANCZOS).save(os.path.join(ICON_DIR, name))

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    base.save(os.path.join(ICON_DIR, "icon.ico"), sizes=[(s, s) for s in ico_sizes])
    print("Icons regenerated from " + SRC + " into " + ICON_DIR)


if __name__ == "__main__":
    main()
