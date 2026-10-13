"""Generate app icons (blue rounded square + 译 glyph) without external deps."""
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src-tauri", "icons")

GLYPH = [
    "................",
    "................",
    "..##........##..",
    "..##........##..",
    "..############..",
    "..##........##..",
    "..##........##..",
    "..##........##..",
    "..#############.",
    "................",
    "................",
    "..##.......###..",
    "..##.......###..",
    "..##......##....",
    "..##.....##.....",
    "................",
]

BG = (26, 115, 232, 255)
FG = (255, 255, 255, 255)


def write_png(path, size):
    grid = [[BG] * size for _ in range(size)]
    scale = size / 16.0
    for gy, line in enumerate(GLYPH):
        for gx, ch in enumerate(line):
            if ch != "#":
                continue
            x0, y0 = int(gx * scale), int(gy * scale)
            x1 = max(x0 + 1, int((gx + 1) * scale))
            y1 = max(y0 + 1, int((gy + 1) * scale))
            for y in range(y0, min(y1, size)):
                for x in range(x0, min(x1, size)):
                    grid[y][x] = FG

    rows = bytearray()
    for y in range(size):
        rows.append(0)  # filter type 0
        for x in range(size):
            rows.extend(grid[y][x])

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(rows), 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)
    return png


def main():
    os.makedirs(OUT, exist_ok=True)
    blobs = {}
    for size in (32, 128, 256, 512):
        blobs[size] = write_png(os.path.join(OUT, f"{size}x{size}.png"), size)
        print(f"{size}x{size}.png  {len(blobs[size])} B")

    sizes = [32, 64, 128, 256]
    ico = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    payload = b""
    for s in sizes:
        data = blobs[s] if s in blobs else write_png(os.path.join(OUT, f"{s}x{s}.png"), s)
        # ICO stores 256 as 0 in the single-byte width/height fields
        dim = 0 if s >= 256 else s
        ico += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        payload += data
        offset += len(data)
    with open(os.path.join(OUT, "icon.ico"), "wb") as f:
        f.write(ico + payload)
    print(f"icon.ico  {len(ico + payload)} B")


if __name__ == "__main__":
    main()