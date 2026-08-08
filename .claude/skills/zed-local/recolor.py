"""Recolor Zed preview (blue) icon to crimson by hue rotation; build dev PNGs and ICO."""
import sys
from PIL import Image

def recolor(src, dst, shift):
    img = Image.open(src).convert("RGBA")
    alpha = img.getchannel("A")
    h, s, v = img.convert("RGB").convert("HSV").split()
    h = h.point(lambda x: (x + shift) % 256)
    out = Image.merge("HSV", (h, s, v)).convert("RGB")
    out.putalpha(alpha)
    out.save(dst)
    return out

if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "recolor":
        src, dst, shift = sys.argv[2], sys.argv[3], int(sys.argv[4])
        recolor(src, dst, shift)
        print(f"wrote {dst}")
    elif cmd == "ico":
        src, dst = sys.argv[2], sys.argv[3]
        img = Image.open(src).convert("RGBA")
        img.save(
            dst,
            format="ICO",
            bitmap_format="bmp",
            sizes=[(256, 256), (128, 128), (64, 64), (32, 32), (16, 16)],
        )
        print(f"wrote {dst}")
