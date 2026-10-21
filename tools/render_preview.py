"""Rasterize the actual egui meshes exported by the GUI layout test.

Requires numpy and Pillow. This renderer is used for visual verification only.
Run: cargo test editor_renders_real_signal_and_exports_meshes
     python tools/render_preview.py
"""
import json
from pathlib import Path

import numpy as np
from PIL import Image


def render(source: Path, destination: Path) -> None:
    data = json.loads(source.read_text(encoding="utf-8"))
    textures = {}
    for update in data["textures"]:
        texture = update["id"]
        w, h = update["size"]
        pixels = np.array(update["pixels"], dtype=np.float32).reshape(h, w, 4) / 255
        if update["pos"] is None:
            textures[texture] = pixels.copy()
        else:
            x, y = update["pos"]
            textures[texture][y:y + h, x:x + w] = pixels
    assert textures, "egui did not export any textures"
    height, width = data["height"], data["width"]
    canvas = np.zeros((height, width, 4), dtype=np.float32)
    canvas[:, :, 3] = 1

    for mesh in data["meshes"]:
        atlas = textures[mesh["texture"]]
        th, tw = atlas.shape[:2]
        vertices = np.asarray(mesh["vertices"], dtype=np.float32)
        clip = mesh["clip"]
        for indices in np.asarray(mesh["indices"]).reshape(-1, 3):
            tri = vertices[indices]
            x0 = max(0, int(np.floor(tri[:, 0].min())), int(np.ceil(clip[0])))
            y0 = max(0, int(np.floor(tri[:, 1].min())), int(np.ceil(clip[1])))
            x1 = min(width, int(np.ceil(tri[:, 0].max())), int(np.floor(clip[2])))
            y1 = min(height, int(np.ceil(tri[:, 1].max())), int(np.floor(clip[3])))
            if x1 <= x0 or y1 <= y0:
                continue
            a, b, c = tri[:, :2]
            denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
            if abs(denominator) < 1e-8:
                continue
            yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32) + 0.5
            wa = ((b[1] - c[1]) * (xx - c[0]) + (c[0] - b[0]) * (yy - c[1])) / denominator
            wb = ((c[1] - a[1]) * (xx - c[0]) + (a[0] - c[0]) * (yy - c[1])) / denominator
            wc = 1 - wa - wb
            mask = (wa >= -1e-7) & (wb >= -1e-7) & (wc >= -1e-7)
            if not mask.any():
                continue
            weights = np.stack((wa[mask], wb[mask], wc[mask]), axis=-1)
            attributes = weights @ tri[:, 2:]
            uv = attributes[:, :2] * [tw, th] - 0.5
            uv[:, 0] = np.clip(uv[:, 0], 0, tw - 1)
            uv[:, 1] = np.clip(uv[:, 1], 0, th - 1)
            lo = np.floor(uv).astype(int)
            hi = np.minimum(lo + 1, [tw - 1, th - 1])
            frac = uv - lo
            fx, fy = frac[:, 0:1], frac[:, 1:2]
            texel = (atlas[lo[:, 1], lo[:, 0]] * (1 - fx) * (1 - fy)
                     + atlas[lo[:, 1], hi[:, 0]] * fx * (1 - fy)
                     + atlas[hi[:, 1], lo[:, 0]] * (1 - fx) * fy
                     + atlas[hi[:, 1], hi[:, 0]] * fx * fy)
            rgba = texel * attributes[:, 2:] / 255
            region = canvas[y0:y1, x0:x1]
            region[mask] = rgba + region[mask] * (1 - rgba[:, 3:4])

    destination.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(np.clip(canvas * 255, 0, 255).astype(np.uint8)).save(destination)
    print(destination.resolve())


if __name__ == "__main__":
    render(Path("target/preview/egui.json"), Path("docs/screenshot.png"))
