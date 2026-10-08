//! "Downtown": real downtown Cleveland at true scale, from OpenStreetMap.
//!
//! About 3 km across, from the West Side Market to Playhouse Square and the lakefront, every
//! building at its real footprint and height (OSM's `height`, or `building:levels` x 3.5 m,
//! or a guess by building type where OSM has neither). Real streets are far too wide to jump,
//! so there's a parkour layer on top: ladders up from the street onto low roofs, and ziplines
//! from roof to lower roof across the gaps. Free run: no course, no finish; 1-0 jump between
//! landmarks.
//!
//! Baked by `tools/osm/bake.py` from `data/osm/cleveland.json` (fetched by
//! `tools/osm/fetch.py`) into `data/downtown.bin`, read here. Map data (c) OpenStreetMap
//! contributors, ODbL 1.0.
//!
//! x is east and z is south (north is −Z), in metres from the middle of Public Square.

use glam::Vec3;

use crate::climb::Ladder;
use crate::greybox::{Level, Look, TriMesh};
use crate::world::{Aabb, Fixture, MeshWorld};

static DATA: &[u8] = include_bytes!("../data/downtown.bin");

/// Heights of the flat shapes on the ground (apart, so they don't flicker into each other).
const ROAD_Y: f32 = 0.03;
const GREEN_Y: f32 = 0.06;
const WATER_Y: f32 = 0.09;

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        s
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take(2).try_into().unwrap())
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn i16(&mut self) -> i16 {
        i16::from_le_bytes(self.take(2).try_into().unwrap())
    }
    fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn vec3(&mut self) -> Vec3 {
        Vec3::new(self.f32(), self.f32(), self.f32())
    }
    fn str(&mut self) -> String {
        let n = self.u8() as usize;
        String::from_utf8_lossy(self.take(n)).into_owned()
    }
    /// Decimetres to metres.
    fn xz(&mut self) -> (f32, f32) {
        (self.i16() as f32 * 0.1, self.i16() as f32 * 0.1)
    }
}

/// A building from the map: its footprint, roof and walls.
#[derive(Clone, Debug)]
pub struct Building {
    pub name: String,
    pub base: f32,
    pub top: f32,
    /// Outer ring then holes, in (x, z).
    pub rings: Vec<Vec<(f32, f32)>>,
}

impl Building {
    /// Whether (x, z) is inside the footprint (outer ring, not in a hole).
    pub fn contains(&self, x: f32, z: f32) -> bool {
        self.rings.iter().filter(|r| inside_ring(r, x, z)).count() % 2 == 1
    }
}

fn inside_ring(r: &[(f32, f32)], x: f32, z: f32) -> bool {
    let mut inside = false;
    let n = r.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, zi) = r[i];
        let (xj, zj) = r[j];
        if (zi > z) != (zj > z) && x < (xj - xi) * (z - zi) / (zj - zi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Everything in the baked file.
pub struct Map {
    pub bounds: (f32, f32, f32, f32),
    pub buildings: Vec<(Look, Building, Vec<[u16; 3]>)>,
    pub flats: Vec<(u8, Vec<Vec<(f32, f32)>>, Vec<[u32; 3]>)>,
    pub zips: Vec<(Vec3, Vec3)>,
    pub ladders: Vec<(Vec3, f32, Vec3)>,
    pub props: Vec<Prop>,
    /// Roofs that can be reached (by ladder, climb, jump, drop or zipline), out of all you can
    /// stand on.
    pub reachable: (u32, u32),
    pub spots: Vec<(String, Vec3, f32)>,
}

/// An object in the city: a box turned by `yaw`, standing on `base`.
#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub kind: u8,
    pub solid: bool,
    pub base: Vec3,
    /// Half width (x), height, half depth (z).
    pub size: Vec3,
    pub yaw: f32,
    pub rgb: [f32; 3],
}

