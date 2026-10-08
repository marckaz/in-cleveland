#!/usr/bin/env python3
"""Bake data/osm/cleveland.json (from fetch.py) into crates/faith_move/data/downtown.bin,
the Downtown map: real downtown Cleveland at true scale.

    pip install mapbox-earcut shapely numpy
    python3 tools/osm/bake.py [data/osm/cleveland.json] [crates/faith_move/data/downtown.bin]

What it makes:
- every building as a prism from its footprint, at its OpenStreetMap height (or `levels` x
  3.5 m, or a guess by building type where OSM has neither). Buildings drawn as 3D parts
  (Terminal Tower, Key Tower, ...) use their parts.
- streets, water and parks as flat shapes on the ground.
- a parkour layer on top, since real streets are too wide to jump: ziplines from roof to
  lower roof across the gaps, ladders up from the street onto low roofs.
- spawn points at landmarks.

The format is read by `crates/faith_move/src/downtown.rs`. Map data (c) OpenStreetMap
contributors, ODbL 1.0.
"""
import json
import math
import re
import struct
import sys
from collections import defaultdict

import mapbox_earcut as earcut
import numpy as np
from shapely import STRtree
from shapely.geometry import LineString, MultiPolygon, Point, Polygon, box
from shapely.ops import nearest_points, unary_union
from shapely.validation import make_valid

SRC = sys.argv[1] if len(sys.argv) > 1 else "data/osm/downtown.json"
DST = sys.argv[2] if len(sys.argv) > 2 else "crates/faith_move/data/downtown.bin"

# The origin: the middle of Public Square. x is east, z is south (so north is -z), in metres.
LAT0, LON0 = 41.49950, -81.69370
M_LAT = 111_132.0
M_LON = 111_320.0 * math.cos(math.radians(LAT0))


def proj(lat, lon):
    return ((lon - LON0) * M_LON, -(lat - LAT0) * M_LAT)


def unproj_bbox(bb):
    s, w, n, e = bb
    x0, z1 = proj(s, w)
    x1, z0 = proj(n, e)
    return x0, z0, x1, z1


# ------------------------------------------------------------------ parsing

def parse_len(v):
    """'44'', '25.3 m', '120', '12 ft' -> metres."""
    if v is None:
        return None
    s = str(v).strip().lower().replace(",", ".")
    m = re.match(r"^([0-9.]+)\s*('|ft|feet)$", s)
    if m:
        return float(m.group(1)) * 0.3048
    m = re.match(r"^([0-9.]+)\s*'\s*([0-9.]+)\s*\"?$", s)
    if m:
        return float(m.group(1)) * 0.3048 + float(m.group(2)) * 0.0254
    m = re.match(r"^([0-9.]+)\s*m?$", s)
    if m:
        try:
            return float(m.group(1))
        except ValueError:
            return None
    return None


def parse_num(v):
    try:
        return float(str(v).split(";")[0].strip())
    except (TypeError, ValueError):
        return None


def ring(geom):
    # Geometry clipped to the box (`out geom(bbox)`) has nulls for the nodes outside it.
    return [proj(p["lat"], p["lon"]) for p in geom if p]


def polygons_of(el):
    """Shapely polygons of a way or multipolygon relation (outer rings with their holes)."""
    if el["type"] == "way":
        g = el.get("geometry")
        if not g or len(g) < 4:
            return []
        try:
            p = make_valid(Polygon(ring(g)))
        except Exception:
            return []
        return [q for q in explode(p)]
    outers, inners = [], []
    for m in el.get("members", []):
        if m.get("type") != "way" or not m.get("geometry"):
            continue
        (inners if m.get("role") == "inner" else outers).append(ring(m["geometry"]))
    # Join ring pieces end to end.
    def join(pieces):
        rings = []
        pieces = [list(p) for p in pieces]
        while pieces:
            cur = pieces.pop(0)
            changed = True
            while cur[0] != cur[-1] and changed:
                changed = False
                for i, p in enumerate(pieces):
                    if p[0] == cur[-1]:
                        cur += p[1:]
                    elif p[-1] == cur[-1]:
                        cur += p[::-1][1:]
                    elif p[-1] == cur[0]:
                        cur = p + cur[1:]
                    elif p[0] == cur[0]:
                        cur = p[::-1] + cur[1:]
                    else:
                        continue
                    pieces.pop(i)
                    changed = True
                    break
            if len(cur) >= 4:
                rings.append(cur)
        return rings
    out = []
    holes = [Polygon(r) for r in join(inners)]
    for r in join(outers):
        try:
            p = make_valid(Polygon(r))
        except Exception:
            continue
        for q in explode(p):
            hs = [h for h in holes if q.contains(h.representative_point())]
            if hs:
                q = make_valid(q.difference(unary_union(hs)))
            out += list(explode(q))
    return out


def explode(g):
    if g.is_empty:
        return []
    if isinstance(g, Polygon):
        return [g]
    if isinstance(g, MultiPolygon):
        return list(g.geoms)
    if hasattr(g, "geoms"):
        return [q for h in g.geoms for q in explode(h)]
    return []


