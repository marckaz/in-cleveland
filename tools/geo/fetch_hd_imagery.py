#!/usr/bin/env python3
"""Download sharp aerial photos for every region: about half a metre a pixel.

    python3 tools/geo/fetch_hd_imagery.py [region ...]

From the Ohio Statewide Imagery Program (OSIP), "available for public use"
(https://maps.ohio.gov/image/rest/services/osip_most_current/ImageServer), requested in
plain latitude/longitude pieces and stitched into data/aerial_hd/<region>.jpg, with a .json
beside it giving the exact box. If the State's server won't answer, falls back to the USGS
National Map imagery (public domain) at zoom 18.

Needs Pillow.
"""
import io
import json
import math
import os
import sys
import time
import urllib.parse
import urllib.request

from PIL import Image

Image.MAX_IMAGE_PIXELS = None
ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
REGIONS = json.load(open(os.path.join(ROOT, "tools", "osm", "regions.json")))
OHIO = "https://maps.ohio.gov/image/rest/services/osip_most_current/ImageServer"
USGS = "https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/tile/{z}/{y}/{x}"
UA = {"User-Agent": "in-cleveland map fetch (github.com/marckaz/in-cleveland)"}
M_PER_PX = 0.5
PIECE = 2000


def note(msg):
    print(f"::notice::{msg}", file=sys.stderr)
    print(msg, flush=True)


def get(url, timeout=180, tries=5):
    last = None
    for attempt in range(tries):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=timeout) as r:
                return r.read(), r.headers.get("Content-Type", "")
        except Exception as e:
            last = e
            print(f"::warning::{url[:160]}: {e}", file=sys.stderr)
            time.sleep(3 + attempt * 5)
    raise RuntimeError(f"could not fetch {url[:160]}: {last}")


def merc(lat, lon):
    return math.radians(lon) * 6378137.0, math.log(math.tan(math.pi / 4 + math.radians(lat) / 2)) * 6378137.0


def ohio(bbox):
    s, w, n, e = bbox
    info = json.loads(get(OHIO + "?f=pjson")[0])
    piece = min(PIECE, int(info.get("maxImageWidth", PIECE)), int(info.get("maxImageHeight", PIECE)))
    lat0 = (s + n) / 2
    m_lon = 111_320.0 * math.cos(math.radians(lat0))
    W = int(round((e - w) * m_lon / M_PER_PX))
    H = int(round((n - s) * 111_132.0 / M_PER_PX))
    note(f"ohio imagery: {W}x{H} px in pieces of {piece}")
    out = Image.new("RGB", (W, H))
    for py in range(0, H, piece):
        for px in range(0, W, piece):
            pw, ph = min(piece, W - px), min(piece, H - py)
            bw = w + (e - w) * px / W
            be = w + (e - w) * (px + pw) / W
            bn = n - (n - s) * py / H
            bs = n - (n - s) * (py + ph) / H
            # Asked for in web mercator, where pixels are square on the ground; over a piece a
            # kilometre across that's the same as plain latitude to a few centimetres.
            mx0, my0 = merc(bs, bw)
            mx1, my1 = merc(bn, be)
            q = urllib.parse.urlencode({
                "bbox": f"{mx0},{my0},{mx1},{my1}", "bboxSR": 3857, "imageSR": 3857, "size": f"{pw},{ph}",
                "format": "jpg", "compressionQuality": 90, "interpolation": "RSP_BilinearInterpolation", "f": "image",
            })
            data, ctype = get(f"{OHIO}/exportImage?{q}")
            if "image" not in ctype:
                raise RuntimeError(f"not an image: {data[:200]!r}")
            out.paste(Image.open(io.BytesIO(data)).convert("RGB").resize((pw, ph)), (px, py))
        note(f"ohio imagery: row {py // piece + 1} of {math.ceil(H / piece)}")
    return out, [s, w, n, e], "Ohio Statewide Imagery Program (OSIP), State of Ohio, available for public use"


def tile_xy(lat, lon, z):
    n = 2 ** z
    return (lon + 180.0) / 360.0 * n, (1.0 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2.0 * n


def tile_lat(y, z):
    return math.degrees(math.atan(math.sinh(math.pi - 2.0 * math.pi * y / 2 ** z)))


def usgs(bbox, z=18):
    s, w, n, e = bbox
    x0, y0 = tile_xy(n, w, z)
    x1, y1 = tile_xy(s, e, z)
    tx0, ty0, tx1, ty1 = int(x0), int(y0), int(x1), int(y1)
    img = Image.new("RGB", ((tx1 - tx0 + 1) * 256, (ty1 - ty0 + 1) * 256))
    for ty in range(ty0, ty1 + 1):
        for tx in range(tx0, tx1 + 1):
            data, _ = get(USGS.format(z=z, x=tx, y=ty), timeout=60)
            img.paste(Image.open(io.BytesIO(data)).convert("RGB"), ((tx - tx0) * 256, (ty - ty0) * 256))
        note(f"usgs imagery: row {ty - ty0 + 1} of {ty1 - ty0 + 1}")
    nb = [tile_lat(ty1 + 1, z), tx0 / 2 ** z * 360 - 180, tile_lat(ty0, z), (tx1 + 1) / 2 ** z * 360 - 180]
    # Mercator rows to plain latitude: resample so latitude is linear down the image.
    H = img.size[1]
    rows = []
    for r in range(H):
        lat = nb[2] - (nb[2] - nb[0]) * (r + 0.5) / H
        rows.append(min(H - 1, int((tile_xy(lat, 0, z)[1] - ty0) * 256)))
    lin = Image.new("RGB", img.size)
    for r, src in enumerate(rows):
        lin.paste(img.crop((0, src, img.size[0], src + 1)), (0, r))
    return lin, nb, "USGS The National Map imagery (public domain)"


def main():
    keys = sys.argv[1:] or list(REGIONS)
    os.makedirs(os.path.join(ROOT, "data", "aerial_hd"), exist_ok=True)
    for k in keys:
        bbox = REGIONS[k]["bbox"]
        try:
            img, bb, src = ohio(bbox)
        except Exception as e:
            print(f"::warning::{k}: Ohio imagery failed ({e}); using USGS zoom 18", file=sys.stderr)
            img, bb, src = usgs(bbox)
        img.save(os.path.join(ROOT, "data", "aerial_hd", f"{k}.jpg"), quality=82, optimize=True)
        json.dump({"bbox": bb, "source": src, "m_per_px": M_PER_PX}, open(os.path.join(ROOT, "data", "aerial_hd", f"{k}.json"), "w"))
        note(f"{k}: {img.size[0]}x{img.size[1]} from {src}")


if __name__ == "__main__":
    main()
