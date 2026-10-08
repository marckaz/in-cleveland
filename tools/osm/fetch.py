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
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
]


def query(q):
    data = urllib.parse.urlencode({"data": q}).encode()
    last = None
    for attempt in range(6):
        url = MIRRORS[attempt % len(MIRRORS)]
        try:
            req = urllib.request.Request(url, data=data, headers={"User-Agent": "in-cleveland map fetch (github.com/marckaz/in-cleveland)"})
            with urllib.request.urlopen(req, timeout=300) as r:
                return json.load(r)
        except Exception as e:  # busy servers answer 429/504; try the next one
            last = e
            print(f"  {url}: {e}; retrying", file=sys.stderr)
            time.sleep(10 + attempt * 10)
    raise SystemExit(f"Overpass failed: {last}")


def main():
    bb = ",".join(str(v) for v in BBOX)
    parts = {
        "buildings": f'(way["building"]({bb});relation["building"]({bb});way["building:part"]({bb});relation["building:part"]({bb}););out tags geom;',
        "streets": f'(way["highway"~"^(motorway|trunk|primary|secondary|tertiary|unclassified|residential|motorway_link|trunk_link|primary_link|secondary_link|tertiary_link|living_street|pedestrian|service)$"]({bb}););out tags geom;',
        "water": f'(way["natural"="water"]({bb});relation["natural"="water"]({bb});way["waterway"="riverbank"]({bb});way["natural"="coastline"]({bb});way["waterway"="river"]({bb}););out tags geom({bb});',
        "green": f'(way["leisure"~"^(park|pitch|garden)$"]({bb});relation["leisure"="park"]({bb});way["landuse"~"^(grass|recreation_ground)$"]({bb}););out tags geom;',
    }
    out = {"bbox": BBOX, "attribution": "Map data (c) OpenStreetMap contributors, ODbL 1.0"}
    for name, body in parts.items():
        print(f"fetching {name}", file=sys.stderr)
        out[name] = query(f"[out:json][timeout:180];{body}")["elements"]
        print(f"  {len(out[name])} elements", file=sys.stderr)
        time.sleep(5)
    path = sys.argv[1] if len(sys.argv) > 1 else "data/osm/cleveland.json"
    with open(path, "w") as f:
        json.dump(out, f, separators=(",", ":"))
    print(f"wrote {path}", file=sys.stderr)


if __name__ == "__main__":
    main()
