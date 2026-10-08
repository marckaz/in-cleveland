#!/usr/bin/env python3
"""Download downtown Cleveland from OpenStreetMap (Overpass API) into data/osm/cleveland.json.

Buildings (and their 3D parts), streets, water and parks, from the West Side Market to
Playhouse Square and the lakefront. Run by .github/workflows/osm.yml; `bake.py` turns the
result into the game's map. Map data (c) OpenStreetMap contributors, ODbL.
"""
import json
import sys
import time
import urllib.parse
import urllib.request

# south, west, north, east
BBOX = (41.4800, -81.7120, 41.5130, -81.6740)
MIRRORS = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
    "https://maps.mail.ru/osm/tools/overpass/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
]


def note(msg, level="notice"):
    """Print, and on GitHub Actions also as an annotation (visible without the raw log)."""
    print(f"::{level}::{msg}" if "GITHUB_ACTIONS" in __import__("os").environ else msg, file=sys.stderr, flush=True)


def query(q):
    data = urllib.parse.urlencode({"data": q}).encode()
    last = None
    for attempt in range(12):
        url = MIRRORS[attempt % len(MIRRORS)]
        try:
            req = urllib.request.Request(url, data=data, headers={"User-Agent": "in-cleveland map fetch (github.com/marckaz/in-cleveland)"})
            with urllib.request.urlopen(req, timeout=240) as r:
                body = r.read()
            j = json.loads(body)
            if "remark" in j and not j.get("elements"):
                raise RuntimeError(f"server remark: {j['remark'][:200]}")
            return j
        except Exception as e:  # busy servers answer 429/504; try the next one
            last = e
            note(f"{url}: {type(e).__name__}: {str(e)[:200]}; retrying", "warning")
            time.sleep(8 + attempt * 6)
    note(f"Overpass failed: {last}", "error")
    raise SystemExit(1)


def main():
    bb = ",".join(str(v) for v in BBOX)
    parts = {
        "buildings": f'(way["building"]({bb});relation["building"]({bb});way["building:part"]({bb});relation["building:part"]({bb}););out tags geom;',
        "streets": f'(way["highway"~"^(motorway|trunk|primary|secondary|tertiary|unclassified|residential|motorway_link|trunk_link|primary_link|secondary_link|tertiary_link|living_street|pedestrian|service)$"]({bb}););out tags geom;',
        "water": f'(way["natural"="water"]({bb});relation["natural"="water"]({bb});way["waterway"="riverbank"]({bb});way["natural"="coastline"]({bb});way["waterway"="river"]({bb}););out tags geom({bb});',
        "green": f'(way["leisure"~"^(park|pitch|garden)$"]({bb});relation["leisure"="park"]({bb});way["landuse"~"^(grass|recreation_ground)$"]({bb}););out tags geom;',
    }
    out = {"bbox": BBOX, "attribution": "Map data (c) OpenStreetMap contributors, ODbL 1.0"}
    # Buildings in four tiles (smaller answers, kinder to the servers), de-duplicated by id.
    s, w, n, e = BBOX
    mid_lat, mid_lon = (s + n) / 2, (w + e) / 2
    tiles = [(s, w, mid_lat, mid_lon), (s, mid_lon, mid_lat, e), (mid_lat, w, n, mid_lon), (mid_lat, mid_lon, n, e)]
    seen, blds = set(), []
    for t in tiles:
        tb = ",".join(str(v) for v in t)
        q = f'[out:json][timeout:180];(way["building"]({tb});relation["building"]({tb});way["building:part"]({tb});relation["building:part"]({tb}););out tags geom;'
        for el in query(q)["elements"]:
            if (el["type"], el["id"]) not in seen:
                seen.add((el["type"], el["id"]))
                blds.append(el)
        note(f"buildings tile {tb}: {len(blds)} so far")
        time.sleep(4)
    out["buildings"] = blds
    parts.pop("buildings")
    for name, body in parts.items():
        out[name] = query(f"[out:json][timeout:180];{body}")["elements"]
        note(f"{name}: {len(out[name])} elements")
        time.sleep(4)
    path = sys.argv[1] if len(sys.argv) > 1 else "data/osm/cleveland.json"
    __import__("os").makedirs(__import__("os").path.dirname(path) or ".", exist_ok=True)
    with open(path, "w") as f:
        json.dump(out, f, separators=(",", ":"))
    print(f"wrote {path}", file=sys.stderr)


if __name__ == "__main__":
    main()
