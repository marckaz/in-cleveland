#!/usr/bin/env python3
"""Download Microsoft's machine-learned building footprints for every region.

    python3 tools/geo/fetch_footprints.py

OpenStreetMap is missing many houses around Cleveland (most of north Lakewood, for one). The
bake fills those gaps from Microsoft's Global ML Building Footprints, which are released under
the same licence as OpenStreetMap (ODbL 1.0): https://github.com/microsoft/GlobalMLBuildingFootprints

The data comes in gzipped tiles of GeoJSON lines, one tile per zoom-9 quadkey, listed in
dataset-links.csv. This downloads the tiles over the regions in tools/osm/regions.json and keeps
the footprints inside each region's box, as data/footprints/<region>.json:
{"source": ..., "buildings": [[height_m or -1, [[lon, lat], ...]], ...]}.
"""
import csv
import gzip
import io
import json
import math
import os
import sys
import time
import urllib.request

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
REGIONS = json.load(open(os.path.join(ROOT, "tools", "osm", "regions.json")))
LINKS = "https://minedbuildings.z5.web.core.windows.net/global-buildings/dataset-links.csv"
UA = {"User-Agent": "in-cleveland map fetch (github.com/marckaz/in-cleveland)"}


def note(msg):
    print(f"::notice::{msg}", file=sys.stderr)
    print(msg)


def get(url, timeout=600):
    for attempt in range(4):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=timeout) as r:
                return r.read()
        except Exception as e:
            print(f"::warning::{url}: {e}", file=sys.stderr)
            time.sleep(5 + attempt * 10)
    raise SystemExit(f"::error::could not fetch {url}")


def quadkey(lat, lon, z=9):
    n = 2 ** z
    x = int((lon + 180.0) / 360.0 * n)
    y = int((1.0 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2.0 * n)
    key = ""
    for i in range(z, 0, -1):
        d = 0
        m = 1 << (i - 1)
        if x & m:
            d += 1
        if y & m:
            d += 2
        key += str(d)
    return key


def main():
    boxes = {r: cfg["bbox"] for r, cfg in REGIONS.items()}  # s, w, n, e
    keys = set()
    for s, w, n, e in boxes.values():
        for lat in (s, n):
            for lon in (w, e):
                keys.add(quadkey(lat, lon))
    note(f"footprints: quadkeys {sorted(keys)}")
    rows = list(csv.DictReader(io.StringIO(get(LINKS).decode())))
    urls = [r["Url"] for r in rows if r.get("QuadKey") in keys]
    if not urls:
        raise SystemExit(f"::error::no footprint tiles for {sorted(keys)} in {LINKS}")
    note(f"footprints: {len(urls)} tiles")
    found = {r: [] for r in boxes}
    for url in urls:
        raw = gzip.decompress(get(url))
        n_all = 0
        for line in raw.splitlines():
            line = line.strip()
            if not line:
                continue
            # A CSV of one column: each row is a GeoJSON feature (maybe quoted).
            if line.startswith(b'"'):
                line = next(csv.reader([line.decode()]))[0].encode()
            try:
                f = json.loads(line)
            except ValueError:
                continue
            n_all += 1
            g = f.get("geometry") or {}
            if g.get("type") != "Polygon" or not g.get("coordinates"):
                continue
            ring = g["coordinates"][0]
            lon, lat = ring[0][0], ring[0][1]
            for r, (s, w, n, e) in boxes.items():
                if s <= lat <= n and w <= lon <= e:
                    h = (f.get("properties") or {}).get("height", -1)
                    h = -1 if h is None else round(float(h), 1)
                    found[r].append([h, [[round(x, 7), round(y, 7)] for x, y in ring]])
        note(f"footprints: {url.rsplit('/', 2)[-2]} read ({n_all} buildings)")
    os.makedirs(os.path.join(ROOT, "data", "footprints"), exist_ok=True)
    for r, b in found.items():
        path = os.path.join(ROOT, "data", "footprints", f"{r}.json")
        json.dump({"source": "Microsoft Global ML Building Footprints (ODbL 1.0)", "buildings": b}, open(path, "w"), separators=(",", ":"))
        note(f"footprints: {r}: {len(b)}")


if __name__ == "__main__":
    main()