/// Prop kinds, as `bake.py` numbers them.
pub mod kind {
    pub const AC: u8 = 0;
    pub const VENT: u8 = 1;
    pub const TANK: u8 = 2;
    pub const HUT: u8 = 3;
    pub const SKYLIGHT: u8 = 4;
    pub const CAR: u8 = 5;
    pub const CABIN: u8 = 6;
    pub const DUMPSTER: u8 = 7;
    pub const TRUNK: u8 = 8;
    pub const CANOPY: u8 = 9;
    pub const POLE: u8 = 10;
    pub const LAMP: u8 = 11;
    pub const BENCH: u8 = 12;
    pub const SHELTER: u8 = 13;
    pub const HYDRANT: u8 = 14;
    pub const BIN: u8 = 15;
}

pub fn read() -> Map {
    let mut r = Reader { b: DATA, at: 0 };
    assert_eq!(r.take(4), b"CLE3", "downtown.bin: wrong format (re-run tools/osm/bake.py)");
    let bounds = (r.f32(), r.f32(), r.f32(), r.f32());
    let mut buildings = vec![];
    for _ in 0..r.u32() {
        let look = match r.u8() {
            1 => Look::Stone,
            2 => Look::Glass,
            _ => Look::Wall,
        };
        let (base, top) = (r.f32(), r.f32());
        let name = r.str();
        let rings = (0..r.u16()).map(|_| (0..r.u16()).map(|_| r.xz()).collect()).collect();
        let tris = (0..r.u32()).map(|_| [r.u16(), r.u16(), r.u16()]).collect();
        buildings.push((look, Building { name, base, top, rings }, tris));
    }
    let mut flats = vec![];
    for _ in 0..r.u32() {
        let kind = r.u8();
        let rings = (0..r.u16()).map(|_| (0..r.u32()).map(|_| r.xz()).collect()).collect();
        let tris = (0..r.u32()).map(|_| [r.u32(), r.u32(), r.u32()]).collect();
        flats.push((kind, rings, tris));
    }
    let zips = (0..r.u32()).map(|_| (r.vec3(), r.vec3())).collect();
    let ladders = (0..r.u32())
        .map(|_| {
            let base = r.vec3();
            let top = r.f32();
            let n = Vec3::new(r.f32(), 0.0, r.f32()).normalize();
            (base, top, n)
        })
        .collect();
    let props = (0..r.u32())
        .map(|_| {
            let kind = r.u8();
            let solid = r.u8() != 0;
            let base = r.vec3();
            let size = r.vec3();
            let yaw = r.f32();
            let rgb = [r.u8() as f32 / 255.0, r.u8() as f32 / 255.0, r.u8() as f32 / 255.0];
            Prop { kind, solid, base, size, yaw, rgb }
        })
        .collect();
    let reachable = (r.u32(), r.u32());
    let spots = (0..r.u32())
        .map(|_| {
            let name = r.str();
            let p = r.vec3();
            (name, p, r.f32())
        })
        .collect();
    Map { bounds, buildings, flats, zips, ladders, props, reachable, spots }
}

/// Every triangle of a closed shape wound to face away from its `centre`.
fn outward(tris: Vec<[Vec3; 3]>, centre: Vec3) -> Vec<[Vec3; 3]> {
    tris.into_iter().map(|t| facing(t, (t[0] + t[1] + t[2]) / 3.0 - centre)).collect()
}

/// A square rod from `a` to `b`, `thick` across.
fn rod(a: Vec3, b: Vec3, thick: f32) -> Vec<[Vec3; 3]> {
    let d = b - a;
    let len = d.length().max(1e-4);
    let rot = glam::Quat::from_rotation_arc(Vec3::Y, d / len);
    let h = Vec3::new(thick * 0.5, len * 0.5, thick * 0.5);
    let c = (a + b) * 0.5;
    let p = |x: f32, y: f32, z: f32| c + rot * Vec3::new(x * h.x, y * h.y, z * h.z);
    let v = [p(-1., -1., -1.), p(1., -1., -1.), p(1., 1., -1.), p(-1., 1., -1.), p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.)];
    [[0, 1, 2, 3], [5, 4, 7, 6], [4, 0, 3, 7], [1, 5, 6, 2]].iter().flat_map(|f| MeshWorld::quad(v[f[0]], v[f[1]], v[f[2]], v[f[3]])).collect()
}

