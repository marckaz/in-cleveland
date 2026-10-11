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


def one_line(t):
    return t.replace("%", "%25").replace("\r", "").replace("\n", "%0A")


def pdal(pipeline):
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump(pipeline, f)
    r = subprocess.run(["pdal", "pipeline", f.name, "--nostream"], capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(f"pdal failed ({r.returncode}): {one_line(r.stderr[-600:])}")
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
    cover, survey, url = found[0]
    note(f"lidar {key}: {survey} covers {cover:.0%}")
    ept = json.loads(urllib.request.urlopen(url, timeout=120).read())
    note(f"lidar {key}: survey srs {json.dumps(ept.get('srs'))[:200]}, bounds {ept.get('bounds')}")
    cls = classes(url, bounds + "/EPSG:3857")
    buildings = 6 in cls
    note(f"lidar {key}: classes {sorted(cls)}; buildings {'marked' if buildings else 'not marked'}")
    # Read it a kilometre square at a time (the whole region at once runs out of memory).
    W, H = int(round((x1 - x0) / RES)), int(round((y1 - y0) / RES))
    dtm = np.full((H, W), np.nan)
    top = np.full((H, W), np.nan)
    CH = 512  # cells per piece side
    pieces = [(i, j) for j in range(0, H, CH) for i in range(0, W, CH)]
    for k, (i, j) in enumerate(pieces):
        w_, h_ = min(CH, W - i), min(CH, H - j)
        cx0, cx1 = x0 + i * RES, x0 + (i + w_) * RES
        cy1 = y1 - j * RES
        cy0 = cy1 - h_ * RES
        pb = f"([{cx0}, {cx1}], [{cy0}, {cy1}])"
        tmp = tempfile.mkdtemp()
        grid = {"resolution": RES, "bounds": pb, "data_type": "float32", "nodata": -9999, "gdaldriver": "GTiff"}
        stages = [
            # Bounds given in web mercator whatever the survey is stored in; points reprojected to it.
            {"type": "readers.ept", "filename": url, "bounds": pb + "/EPSG:3857", "resolution": READ_RES, "threads": 8},
            {"type": "filters.reprojection", "out_srs": "EPSG:3857"},
            # Not noise (7: low, 18: high).
            {"type": "filters.range", "limits": "Classification![7:7],Classification![18:18]", "tag": "clean"},
            {"type": "filters.range", "inputs": ["clean"], "limits": "Classification[2:2]", "tag": "ground"},
            {"type": "writers.gdal", "inputs": ["ground"], "filename": f"{tmp}/dtm.tif", "output_type": "mean", **grid},
            {"type": "filters.range", "inputs": ["clean"], "limits": "Classification[6:6]" if buildings else "Classification[0:31]", "tag": "tops"},
            {"type": "writers.gdal", "inputs": ["tops"], "filename": f"{tmp}/top.tif", "output_type": "max", **grid},
        ]
        try:
            pdal(stages)
        except SystemExit as e:
            # A piece with no ground or no buildings (all lake) has nothing to write.
            print(f"::warning::lidar {key}: piece {k + 1} skipped: {str(e)[:600]}", file=sys.stderr)
            continue
        for arr, layer in ((dtm, "dtm"), (top, "top")):
            a = read(f"{tmp}/{layer}.tif")
            if k < 3:
                note(f"lidar {key}: piece {k + 1} {layer}: " + ("missing" if a is None else f"{np.isfinite(a).mean():.0%} filled, {np.nanmin(a) if np.isfinite(a).any() else 'nan'}..{np.nanmax(a) if np.isfinite(a).any() else 'nan'}"))
            if a is not None:
                a = a[:h_, :w_]
                arr[j:j + a.shape[0], i:i + a.shape[1]] = a
        if (k + 1) % 4 == 0 or k + 1 == len(pieces):
            print(f"lidar {key}: {k + 1}/{len(pieces)} pieces", flush=True)

    # The ground everywhere: filled in under buildings and water from around them.
    drv = gdal.GetDriverByName("MEM").Create("", W, H, 1, gdal.GDT_Float32)
    band = drv.GetRasterBand(1)
    band.SetNoDataValue(-9999)
    band.WriteArray(np.where(np.isfinite(dtm), dtm, -9999).astype(np.float32))
    gdal.FillNodata(band, None, maxSearchDist=400, smoothingIterations=2)
    filled = band.ReadAsArray().astype(np.float64)
    filled[filled == -9999] = np.nan
    h = top - filled
    h[~np.isfinite(h)] = 0.0
    h = np.clip(h, 0.0, 6553.0)
    if not np.isfinite(top).any():
        raise SystemExit(f"{key}: no lidar points were read")
    note(f"lidar {key}: grid {h.shape[1]}x{h.shape[0]}, tallest {h.max():.1f} m, {np.mean(h > 2.5):.0%} of it above 2.5 m")
    os.makedirs(os.path.join(ROOT, "data", "lidar"), exist_ok=True)
    # Row 0 is the north edge (GDAL's order).
    Image.fromarray(np.round(h * 10).astype(np.uint16)).save(os.path.join(ROOT, "data", "lidar", f"{key}.png"), optimize=True)
    json.dump(
        {
            "bounds_3857": [x0, y0, x1, y1],
            "res": RES,
            "units": "uint16 decimetres above the ground; row 0 is the north edge",
            "survey": survey,
            "buildings_marked": buildings,
            "source": "USGS 3D Elevation Program lidar (public domain), via the Entwine copies on AWS",
        },
        open(os.path.join(ROOT, "data", "lidar", f"{key}.json"), "w"),
        indent=1,
    )


def main():
    import traceback
    keys = sys.argv[1:] or list(REGIONS)
    failed = []
    for k in keys:
        try:
            region(k, REGIONS[k])
        except BaseException as e:
            print(f"::error::lidar {k}: {one_line(str(e))}%0A{one_line(traceback.format_exc()[-1200:])}", file=sys.stderr)
            failed.append(k)
    if failed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
