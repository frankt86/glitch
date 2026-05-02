"""Generate Glitch app icons: a clean gold lightning bolt on a dark background."""
from PIL import Image, ImageDraw, ImageFilter

SIZE = 512
BG = (13, 17, 23)          # #0d1117 dark navy
BOLT = (240, 180, 41)       # #f0b429 gold
BOLT_BRIGHT = (255, 215, 90)  # lighter gold for highlight
GLOW = (255, 215, 90)       # glow color

# Lightning bolt polygon — 6 vertices, Z-shape
# Upper arm goes top-right → middle-left
# Horizontal step in middle
# Lower arm continues down to bottom-left
BOLT_POLY = [
    (335, 60),   # top-right
    (168, 285),  # middle-left outer
    (255, 285),  # middle notch step right
    (178, 455),  # bottom-left tip
    (352, 243),  # middle-right outer
    (265, 243),  # middle notch step left
]


def make_icon(size: int) -> Image.Image:
    scale = size / SIZE
    poly = [(int(x * scale), int(y * scale)) for x, y in BOLT_POLY]
    radius = int(80 * scale)

    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Rounded square background
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=(*BG, 255))

    # Glow behind bolt — draw a slightly expanded, blurred version
    glow_layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow_layer)
    gd.polygon(poly, fill=(*GLOW, 180))
    blur_r = max(1, int(10 * scale))
    glow_layer = glow_layer.filter(ImageFilter.GaussianBlur(blur_r))
    img = Image.alpha_composite(img, glow_layer)

    # Main bolt
    draw2 = ImageDraw.Draw(img)
    draw2.polygon(poly, fill=(*BOLT, 255))

    # Subtle highlight — narrow strip along the upper-right edge of each arm
    # Achieved by drawing a slightly offset version in bright gold
    offset = max(1, int(3 * scale))
    hi_poly = [(x - offset, y + offset) for x, y in poly]
    draw2.polygon(hi_poly, fill=(*BOLT_BRIGHT, 160))

    return img


def main():
    import os

    out_dir = os.path.join(os.path.dirname(__file__), "..", "app", "glitch", "assets")
    out_dir = os.path.normpath(out_dir)

    # 512x512 PNG
    img_512 = make_icon(512)
    img_512.save(os.path.join(out_dir, "glitch_icon_512.png"))
    print("Saved glitch_icon_512.png")

    # ICO with multiple embedded sizes (256 is max for Windows ICO display)
    ico_sizes = [16, 32, 48, 64, 128, 256]
    frames = [make_icon(s).convert("RGBA") for s in ico_sizes]
    # Pillow saves ICO using the last image as the base, append_images for others
    frames[-1].save(
        os.path.join(out_dir, "app_icon.ico"),
        format="ICO",
        append_images=frames[:-1],
        sizes=[(s, s) for s in ico_sizes],
    )
    print("Saved app_icon.ico")


if __name__ == "__main__":
    main()
