#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy-stl>=3.1", "matplotlib>=3.8"]
# ///
"""Render PNG previews of out/body.stl and out/lid.stl (front, back, iso views)."""
import sys
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from mpl_toolkits.mplot3d.art3d import Poly3DCollection
from stl import mesh

out = Path(__file__).parent / "out"
views = {"front": (-90, -90), "back": (90, -90), "iso_front": (25, -60), "iso_back": (-25, 60)}
for name in ("body", "lid"):
    m = mesh.Mesh.from_file(str(out / f"{name}.stl"))
    tri = m.vectors
    n = m.normals / (np.linalg.norm(m.normals, axis=1, keepdims=True) + 1e-9)
    shade = 0.35 + 0.65 * np.abs(n @ np.array([0.3, 0.5, 0.8]))
    cols = np.stack([shade * 0.55, shade * 0.7, shade * 0.9, np.ones_like(shade)], axis=1)
    for view, (elev, azim) in views.items():
        fig = plt.figure(figsize=(12, 6))
        ax = fig.add_subplot(111, projection="3d")
        ax.add_collection3d(Poly3DCollection(tri, facecolors=cols, edgecolor="none"))
        lo, hi = tri.reshape(-1, 3).min(0), tri.reshape(-1, 3).max(0)
        ax.set_xlim(lo[0], hi[0]); ax.set_ylim(lo[1], hi[1]); ax.set_zlim(lo[2], hi[2])
        ax.set_box_aspect(hi - lo)
        ax.view_init(elev=elev, azim=azim)
        ax.set_axis_off()
        fig.savefig(out / f"{name}_{view}.png", dpi=110, bbox_inches="tight")
        plt.close(fig)
print("ok")