/// The triangle wound so its front faces along `want` (renderers cull the back).
fn facing(t: [Vec3; 3], want: Vec3) -> [Vec3; 3] {
    if (t[1] - t[0]).cross(t[2] - t[0]).dot(want) < 0.0 { [t[0], t[2], t[1]] } else { t }
}

/// Stable 0..1 hash, for varying the buildings' shades.
fn hash(i: usize) -> f32 {
    let mut x = (i as u32).wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

pub fn downtown() -> Level {
    let map = read();
    let mut l = Level { name: "Downtown", fog: Some((250.0, 2200.0)), ..Level::default() };
    let (x0, z0, x1, z1) = map.bounds;

    // ---- the ground, and a wall of fog-coloured cliffs past the edge of the data
    l.add(Look::Roof, [x0 - 400.0, -2.0, z0 - 400.0], [x1 + 400.0, 0.0, z1 + 400.0]);

    // ---- buildings
    let mut walls: [TriMesh; 3] = [Look::Wall, Look::Stone, Look::Glass].map(|look| TriMesh { look, solid: true, tris: vec![], tint: vec![], colors: vec![] });
    let mut roofs = TriMesh { look: Look::Roof, solid: true, tris: vec![], tint: vec![], colors: vec![] };
    for (i, (look, b, tris)) in map.buildings.iter().enumerate() {
        let wi = match look {
            Look::Stone => 1,
            Look::Glass => 2,
            _ => 0,
        };
        let shade = 0.86 + 0.14 * hash(i);
        let flat: Vec<(f32, f32)> = b.rings.iter().flatten().copied().collect();
        for ring in &b.rings {
            for k in 0..ring.len() {
                let (ax, az) = ring[k];
                let (bx, bz) = ring[(k + 1) % ring.len()];
                // Out of the building: whichever side of the wall isn't inside it.
                let (ex, ez) = (bx - ax, bz - az);
                let len = (ex * ex + ez * ez).sqrt().max(1e-6);
                let mut out = Vec3::new(ez / len, 0.0, -ex / len);
                let (mx, mz) = ((ax + bx) * 0.5, (az + bz) * 0.5);
                if b.contains(mx + out.x * 0.05, mz + out.z * 0.05) {
                    out = -out;
                }
                let q = MeshWorld::quad(
                    Vec3::new(ax, b.base, az),
                    Vec3::new(bx, b.base, bz),
                    Vec3::new(bx, b.top, bz),
                    Vec3::new(ax, b.top, az),
                );
                for t in q {
                    walls[wi].tris.push(facing(t, out));
                    walls[wi].tint.push(shade);
                }
            }
        }
        for t in tris {
            let p = |k: u16| {
                let (x, z) = flat[k as usize];
                Vec3::new(x, b.top, z)
            };
            roofs.tris.push(facing([p(t[0]), p(t[1]), p(t[2])], Vec3::Y));
            roofs.tint.push(0.9 + 0.1 * hash(i + 7));
            if b.base > 0.0 {
                let p = |k: u16| {
                    let (x, z) = flat[k as usize];
                    Vec3::new(x, b.base, z)
                };
                walls[wi].tris.push(facing([p(t[0]), p(t[1]), p(t[2])], -Vec3::Y));
                walls[wi].tint.push(shade * 0.8);
            }
        }
    }
    l.meshes.extend(walls);
    l.meshes.push(roofs);

    // ---- streets, parks and water, painted on the ground
    for (look, y, kind) in [(Look::Road, ROAD_Y, 0u8), (Look::Green, GREEN_Y, 2), (Look::Water, WATER_Y, 1)] {
        let mut m = TriMesh { look, solid: false, tris: vec![], tint: vec![], colors: vec![] };
        for (k, rings, tris) in &map.flats {
            if *k != kind {
                continue;
            }
            let flat: Vec<(f32, f32)> = rings.iter().flatten().copied().collect();
            for t in tris {
                let p = |i: u32| {
                    let (x, z) = flat[i as usize];
                    Vec3::new(x, y, z)
                };
                m.tris.push(facing([p(t[0]), p(t[1]), p(t[2])], Vec3::Y));
                m.tint.push(1.0);
            }
        }
        l.meshes.push(m);
    }

    // ---- parkour: ziplines (with masts) and ladders
    let mut props = TriMesh { look: Look::Runner, solid: false, tris: vec![], tint: vec![], colors: vec![] };
    for &(a, b) in &map.zips {
        l.fixtures.push(Fixture::ZipLine { a, b });
        let along = Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize_or_zero();
        let side = Vec3::new(-along.z, 0.0, along.x);
        for (end, roof_drop) in [(a, 2.8), (b, 2.6)] {
            let foot = Vec3::new(end.x, end.y - roof_drop, end.z) + side * 0.85;
            l.add(Look::Runner, [foot.x - 0.15, foot.y, foot.z - 0.15], [foot.x + 0.15, end.y + 0.5, foot.z + 0.15]);
            // The arm out over the cable.
            let mid = (Vec3::new(end.x, end.y + 0.38, end.z) + Vec3::new(foot.x, end.y + 0.38, foot.z)) * 0.5;
            let yaw = side.x.atan2(side.z);
            props.tris.extend(MeshWorld::oriented_box(mid, Vec3::new(0.06, 0.06, 0.5), yaw));
        }
    }
    props.tint = vec![1.0; props.tris.len()];
    l.meshes.push(props);
    // Ladders: rails as boxes, rungs as flat strips (there are over a thousand of them).
    let mut iron = TriMesh { look: Look::Paint, solid: false, tris: vec![], tint: vec![], colors: vec![] };
    for &(base, top, normal) in &map.ladders {
        let ladder = Ladder { base, top, normal, pipe: false, exit: true };
        for (k, (a, b, thick)) in ladder.rods().into_iter().enumerate() {
            if k < 2 {
                iron.tris.extend(outward(rod(a, b, thick), (a + b) * 0.5));
            } else {
                let up = Vec3::Y * thick * 0.5;
                let out = normal * 0.02;
                iron.tris.extend(MeshWorld::quad(a - up + out, b - up + out, b + up + out, a + up + out).map(|t| facing(t, normal)));
            }
        }
        l.fixtures.push(Fixture::Ladder(ladder));
    }
    iron.colors = vec![[0.62, 0.10, 0.07]; iron.tris.len()];
    l.meshes.push(iron);
    l.ladders_in_meshes = true;

    // ---- objects: rooftop clutter, cars, dumpsters, trees, lamps, benches, bus shelters
    let mut solid = TriMesh { look: Look::Paint, solid: true, tris: vec![], tint: vec![], colors: vec![] };
    let mut soft = TriMesh { look: Look::Paint, solid: false, tris: vec![], tint: vec![], colors: vec![] };
    let mut lit = TriMesh { look: Look::Lights, solid: false, tris: vec![], tint: vec![], colors: vec![] };
    for p in &map.props {
        let half = Vec3::new(p.size.x, p.size.y * 0.5, p.size.z);
        let centre = p.base + Vec3::Y * half.y;
        let tris = outward(MeshWorld::oriented_box(centre, half, p.yaw), centre);
        let m = if p.kind == kind::LAMP {
            &mut lit
        } else if p.solid {
            &mut solid
        } else {
            &mut soft
        };
        for (k, t) in tris.into_iter().enumerate() {
            // Faces a touch darker on the sides than on top, so boxes read in flat light.
            let shade = if k >= 8 && k < 10 { 1.0 } else { 0.86 };
            m.tris.push(t);
            m.colors.push([p.rgb[0] * shade, p.rgb[1] * shade, p.rgb[2] * shade]);
        }
    }
    lit.tint = vec![1.0; lit.tris.len()];
    lit.colors.clear();
    l.meshes.extend([solid, soft, lit]);

    // ---- spawn points: 1-0 jump between them
    for (name, p, yaw) in map.spots.iter().filter(|(_, p, _)| p.x > x0 && p.x < x1 && p.z > z0 && p.z < z1) {
        let name: &'static str = Box::leak(name.clone().into_boxed_str());
        l.checkpoints.push(crate::greybox::Checkpoint {
            name,
            region: Aabb::new(*p - Vec3::new(6.0, 1.0, 6.0), *p + Vec3::new(6.0, 4.0, 6.0)),
            spawn: *p,
            yaw: *yaw,
        });
    }
    l
}
