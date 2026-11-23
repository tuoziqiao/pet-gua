"""Generate the two seamless cloud-opacity textures used by the weather page.

Requires NumPy only when regenerating the checked-in PNGs, not at runtime.
"""
from pathlib import Path
import struct
import zlib

import numpy as np

SIZE = 512
ROOT = Path(__file__).resolve().parent.parent / "assets" / "weather"


def field(rng, stretch_x, stretch_y, detail):
    noise = rng.standard_normal((SIZE, SIZE))
    fx = np.fft.fftfreq(SIZE)[None, :] * stretch_x
    fy = np.fft.fftfreq(SIZE)[:, None] * stretch_y
    frequency = fx * fx + fy * fy
    spectrum = np.fft.fft2(noise) / (1.0 + frequency) ** detail
    result = np.fft.ifft2(spectrum).real
    return (result - result.mean()) / result.std()


def write_png(path, alpha):
    rgba = np.full((SIZE, SIZE, 4), 255, dtype=np.uint8)
    rgba[:, :, 3] = alpha
    scanlines = b"".join(b"\0" + row.tobytes() for row in rgba)

    def chunk(kind, payload):
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))

    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(scanlines, 9)) + chunk(b"IEND", b""))


def main():
    ROOT.mkdir(parents=True, exist_ok=True)
    for seed, name in [(1441, "cloud-near.png"), (7277, "cloud-far.png")]:
        rng = np.random.default_rng(seed)
        base = field(rng, 150, 72, 1.45)
        detail = field(rng, 65, 34, 1.35)
        warp_x = field(rng, 160, 100, 1.5)
        warp_y = field(rng, 180, 120, 1.5)
        yy, xx = np.indices((SIZE, SIZE))
        wx = (xx + warp_x * 6).astype(np.int32) % SIZE
        wy = (yy + warp_y * 4).astype(np.int32) % SIZE
        density = base[wy, wx] * 0.82 + detail * 0.25
        density = np.clip((density - 0.12) / 1.6, 0.0, 1.0)
        density = density * density * (3.0 - 2.0 * density)
        write_png(ROOT / name, np.rint(density * 190).astype(np.uint8))


if __name__ == "__main__":
    main()