# ------------------------------------------------------------------ heights

CORE = Polygon([(-450, 420), (-150, -900), (1400, -900), (1500, 650), (300, 700)])  # downtown east of the river


def height_of(t, poly):
    """(base, top, guessed) for a building or part."""
    h = parse_len(t.get("height"))
    lv = parse_num(t.get("building:levels"))
    roof_lv = parse_num(t.get("roof:levels")) or 0.0
    base = parse_len(t.get("min_height"))
    if base is None:
        ml = parse_num(t.get("building:min_level"))
        base = ml * 3.5 if ml else 0.0
    guessed = False
    if h is None and lv is not None:
        h = lv * 3.5 + roof_lv * 2.0 + (1.0 if lv > 2 else 0.0)
    if h is None:
        guessed = True
        b = t.get("building") or t.get("building:part") or "yes"
        area = poly.area
        if b in ("house", "detached", "semidetached_house", "terrace", "residential"):
            h = 8.0
        elif b in ("shed", "kiosk", "roof", "carport", "service", "hut"):
            h = 3.5
        elif b == "garage" or b == "garages":
            h = 3.5 if area < 120 else 14.0
        elif b in ("parking",):
            h = 14.0
        elif b in ("church", "cathedral", "chapel"):
            h = 16.0
        elif b in ("industrial", "warehouse", "hangar", "manufacture"):
            h = 9.0
        elif b in ("stadium",):
            h = 35.0
        elif b in ("ship",):
            h = 8.0
        elif b in ("silo",):
            h = 25.0
        else:
            in_core = CORE.contains(poly.centroid)
            h = (14.0 if area < 400 else 20.0 if area < 2500 else 26.0) if in_core else (7.0 if area < 300 else 9.0)
    h = max(h, base + 2.5)
    return base, h, guessed


# ------------------------------------------------------------------ the map

LOOK_WALL, LOOK_LANDMARK, LOOK_GLASS = 0, 1, 2
FLAT_ROAD, FLAT_WATER, FLAT_GREEN, FLAT_WALK = 0, 1, 2, 3

LANDMARKS = {
    "Terminal Tower", "Key Tower", "200 Public Square", "West Side Market", "Rocket Arena",
    "Cleveland City Hall", "Great Lakes Science Center", "Old Stone Church", "Cleveland Arcade",
    "Federal Reserve Bank of Cleveland", "Cleveland Public Auditorium", "Hanna Building",
    "Rock and Roll Hall of Fame", "Rock & Roll Hall of Fame", "Progressive Field", "Huntington Bank Field",
    "FirstEnergy Stadium", "Cleveland Browns Stadium", "Cathedral of Saint John the Evangelist", "May Company Building",
}


def simplify(p, tol=0.25):
    q = p.simplify(tol, preserve_topology=True)
    q = make_valid(q)
    return [r for r in explode(q) if r.area > 6.0]


def tri_poly(p):
    """Earcut a polygon with holes: (rings, triangle indices into the concatenated rings)."""
    p = Polygon(p.exterior.coords, [i.coords for i in p.interiors])
    if not p.exterior.is_ccw:
        p = Polygon(list(p.exterior.coords)[::-1], [list(i.coords) for i in p.interiors])
    rings = [list(p.exterior.coords)[:-1]] + [list(i.coords)[:-1] for i in p.interiors]
    rings = [r for r in rings if len(r) >= 3]
    verts = np.array([v for r in rings for v in r], dtype=np.float64).reshape(-1, 2)
    ends = np.cumsum([len(r) for r in rings]).astype(np.uint32)
    idx = earcut.triangulate_float64(verts, ends)
    return rings, idx.reshape(-1, 3)


