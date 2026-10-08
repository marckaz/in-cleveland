#!/usr/bin/env python3
"""Download a region's terrain and aerial photos.

    python3 tools/geo/fetch_geo.py <region>

- Terrain: AWS Terrain Tiles (terrarium PNGs, zoom 14, about 7 m a pixel; mostly USGS 3DEP in
  the US), stitched and saved as data/terrain/<region>.png: 16-bit greyscale, decimetres above
  100 m below sea level. Sources: https://github.com/tilezen/joerd/blob/master/docs/attribution.md
- Aerial photos: USGS National Map imagery (public domain), zoom 16 (about 1.8 m a pixel),
  stitched and saved as data/aerial/<region>.jpg.

Each file also gets a .json beside it with the exact lat/lon box the pixels cover.
Needs Pillow (pip install pillow).
"""
import io
import json
import math
import os
import sys
import time
import urllib.request

from PIL import Image

REGIONS = json.load(open(os.path.join(os.path.dirname(__file__), "..", "osm", "regions.json")))
UA = {"User-Agent": "in-cleveland map fetch (github.com/marckaz/in-cleveland)"}


def tile_xy(lat, lon, z):
    n = 2 ** z
    x = (lon + 180.0) / 360.0 * n
    y = (1.0 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2.0 * n
    return x, y


def tile_lat(y, z):
    n = math.pi - 2.0 * math.pi * y / 2 ** z
    return math.degrees(math.atan(math.sinh(n)))


def get(url):
    for attempt in range(5):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=60) as r:
                return r.read()
        except Exception as e:
            print(f"::warning::{url}: {e}", file=sys.stderr)
            time.sleep(3 + attempt * 4)
    raise SystemExit(f"::error::could not fetch {url}")


def mosaic(bbox, z, url_of, mode):
    s, w, n, e = bbox
    x0, y0 = tile_xy(n, w, z)
    x1, y1 = tile_xy(s, e, z)
    tx0, ty0, tx1, ty1 = int(x0), int(y0), int(x1), int(y1)
    img = Image.new(mode, ((tx1 - tx0 + 1) * 256, (ty1 - ty0 + 1) * 256))
    for ty in range(ty0, ty1 + 1):
        for tx in range(tx0, tx1 + 1):
            t = Image.open(io.BytesIO(get(url_of(z, tx, ty)))).convert(mode)
            img.paste(t.resize((256, 256)), ((tx - tx0) * 256, (ty - ty0) * 256))
    # Crop to the box.
    px = lambda v: int(round(v * 256))
    box = (px(x0 - tx0), px(y0 - ty0), px(x1 - tx0), px(y1 - ty0))
    img = img.crop(box)
    # The box the cropped pixels really cover.
    cover = [tile_lat(ty0 + box[3] / 256, z), (tx0 + box[0] / 256) / 2 ** z * 360 - 180,
             tile_lat(ty0 + box[1] / 256, z), (tx0 + box[2] / 256) / 2 ** z * 360 - 180]
    return img, cover


def main():
    region = sys.argv[1]
    bbox = REGIONS[region]["bbox"]
    os.makedirs("data/terrain", exist_ok=True)
    os.makedirs("data/aerial", exist_ok=True)

    terr, cover = mosaic(bbox, 14, lambda z, x, y: f"https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png", "RGB")
    out = Image.new("I;16", terr.size)
    px_in, px_out = terr.load(), out.load()
    for j in range(terr.size[1]):
        for i in range(terr.size[0]):
            r, g, b = px_in[i, j]
            h = r * 256 + g + b / 256 - 32768  # metres
            px_out[i, j] = max(0, min(65535, int(round((h + 100.0) * 10))))
    out.save(f"data/terrain/{region}.png")
    json.dump({"bbox": cover, "encoding": "uint16 decimetres above -100 m", "source": "AWS Terrain Tiles (terrarium, z14)"}, open(f"data/terrain/{region}.json", "w"))
    print(f"::notice::terrain {region}: {out.size}")

    img, cover = mosaic(bbox, 16, lambda z, x, y: f"https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/tile/{z}/{y}/{x}", "RGB")
    img.save(f"data/aerial/{region}.jpg", quality=86)
    json.dump({"bbox": cover, "source": "USGS The National Map, USGSImageryOnly (public domain), z16"}, open(f"data/aerial/{region}.json", "w"))
    print(f"::notice::aerial {region}: {img.size}")


if __name__ == "__main__":
    main()
