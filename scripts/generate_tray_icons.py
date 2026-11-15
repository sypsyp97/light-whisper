




import math
from pathlib import Path
import struct
import zlib

ICONS = Path(__file__).resolve().parents[1] / "src-tauri" / "icons"
SIZES = (16, 20, 24, 32)


def icon(size, recording=False):

    pad, radius, stroke = {16: (1, 4, 1.5), 20: (1, 5, 2), 24: (2, 6, 2), 32: (2, 8, 3)}[size]
    cy = size / 2
    arcs = [(size * 0.51, size * 0.78, size * 0.25)]
    if size >= 20:
        arcs.append((size * 0.52, size * 0.64, size * 0.13))
    points = [[(left + (right - left) * math.sin(t), cy - height * math.cos(t))
               for t in (i * math.pi / 48 for i in range(49))]
              for left, right, height in arcs]
    white, warm, signal = (253, 252, 251), (174, 86, 48), (229, 77, 55)

    def color(x, y):
        cx = max(pad + radius, min(size - pad - radius, x))
        yy = max(pad + radius, min(size - pad - radius, y))
        inside = math.hypot(x - cx, y - yy) <= radius
        result = (*warm, 255) if inside else (0, 0, 0, 0)
        if math.hypot(x - size * 0.29, y - cy) < stroke:
            result = (*white, 255)
        if any(math.hypot(x - px, y - py) <= stroke / 2 for arc in points for px, py in arc):
            result = (*white, 255)
        if recording:
            distance = math.hypot(x - (size - pad - 2), y - (size - pad - 2))
            if distance <= size * 0.18:
                result = (*white, 255)
            if distance <= size * 0.12:
                result = (*signal, 255)
        return result

    pixels = bytearray()
    for y in range(size):
        for x in range(size):
            samples = [color(x + (sx + 0.5) / 4, y + (sy + 0.5) / 4) for sy in range(4) for sx in range(4)]
            alpha = sum(c[3] for c in samples)
            pixels.extend([round(sum(c[channel] * c[3] for c in samples) / alpha) if alpha else 0 for channel in range(3)])
            pixels.append(round(alpha / 16))
    return bytes(pixels)


def png(rgba, size):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = b"".join(b"\0" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">2I5B", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def main():
    small = {}
    for size in SIZES:
        for recording in (False, True):
            name = f"tray-{size}" + ("-recording" if recording else "")
            rgba = icon(size, recording)
            (ICONS / f"{name}.rgba").write_bytes(rgba)
            image = png(rgba, size)
            (ICONS / f"{name}.png").write_bytes(image)
            if not recording:
                small[size] = image

    (ICONS / "32x32.png").write_bytes(small[32])


    path = ICONS / "icon.ico"
    original = path.read_bytes()
    entries = [(size, size, data) for size, data in small.items()]
    for index in range(struct.unpack_from("<H", original, 4)[0]):
        width, height, _, _, _, _, length, offset = struct.unpack_from("<4B2H2I", original, 6 + index * 16)
        if (width or 256) > 32:
            entries.append((width, height, original[offset:offset + length]))
    offset = 6 + 16 * len(entries)
    headers, images = bytearray(), bytearray()
    for width, height, data in entries:
        headers.extend(struct.pack("<4B2H2I", width, height, 0, 0, 1, 32, len(data), offset))
        images.extend(data)
        offset += len(data)
    path.write_bytes(struct.pack("<3H", 0, 1, len(entries)) + headers + images)


if __name__ == "__main__":
    main()