def main():
    src = json.load(open(SRC))
    x0, z0, x1, z1 = unproj_bbox(src["bbox"])
    area_box = box(x0, z0, x1, z1)

    # ---- buildings, with 3D parts standing in for the outlines they fill
    parts, outlines = [], []
    for el in src["buildings"]:
        t = el.get("tags", {})
        for p in polygons_of(el):
            if p.area < 6:
                continue
            (parts if "building:part" in t and "building" not in t else outlines).append((t, p))
    part_tree = STRtree([p for _, p in parts]) if parts else None
    kept = []
    for t, p in outlines:
        covered = 0.0
        inside = []
        if part_tree is not None:
            for i in part_tree.query(p):
                q = parts[i][1]
                if p.contains(q.representative_point()):
                    covered += p.intersection(q).area
                    inside.append(i)
        # Parts are usually unnamed: they take their building's name (Key Tower's tower is a part).
        if t.get("name"):
            for i in inside:
                if not parts[i][0].get("name"):
                    parts[i] = ({**parts[i][0], "name": t["name"]}, parts[i][1])
        if covered < 0.6 * p.area:
            kept.append((t, p))
    blds = []
    for t, p in kept + parts:
        if t.get("building") in ("construction", "roof") or t.get("building:part") == "roof":
            continue
        base, top, guessed = height_of(t, p)
        name = t.get("name", "")
        look = LOOK_LANDMARK if name in LANDMARKS else (LOOK_GLASS if top > 60 else LOOK_WALL)
        for q in simplify(p):
            blds.append({"poly": q, "base": base, "top": top, "look": look, "name": name, "guessed": guessed})
    print(f"buildings: {len(blds)} prisms ({sum(b['guessed'] for b in blds)} with guessed heights)")

    # ---- flat shapes: streets, water, parks
    widths = {"motorway": 18, "trunk": 16, "primary": 14, "secondary": 12, "tertiary": 10, "unclassified": 8,
              "residential": 8, "living_street": 6, "pedestrian": 6, "service": 4.5}
    roads = []
    for el in src["streets"]:
        g = el.get("geometry")
        t = el.get("tags", {})
        if not g or len(g) < 2 or t.get("tunnel") == "yes" or t.get("area") == "yes":
            continue
        hw = t.get("highway", "").replace("_link", "")
        w = widths.get(hw, 6) * (0.6 if t.get("highway", "").endswith("_link") else 1.0)
        pts = ring(g)
        if len(pts) >= 2:
            roads.append(LineString(pts).buffer(w / 2, cap_style=2, join_style=2))
    road_area = unary_union(roads).intersection(area_box) if roads else Polygon()
    water = []
    for el in src["water"]:
        t = el.get("tags", {})
        if t.get("natural") == "coastline":
            continue
        if t.get("waterway") == "river" and el["type"] == "way" and el.get("geometry") and len(ring(el["geometry"])) >= 2:
            water.append(LineString(ring(el["geometry"])).buffer(35))
            continue
        water += polygons_of(el)
    # Lake Erie: everything on the water side of the coastline (OSM keeps the water on the
    # right of a coastline way's direction).
    coast = [LineString(ring(el["geometry"])) for el in src["water"]
             if el.get("tags", {}).get("natural") == "coastline" and el.get("geometry") and len(ring(el["geometry"])) >= 2]
    if coast:
        from shapely.ops import polygonize, split
        pieces = list(polygonize(unary_union([area_box.exterior] + coast)))
        for piece in pieces:
            c = piece.representative_point()
            # Which side of the nearest coastline segment is it on?
            best = min(coast, key=lambda l: l.distance(c))
            d = best.project(c)
            a = best.interpolate(max(0.0, d - 1.0))
            b = best.interpolate(min(best.length, d + 1.0))
            cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
            # x east, z south: z flips handedness, so "right of the way" is cross > 0 here.
            if cross > 0:
                water.append(piece)
    water_area = unary_union(water).intersection(area_box) if water else Polygon()
    green = []
    for el in src["green"]:
        green += polygons_of(el)
    green_area = unary_union(green).intersection(area_box).difference(water_area) if green else Polygon()
    road_area = road_area.difference(water_area)
    flats = []
    for kind, shape in [(FLAT_WATER, water_area), (FLAT_GREEN, green_area), (FLAT_ROAD, road_area)]:
        for p in explode(make_valid(shape.simplify(0.4))):
            if p.area > 4:
                flats.append((kind, p))
    print(f"flat shapes: {len(flats)}")

    # ---- the parkour layer
    tree = STRtree([b["poly"] for b in blds])

    def roof_at(x, z):
        """Highest roof under (x, z), or 0 (the street)."""
        pt = Point(x, z)
        best = 0.0
        for i in tree.query(pt):
            if blds[i]["poly"].contains(pt):
                best = max(best, blds[i]["top"])
        return best

    def clear_line(a, b, skip):
        """Nothing stands within reach under the cable a -> b, between leaving the first roof
        and arriving over the last (the two ends sit over their own roofs)."""
        from shapely.ops import substring
        full = LineString([(a[0], a[2]), (b[0], b[2])])
        L = full.length
        line = substring(full, 3.0, L - 5.0)
        if line.is_empty or line.length < 1.0:
            return False
        # Her body is half a metre wide, and swings: keep 1.5 m clear either side.
        corridor = line.buffer(1.5, cap_style=2)
        for i in tree.query(corridor):
            q = blds[i]["poly"]
            if not q.intersects(corridor):
                continue
            seg = q.intersection(corridor)
            pts = [seg.centroid]
            for g in explode(seg):
                pts += [Point(c) for c in g.exterior.coords]
            for pt in pts:
                s = full.project(pt) / max(L, 1e-6)
                y = a[1] + (b[1] - a[1]) * s
                # Feet hang 1.8 m under the cable: the two end roofs just need to pass under
                # them, anything else gets more room.
                if blds[i]["top"] > y - (1.9 if i in skip else 2.3):
                    return False
        return True

    zips = []
    starts = defaultdict(int)
    ends = defaultdict(int)
    # Candidate roofs: flat-topped places you can stand, 8 to 70 m up.
    cand = [i for i, b in enumerate(blds) if 8.0 <= b["top"] <= 70.0 and b["base"] == 0.0 and b["poly"].area > 80]
    for i in cand:
        a = blds[i]
        near = tree.query(a["poly"].buffer(45.0))
        options = []
        for j in near:
            if j == i or j not in set(cand):
                continue
            b = blds[j]
            drop = a["top"] - b["top"]
            if not (2.5 <= drop <= 22.0):
                continue
            pa, pb = nearest_points(a["poly"], b["poly"])
            gap = pa.distance(pb)
            if not (9.0 <= gap <= 42.0):
                continue
            # Gentle enough that, hanging 1.8 m under the cable, you clear the edge of the roof
            # you set off from (the ride looks 6 m ahead for things to brace against).
            if drop / (gap + 6.0) > 0.28:
                continue
            options.append((gap, j, pa, pb))
        options.sort(key=lambda o: o[0])
        for gap, j, pa, pb in options[:2]:
            if starts[i] >= 2 or ends[j] >= 2:
                continue
            b = blds[j]
            # Step each end 2.5 m in from the roof edge, along the cable.
            dx, dz = pb.x - pa.x, pb.y - pa.y
            n = math.hypot(dx, dz)
            ux, uz = dx / n, dz / n
            sa = (pa.x - ux * 2.5, pa.y - uz * 2.5)
            sb = (pb.x + ux * 3.5, pb.y + uz * 3.5)
            if not a["poly"].buffer(-0.6).contains(Point(sa)) or not b["poly"].buffer(-0.6).contains(Point(sb)):
                continue
            if roof_at(*sa) > a["top"] + 0.5 or roof_at(*sb) > b["top"] + 0.5:
                continue
            # A run-up behind the mast: 7 m of this roof, nothing taller on it.
            runway = [(sa[0] - ux * d, sa[1] - uz * d) for d in (1.0, 3.0, 5.0, 7.0)]
            if not all(a["poly"].buffer(-0.5).contains(Point(p)) and abs(roof_at(*p) - a["top"]) < 0.3 for p in runway):
                continue
            # And room to land: 4 m of the far roof past the end.
            landing = [(sb[0] + ux * d, sb[1] + uz * d) for d in (1.0, 2.5, 4.0)]
            if not all(abs(roof_at(*p) - b["top"]) < 0.3 for p in landing):
                continue
            A = (sa[0], a["top"] + 2.8, sa[1])
            B = (sb[0], b["top"] + 2.6, sb[1])
            if not clear_line(A, B, {i, j}):
                continue
            # Not on top of another cable.
            if any(math.dist((A[0], A[2]), (z[0][0], z[0][2])) < 6 for z in zips):
                continue
            zips.append((A, B))
            starts[i] += 1
            ends[j] += 1
    print(f"ziplines: {len(zips)}")

    # ---- every roof reachable: ladders up from the street, or up from a neighbouring roof you
    # can already get to, chained until nothing more connects.
    CLIMB_FREE = 2.8  # a wallclimb gets you up this much without help
    DROP_SAFE = 8.0  # a drop you can take (roll it past 5.3 m)
    JUMP_GAP = 2.5  # a gap you can jump
    by_top = {}

    def owner_at(x, z):
        """The building whose roof you'd stand on at (x, z): the highest one there, or None."""
        pt = Point(x, z)
        best = None
        for i in tree.query(pt):
            if blds[i]["poly"].contains(pt) and (best is None or blds[i]["top"] > blds[best]["top"]):
                best = i
        return best

    def standable(i):
        b = blds[i]
        return b["base"] == 0.0 and b["poly"].area >= 25.0

    def edges(i):
        b = blds[i]
        coords = list(b["poly"].exterior.coords)
        ccw = b["poly"].exterior.is_ccw
        for (ax, az), (bx, bz) in zip(coords, coords[1:]):
            L = math.hypot(bx - ax, bz - az)
            if L < 2.5:
                continue
            ex, ez = (bx - ax) / L, (bz - az) / L
            nx, nz = (ez, -ex) if ccw else (-ez, ex)
            yield (ax + bx) / 2, (az + bz) / 2, nx, nz, L

    want = [i for i in range(len(blds)) if standable(i)]
    reached = set()
    ladders = []
    # Ziplines: reaching the start roof reaches the end roof.
    zip_from = defaultdict(list)
    for A, B in zips:
        ia, ib = owner_at(A[0], A[2]), owner_at(B[0], B[2])
        if ia is not None and ib is not None:
            zip_from[ia].append(ib)
    neighbours = {i: [j for j in tree.query(blds[i]["poly"].buffer(JUMP_GAP)) if j != i and standable(j)] for i in want}

    def free_way_up(i):
        """Reached without a ladder: a low roof off the street, or from a reached neighbour by a
        short climb, a jump across or a safe drop."""
        b = blds[i]
        if b["top"] <= CLIMB_FREE:
            return True
        for j in neighbours[i]:
            if j in reached and -CLIMB_FREE <= blds[j]["top"] - b["top"] <= DROP_SAFE:
                return True
        return False

    def ladder_up(i, max_climb):
        """The best wall for a ladder onto roof i, from the street or a reached roof."""
        b = blds[i]
        best = None
        for mx, mz, nx, nz, L in edges(i):
            # Climb out onto this roof (not a taller one standing on it).
            ex, ez = mx - nx * 1.0, mz - nz * 1.0
            if owner_at(ex, ez) != i:
                continue
            # Something flat to stand on in front: the street, or a reached roof.
            f1, f2 = owner_at(mx + nx * 1.2, mz + nz * 1.2), owner_at(mx + nx * 3.0, mz + nz * 3.0)
            if f1 != f2:
                continue
            if f1 is None:
                base_h = 0.0
            elif f1 in reached:
                base_h = blds[f1]["top"]
            else:
                continue
            climb = b["top"] - base_h
            if not (CLIMB_FREE < climb <= max_climb):
                continue
            score = climb - 0.2 * min(L, 10.0) + (0.0 if f1 is None else 1.0)
            if best is None or score < best[0]:
                best = (score, (mx, base_h, mz), (nx, nz))
        return best

    # Short ladders first; long ones only where nothing shorter connects (the skyscrapers get
    # service ladders all the way up).
    for max_climb in (24.0, 60.0, 400.0):
        changed = True
        while changed:
            changed = False
            for i in sorted(want, key=lambda i: blds[i]["top"]):
                if i in reached:
                    continue
                if free_way_up(i):
                    reached.add(i)
                    changed = True
                else:
                    up = ladder_up(i, max_climb)
                    if up:
                        _, base, n = up
                        ladders.append((base, blds[i]["top"], n))
                        reached.add(i)
                        changed = True
                if i in reached:
                    for j in zip_from.get(i, []):
                        if j not in reached:
                            reached.add(j)
                            changed = True
    lost = [i for i in want if i not in reached]
    print(f"ladders: {len(ladders)}; roofs reachable: {len(reached)}/{len(want)}")
    if lost:
        print("  not reachable (tallest): " + ", ".join(f"{blds[i]['name'] or '?'} {blds[i]['top']:.0f} m" for i in sorted(lost, key=lambda i: -blds[i]['top'])[:8]))
    reach_stats = (len(reached), len(want))

    # ---- spawn points: street level at landmarks, and a few roofs
    def spot(name, lat, lon, yaw_deg, on_roof=False):
        x, z = proj(lat, lon)
        y = roof_at(x, z) if on_roof else 0.0
        def clear(px, pz):
            area = Point(px, pz).buffer(1.6)
            return not any(blds[i]["poly"].intersects(area) for i in tree.query(area))
        if not on_roof and not clear(x, z):
            # Step out to the nearest open ground, clear of every wall.
            for r in range(1, 80):
                found = False
                for k in range(24):
                    ang = k * math.pi / 12
                    if clear(x + r * math.cos(ang), z + r * math.sin(ang)):
                        x, z = x + r * math.cos(ang), z + r * math.sin(ang)
                        found = True
                        break
                if found:
                    break
        if not on_roof:
            # Face the most open way (down the street), nearest the asked-for heading.
            def open_run(ang):
                dx, dz = -math.sin(ang), -math.cos(ang)  # yaw 0 faces -z
                for d in range(2, 160, 2):
                    if roof_at(x + dx * d, z + dz * d) > 0.0:
                        return d
                return 160
            want_yaw = math.radians(yaw_deg)
            best = max((k * math.pi / 8 for k in range(16)), key=lambda a: open_run(a) - 20 * abs(math.remainder(a - want_yaw, 2 * math.pi)))
            return (name, (x, y, z), best)
        return (name, (x, y, z), math.radians(yaw_deg))

    def roof_spot(name, building, yaw_deg):
        """On top of a named building, in the middle of its roof."""
        cands = [b for b in blds if b["name"] == building]
        if not cands:
            return None
        # The highest roof piece with room to stand: not a spire, not a pit walled in by the
        # taller pieces of the crown (open on at least half its sides).
        for b in sorted(cands, key=lambda b: -b["top"]):
            if b["poly"].area < 60:
                continue
            p = b["poly"].buffer(-2.0)
            c = (p if not p.is_empty else b["poly"]).representative_point()
            if abs(roof_at(c.x, c.y) - b["top"]) > 0.3:
                continue
            open_sides = sum(roof_at(c.x + 6 * math.cos(k * math.pi / 4), c.y + 6 * math.sin(k * math.pi / 4)) <= b["top"] + 0.3 for k in range(8))
            if open_sides >= 3:
                return (name, (c.x, b["top"], c.y), math.radians(yaw_deg))
        return None

    # Yaw: 0 faces north (-z); positive turns left (west).
    spots = [
        spot("Public Square", 41.49950, -81.69370, 0),
        spot("West Side Market", 41.48455, -81.70300, -60),
        roof_spot("Tower City roof", "Hotel Cleveland, Autograph Collection", 0) or spot("Tower City", 41.49800, -81.69420, 0),
        spot("East 4th St", 41.49905, -81.68990, 0),
        spot("Playhouse Square", 41.50130, -81.68090, 90),
        spot("Progressive Field", 41.49580, -81.68620, 90),
        spot("Warehouse District", 41.49960, -81.70000, 0),
        spot("Rock Hall", 41.50760, -81.69530, 180),
        spot("Flats East Bank", 41.49640, -81.70420, 0),
        roof_spot("Key Tower roof", "Key Tower", 180) or roof_spot("200 Public Square roof", "200 Public Square", 0)
        or spot("Key Tower", 41.50060, -81.69310, 180),
    ]
    for n, p, _ in spots:
        print(f"  spot {n}: {p[0]:.0f}, {p[1]:.0f}, {p[2]:.0f}")

    # ---- objects: rooftop clutter, parked cars, dumpsters, and the street furniture OSM maps
    import random
    rng = random.Random(1796)
    props = []  # (kind, (cx, base_y, cz), (half_x, height, half_z), yaw, (r, g, b), solid)
    P_AC, P_VENT, P_TANK, P_HUT, P_SKY, P_CAR, P_CABIN, P_DUMP, P_TRUNK, P_CANOPY, P_POLE, P_LAMP, P_BENCH, P_SHELTER, P_HYDRANT, P_BIN = range(16)
    keep_clear = []  # (x, z, radius) spots no object may stand on

    for (bx, by, bz), top, (nx, nz) in ladders:
        keep_clear.append((bx - nx * 1.2, bz - nz * 1.2, 2.6))  # where you climb out on top
        keep_clear.append((bx + nx * 1.0, bz + nz * 1.0, 1.6))  # the foot of it
    for A, B in zips:
        dx, dz = B[0] - A[0], B[2] - A[2]
        n = math.hypot(dx, dz) or 1.0
        for d in range(-8, 0):  # the run-up behind the mast
            keep_clear.append((A[0] + dx / n * d, A[2] + dz / n * d, 1.8))
        for d in range(0, 6):  # the landing
            keep_clear.append((B[0] + dx / n * d, B[2] + dz / n * d, 2.2))
    for _, (x, y, z), _ in [sp for sp in spots if sp]:
        keep_clear.append((x, z, 6.0))
    clear_tree = STRtree([Point(x, z).buffer(r) for x, z, r in keep_clear])

    placed = []  # footprints of what's been put down, so nothing overlaps

    def box_poly(cx, cz, hx, hz, yaw):
        c, s_ = math.cos(yaw), math.sin(yaw)
        pts = [(cx + c * x + s_ * z, cz - s_ * x + c * z) for x, z in ((-hx, -hz), (hx, -hz), (hx, hz), (-hx, hz))]
        return Polygon(pts)

    def free(poly):
        if any(True for _ in clear_tree.query(poly, predicate="intersects")):
            return False
        return not any(q.intersects(poly) for q in placed[-400:] if q.distance(poly) < 0.8)

    def put(kind, cx, base, cz, hx, h, hz, yaw, rgb, solid=True, check=True, mark=True):
        poly = box_poly(cx, cz, hx, hz, yaw)
        if check and not free(poly.buffer(0.5)):
            return False
        props.append((kind, (cx, base, cz), (hx, h, hz), yaw, rgb, solid))
        if mark:
            placed.append(poly)
        return True

    def main_yaw(poly):
        """Yaw of the footprint's longest edge (so things on the roof line up with it)."""
        coords = list(poly.exterior.coords)
        e = max(zip(coords, coords[1:]), key=lambda e: math.dist(e[0], e[1]))
        return math.atan2(-(e[1][1] - e[0][1]), e[1][0] - e[0][0])

    # Rooftops: AC units, vents, a hut over the stairs, water tanks on the older blocks.
    for i in want:
        b = blds[i]
        if b["top"] < 5.0 or b["poly"].area < 90:
            continue
        inner = b["poly"].buffer(-2.2)
        if inner.is_empty:
            continue
        yaw = main_yaw(b["poly"])
        minx, minz, maxx, maxz = inner.bounds
        n = int(min(9, max(1, b["poly"].area / 220)))
        kinds = [P_HUT] + [P_AC] * 5 + [P_VENT] * 3 + [P_SKY] * 2 + ([P_TANK] * 2 if 10 < b["top"] < 45 else [])
        tries = 0
        made = 0
        while made < n and tries < n * 8:
            tries += 1
            x, z = rng.uniform(minx, maxx), rng.uniform(minz, maxz)
            if not inner.contains(Point(x, z)) or owner_at(x, z) != i:
                continue
            k = P_HUT if made == 0 and b["poly"].area > 300 else rng.choice(kinds)
            dims = {P_AC: (0.9, 1.35, 0.65), P_VENT: (0.4, 1.0, 0.4), P_TANK: (1.6, 4.2, 1.6), P_HUT: (1.6, 2.6, 1.3), P_SKY: (1.3, 0.7, 0.8)}[k]
            col = {P_AC: (0.70, 0.74, 0.78), P_VENT: (0.62, 0.65, 0.68), P_TANK: (0.55, 0.42, 0.30), P_HUT: (0.86, 0.84, 0.80), P_SKY: (0.62, 0.76, 0.86)}[k]
            hx, h, hz = dims
            if not inner.contains(box_poly(x, z, hx, hz, yaw)):
                continue
            if put(k, x, b["top"], z, hx, h, hz, yaw, col):
                made += 1

    # Streets: parked cars along the kerbs, both sides.
    car_colours = [(0.82, 0.10, 0.08), (0.12, 0.22, 0.45), (0.92, 0.92, 0.93), (0.12, 0.12, 0.13), (0.60, 0.62, 0.65), (0.15, 0.35, 0.22), (0.85, 0.65, 0.15), (0.35, 0.36, 0.40)]
    for el in src["streets"]:
        t = el.get("tags", {})
        hw = t.get("highway", "")
        if hw not in ("residential", "tertiary", "secondary", "unclassified", "primary", "living_street") or t.get("bridge") == "yes" or t.get("oneway") == "yes" and hw == "primary":
            continue
        pts = ring(el.get("geometry") or [])
        if len(pts) < 2:
            continue
        line = LineString(pts)
        w = widths.get(hw, 8)
        d = 12.0
        while d < line.length - 12.0:
            a = line.interpolate(d)
            bpt = line.interpolate(d + 0.5)
            ux, uz = bpt.x - a.x, bpt.y - a.y
            n = math.hypot(ux, uz) or 1.0
            ux, uz = ux / n, uz / n
            yaw = math.atan2(-uz, ux)
            for side in (-1, 1):
                if rng.random() > 0.26:
                    continue
                off = w / 2 - 1.2
                cx, cz = a.x - uz * off * side, a.y + ux * off * side
                if owner_at(cx, cz) is not None or not road_area.contains(Point(cx, cz)):
                    continue
                col = rng.choice(car_colours)
                if put(P_CAR, cx, 0.0, cz, 2.2, 1.0, 0.9, yaw, col):
                    put(P_CABIN, cx - ux * 0.2, 1.0, cz - uz * 0.2, 1.2, 0.5, 0.8, yaw, tuple(min(1.0, c * 0.85 + 0.05) for c in col), check=False, mark=False)
            d += rng.uniform(6.5, 9.0)

    # Dumpsters against walls by the alleys and service roads: a step up toward the roofs.
    for el in src["streets"]:
        if el.get("tags", {}).get("highway") != "service":
            continue
        pts = ring(el.get("geometry") or [])
        if len(pts) < 2:
            continue
        line = LineString(pts)
        d = 4.0
        while d < line.length - 4.0:
            a = line.interpolate(d)
            for i in tree.query(a.buffer(7.0)):
                if not standable(i) or rng.random() > 0.15:
                    continue
                for mx, mz, nx, nz, L in edges(i):
                    if L < 4 or Point(mx, mz).distance(a) > 7.0:
                        continue
                    cx, cz = mx + nx * 0.75, mz + nz * 0.75
                    if owner_at(cx, cz) is not None:
                        continue
                    put(P_DUMP, cx, 0.0, cz, 0.95, 1.3, 0.6, math.atan2(-nz, nx) + math.pi / 2, (0.16, 0.36, 0.22))
                    break
            d += 9.0

    # Street furniture where OpenStreetMap has it: trees, lamps, benches, bus shelters, hydrants.
    furniture = src.get("furniture", [])
    for el in furniture:
        t = el.get("tags", {})
        nodes = [(el["lat"], el["lon"])] if "lat" in el else [(p["lat"], p["lon"]) for p in (el.get("geometry") or []) if p][::3]
        for lat, lon in nodes:
            x, z = proj(lat, lon)
            if owner_at(x, z) is not None or not (x0 < x < x1 and z0 < z < z1):
                continue
            yaw = rng.uniform(0, math.pi)
            if t.get("natural") in ("tree", "tree_row"):
                hgt = rng.uniform(5.0, 8.5)
                if put(P_TRUNK, x, 0.0, z, 0.16, hgt * 0.45, 0.16, yaw, (0.36, 0.26, 0.18)):
                    r = rng.uniform(1.6, 2.4)
                    props.append((P_CANOPY, (x, hgt * 0.38, z), (r, hgt * 0.62, r), yaw, (0.30 + rng.uniform(-0.04, 0.04), 0.52 + rng.uniform(-0.06, 0.06), 0.26), False))
            elif t.get("highway") == "street_lamp":
                if put(P_POLE, x, 0.0, z, 0.08, 6.0, 0.08, yaw, (0.20, 0.21, 0.23)):
                    props.append((P_LAMP, (x, 5.75, z), (0.28, 0.25, 0.28), yaw, (1.0, 0.9, 0.7), False))
            elif t.get("amenity") == "bench":
                put(P_BENCH, x, 0.0, z, 0.9, 0.48, 0.28, yaw, (0.42, 0.30, 0.20))
            elif t.get("highway") == "bus_stop":
                if put(P_SHELTER, x, 0.0, z, 1.6, 2.5, 0.08, yaw, (0.55, 0.62, 0.68)):
                    props.append((P_SHELTER, (x - math.sin(yaw) * 0.7, 2.5, z - math.cos(yaw) * 0.7), (1.7, 0.1, 0.8), yaw, (0.55, 0.62, 0.68), True))
            elif t.get("emergency") == "fire_hydrant":
                put(P_HYDRANT, x, 0.0, z, 0.15, 0.8, 0.15, 0.0, (0.86, 0.14, 0.08))
            elif t.get("amenity") == "waste_basket":
                put(P_BIN, x, 0.0, z, 0.25, 0.9, 0.25, yaw, (0.15, 0.18, 0.16))
    # Street lamps along the main streets (OSM maps only a handful downtown), staggered sides.
    for el in src["streets"]:
        hw = el.get("tags", {}).get("highway", "")
        if hw not in ("primary", "secondary", "tertiary", "trunk"):
            continue
        pts = ring(el.get("geometry") or [])
        if len(pts) < 2:
            continue
        line = LineString(pts)
        w = widths.get(hw, 10)
        d, side = 10.0, 1
        while d < line.length - 6.0:
            a = line.interpolate(d)
            bpt = line.interpolate(d + 0.5)
            ux, uz = bpt.x - a.x, bpt.y - a.y
            n = math.hypot(ux, uz) or 1.0
            ux, uz = ux / n, uz / n
            off = w / 2 + 0.9
            cx, cz = a.x - uz * off * side, a.y + ux * off * side
            if owner_at(cx, cz) is None and put(P_POLE, cx, 0.0, cz, 0.08, 6.5, 0.08, 0.0, (0.20, 0.21, 0.23)):
                # The arm reaches back out over the road.
                hx, hz = cx + uz * side * 1.0, cz - ux * side * 1.0
                props.append((P_LAMP, (hx, 6.25, hz), (0.32, 0.22, 0.32), math.atan2(-uz, ux), (1.0, 0.9, 0.7), False))
            d += 30.0
            side = -side

    counts = defaultdict(int)
    for pr in props:
        counts[pr[0]] += 1
    print(f"objects: {len(props)} (roof {sum(counts[k] for k in (P_AC, P_VENT, P_TANK, P_HUT, P_SKY))}, cars {counts[P_CAR]}, dumpsters {counts[P_DUMP]}, trees {counts[P_TRUNK]}, lamps {counts[P_POLE]}, benches {counts[P_BENCH]}, shelters {counts[P_SHELTER]})")

    # ---- write
    def q(v):
        return max(-32767, min(32767, int(round(v * 10))))

    out = bytearray(b"CLE3")
    out += struct.pack("<4f", x0, z0, x1, z1)
    out += struct.pack("<I", len(blds))
    for b in blds:
        rings, tris = tri_poly(b["poly"])
        name = b["name"].encode()[:255]
        out += struct.pack("<BffB", b["look"], b["base"], b["top"], len(name)) + name
        out += struct.pack("<H", len(rings))
        for r in rings:
            out += struct.pack("<H", len(r))
            for x, z in r:
                out += struct.pack("<hh", q(x), q(z))
        out += struct.pack("<I", len(tris))
        for t in tris:
            out += struct.pack("<3H", *[int(v) for v in t])
    out += struct.pack("<I", len(flats))
    for kind, p in flats:
        rings, tris = tri_poly(p)
        out += struct.pack("<BH", kind, len(rings))
        for r in rings:
            out += struct.pack("<I", len(r))
            for x, z in r:
                out += struct.pack("<hh", q(x), q(z))
        out += struct.pack("<I", len(tris))
        for t in tris:
            out += struct.pack("<3I", *[int(v) for v in t])
    out += struct.pack("<I", len(zips))
    for A, B in zips:
        out += struct.pack("<6f", *A, *B)
    out += struct.pack("<I", len(ladders))
    for base, top, (nx, nz) in ladders:
        out += struct.pack("<6f", base[0], base[1], base[2], top, nx, nz)
    out += struct.pack("<I", len(props))
    for kind, (cx, cy, cz), (hx, h, hz), yaw, (r, g, b_), solid in props:
        out += struct.pack("<BB7f3B", kind, 1 if solid else 0, cx, cy, cz, hx, h, hz, yaw, int(r * 255), int(g * 255), int(b_ * 255))
    out += struct.pack("<II", *reach_stats)
    out += struct.pack("<I", len(spots))
    for name, p, yaw in spots:
        nb = name.encode()
        out += struct.pack("<B", len(nb)) + nb + struct.pack("<4f", *p, yaw)
    with open(DST, "wb") as f:
        f.write(out)
    print(f"wrote {DST}: {len(out) / 1e6:.2f} MB")


if __name__ == "__main__":
    main()
