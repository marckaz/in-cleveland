#!/usr/bin/env python3
"""Measure how tall everything is, from USGS lidar, for every region.

    python3 tools/geo/fetch_lidar.py [region ...]

Needs PDAL and GDAL with their Python bindings (the "Fetch lidar and photos" workflow installs
them with micromamba). For each region in tools/osm/regions.json this reads the USGS 3DEP lidar
survey that covers it (public domain; streamed from the Entwine copies on AWS, found through
hobuinc/usgs-lidar's boundaries) and writes:

- data/lidar/<region>.png: height above the ground, 16-bit greyscale, in decimetres, on a grid
  of RES web-mercator metres (about 1.5 m on the ground in Cleveland). 0 is the ground.
- data/lidar/<region>.json: the grid's bounds (EPSG:3857), which survey, and whether the survey
  marks buildings (class 6). Where it does, only building points count, so trees over a house
  don't make it taller.

Heights are each cell's highest point minus the ground under it; the ground under buildings
(where the lidar can't see it) is filled in from around them.
"""
import json
import math
import os
import subprocess
import sys
import tempfile
import urllib.request

import numpy as np
from osgeo import gdal
from PIL import Image

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
REGIONS = json.load(open(os.path.join(ROOT, "tools", "osm", "regions.json")))
BOUNDARIES = "https://raw.githubusercontent.com/hobuinc/usgs-lidar/master/boundaries/resources.geojson"
RES = 2.0  # web-mercator metres per cell
READ_RES = 1.0  # how finely to read the point cloud (EPT resolution)


def note(msg):
    print(f"::notice::{msg}", file=sys.stderr)
    print(msg, flush=True)


def merc(lat, lon):
    x = math.radians(lon) * 6378137.0
    y = math.log(math.tan(math.pi / 4 + math.radians(lat) / 2)) * 6378137.0
    return x, y


def surveys(bbox):
    """The lidar surveys over a box (s, w, n, e), most of it first."""
    from shapely.geometry import box, shape

    d = json.loads(urllib.request.urlopen(BOUNDARIES, timeout=120).read())
    s, w, n, e = bbox
    b = box(w, s, e, n)
    out = []
    for f in d["features"]:
        g = shape(f["geometry"])
        if g.intersects(b):
            out.append((g.intersection(b).area / b.area, f["properties"]["name"], f["properties"]["url"]))
    out.sort(reverse=True)
    return out


def pdal(pipeline):
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump(pipeline, f)
    r = subprocess.run(["pdal", "pipeline", f.name, "--nostream"], capture_output=True, text=True)
    if r.returncode != 0:
        print(r.stdout[-3000:], r.stderr[-3000:], file=sys.stderr)
        raise SystemExit(f"::error::pdal failed: {r.stderr[-400:]}")
    return r.stdout


def classes(url, bounds):
    """Which point classes a survey has in these bounds (from a coarse read)."""
    out = subprocess.run(
        ["pdal", "info", "--readers.ept.bounds=" + bounds, "--readers.ept.resolution=20", "--enumerate", "Classification", "--stats", url],
        capture_output=True, text=True,
    )
    try:
        info = json.loads(out.stdout)
        for s in info["stats"]["statistic"]:
            if s["name"] == "Classification":
                return set(int(v) for v in s.get("values", []))
    except Exception as e:
        print(f"::warning::class check failed ({e}): {out.stderr[-300:]}", file=sys.stderr)
    return set()


def read(path):
    ds = gdal.Open(path)
    if ds is None:
        return None
    a = ds.GetRasterBand(1).ReadAsArray().astype(np.float64)
    nd = ds.GetRasterBand(1).GetNoDataValue()
    if nd is not None:
        a[a == nd] = np.nan
    return a


def region(key, cfg):
    s, w, n, e = cfg["bbox"]
    x0, y0 = merc(s, w)
    x1, y1 = merc(n, e)
    # Snap to the grid so every raster lines up.
    x0, y0 = math.floor(x0 / RES) * RES, math.floor(y0 / RES) * RES
    x1, y1 = math.ceil(x1 / RES) * RES, math.ceil(y1 / RES) * RES
    bounds = f"([{x0}, {x1}], [{y0}, {y1}])"
    found = surveys(cfg["bbox"])
    if not found:
        raise SystemExit(f"::error::{key}: no lidar survey covers it")
    cover, name, url = found[0]
    note(f"lidar {key}: {name} covers {cover:.0%}")
    cls = classes(url, bounds)
    buildings = 6 in cls
    note(f"lidar {key}: classes {sorted(cls)}; buildings {'marked' if buildings else 'not marked'}")
    tmp = tempfile.mkdtemp()
    grid = {"resolution": RES, "bounds": bounds, "data_type": "float32", "nodata": -9999, "gdaldriver": "GTiff"}
    stages = [
        {"type": "readers.ept", "filename": url, "bounds": bounds, "resolution": READ_RES, "threads": 8},
        # Not noise (7: low, 18: high).
        {"type": "filters.range", "limits": "Classification![7:7],Classification![18:18]", "tag": "clean"},
        {"type": "filters.range", "inputs": ["clean"], "limits": "Classification[2:2]", "tag": "ground"},
        {"type": "writers.gdal", "inputs": ["ground"], "filename": f"{tmp}/dtm.tif", "output_type": "mean", **grid},
    ]
    if buildings:
        stages += [
            {"type": "filters.range", "inputs": ["clean"], "limits": "Classification[6:6]", "tag": "bld"},
            {"type": "writers.gdal", "inputs": ["bld"], "filename": f"{tmp}/top.tif", "output_type": "max", **grid},
        ]
    else:
        stages += [{"type": "writers.gdal", "inputs": ["clean"], "filename": f"{tmp}/top.tif", "output_type": "max", **grid}]
    pdal(stages)

    # The ground everywhere: filled in under buildings and water from around them.
    ds = gdal.Open(f"{tmp}/dtm.tif", gdal.GA_Update)
    gdal.FillNodata(ds.GetRasterBand(1), None, maxSearchDist=400, smoothingIterations=2)
    ds = None
    dtm = read(f"{tmp}/dtm.tif")
    top = read(f"{tmp}/top.tif")
    h = top - dtm
    h[~np.isfinite(h)] = 0.0
    h = np.clip(h, 0.0, 6553.0)
    note(f"lidar {key}: grid {h.shape[1]}x{h.shape[0]}, tallest {h.max():.1f} m, {np.mean(h > 2.5):.0%} of it above 2.5 m")
    os.makedirs(os.path.join(ROOT, "data", "lidar"), exist_ok=True)
    # Row 0 is the north edge (GDAL's order).
    Image.fromarray(np.round(h * 10).astype(np.uint16)).save(os.path.join(ROOT, "data", "lidar", f"{key}.png"), optimize=True)
    json.dump(
        {
            "bounds_3857": [x0, y0, x1, y1],
            "res": RES,
            "units": "uint16 decimetres above the ground; row 0 is the north edge",
            "survey": name,
            "buildings_marked": buildings,
            "source": "USGS 3D Elevation Program lidar (public domain), via the Entwine copies on AWS",
        },
        open(os.path.join(ROOT, "data", "lidar", f"{key}.json"), "w"),
        indent=1,
    )


def main():
    keys = sys.argv[1:] or list(REGIONS)
    for k in keys:
        region(k, REGIONS[k])


if __name__ == "__main__":
    main()
