//! The city maps (real Cleveland from OpenStreetMap: Downtown, Ohio City & Tremont, Lakewood):
//! each loads, you can stand at every spawn point, the ground follows the real terrain, and the
//! parkour layer works: ladders get you onto roofs, ziplines carry you across streets onto the
//! next roof.

use glam::{Vec2, Vec3};

use crate::downtown::{self, Map};
use crate::world::Layered;
use crate::*;

const DT: f32 = 1.0 / 60.0;

struct Town {
    level: greybox::Level,
    boxes: BoxWorld,
    mesh: MeshWorld,
    map: Map,
}

fn town() -> Town {
    town_of("downtown")
}

fn town_of(key: &str) -> Town {
    let name = downtown::REGIONS.iter().find(|r| r.0 == key).unwrap().1;
    let level = downtown::region(name, downtown::read_key(key));
    let boxes = level.world();
    let mesh = level.mesh_world().expect("the city has buildings");
    Town { level, boxes, mesh, map: downtown::read_key(key) }
}

impl Town {
    fn world(&self) -> Layered<'_> {
        Layered { still: &self.boxes, moving: &self.mesh }
    }

    /// Run from `at` facing `dir`, with `f` choosing the input. Returns the events and deaths.
    fn run(&self, at: Vec3, dir: Vec3, secs: f32, mut f: impl FnMut(&Controller) -> Input) -> (Controller, Vec<Event>) {
        let mut c = Controller::new(Tuning::default(), at + Vec3::Y * 0.01, yaw_of(dir));
        c.state = State::Ground;
        let w = self.world();
        let mut ev = vec![];
        let mut t = 0.0;
        while t < secs {
            let i = f(&c);
            c.step(DT, &i, &w);
            ev.extend(c.events.iter().copied());
            t += DT;
        }
        (c, ev)
    }

    /// Height of the highest roof under (x, z), or `f32::MIN` in the open.
    fn roof(&self, x: f32, z: f32) -> f32 {
        self.map.buildings.iter().filter(|(_, b, _)| b.contains(x, z)).map(|(_, b, _)| b.top).fold(f32::MIN, f32::max)
    }
}

/// The controller's yaw to face `d` (yaw 0 faces -Z, turning left is positive).
fn yaw_of(d: Vec3) -> f32 {
    (-d.x).atan2(-d.z)
}

fn fwd() -> Input {
    Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() }
}

fn died(ev: &[Event]) -> bool {
    ev.iter().any(|e| *e == Event::Death)
}

#[test]
fn the_map_loads() {
    let t = town();
    assert!(t.map.buildings.len() > 50, "{} buildings", t.map.buildings.len());
    assert!(!t.map.zips.is_empty() && !t.map.ladders.is_empty() && t.level.checkpoints.len() >= 3);
    // Key Tower is the tallest thing in Ohio.
    let tallest = t.map.buildings.iter().max_by(|a, b| a.1.top.total_cmp(&b.1.top)).unwrap();
    assert_eq!(tallest.1.name, "Key Tower");
    assert!(tallest.1.top > 280.0, "{}", tallest.1.top);
}

#[test]
fn every_spawn_point_stands() {
    spawns_stand(&town());
}

fn spawns_stand(t: &Town) {
    for cp in &t.level.checkpoints {
        let (c, ev) = t.run(cp.spawn, Vec3::NEG_Z, 0.6, |_| Input::default());
        assert_eq!(c.state, State::Ground, "{} at {:?}: {:?}", cp.name, c.feet, ev);
        assert!((c.feet.y - cp.spawn.y).abs() < 0.1, "{} sank to {:?}", cp.name, c.feet);
        assert!(!died(&ev), "{}", cp.name);
        // And the view isn't inside a wall.
        let eye = cp.spawn + Vec3::Y * 1.6;
        assert!(!t.world().overlaps(&Aabb::new(eye - Vec3::splat(0.3), eye + Vec3::splat(0.3))), "{}: eye inside something", cp.name);
    }
}

/// Every roof you can stand on has a way up (ladders from the street or from a neighbouring
/// roof, climbs, jumps, safe drops, ziplines): all but a few slivers of skyscraper crowns.
#[test]
fn nearly_every_roof_can_be_reached() {
    roofs_reachable(&town(), 2000);
}

fn roofs_reachable(t: &Town, at_least: u32) {
    let (got, of) = t.map.reachable;
    println!("roofs reachable: {got}/{of}");
    assert!(of > at_least && got as f32 >= 0.995 * of as f32, "{got}/{of}");
}

/// Ladders work: a spread of them (from the street and from roofs, short and tall, and the
/// tallest of all) climbed by scripted input onto their roofs.
#[test]
fn ladders_climb_onto_the_roofs() {
    ladders_climb(&town(), 160);
}

fn ladders_climb(t: &Town, sample: usize) {
    let n = t.map.ladders.len();
    let mut pick: Vec<usize> = (0..n).step_by((n / sample).max(1)).collect();
    // The longest few too (the skyscraper service ladders) and some from roof to roof.
    let mut by_len: Vec<usize> = (0..n).collect();
    by_len.sort_by(|&a, &b| (t.map.ladders[b].1 - t.map.ladders[b].0.y).total_cmp(&(t.map.ladders[a].1 - t.map.ladders[a].0.y)));
    pick.extend(by_len.iter().take(2));
    // Roof-to-roof ladders: their foot stands on a roof rather than the ground.
    let on_roof = |i: usize| {
        let b = t.map.ladders[i].0;
        b.y > t.map.ground.at(b.x, b.z) + 0.5
    };
    pick.extend((0..n).filter(|&i| on_roof(i)).step_by(10).take(sample / 4));
    pick.sort();
    pick.dedup();
    let mut bad = vec![];
    for &i in &pick {
        let (base, top, nrm) = t.map.ladders[i];
        let secs = 20.0 + (top - base.y) * 1.3;
        // Start on whatever is under the run-up (the ground may rise towards the ladder).
        let mut start = base + nrm * 2.5;
        start.y = start.y.max(t.map.ground.at(start.x, start.z) + 0.02);
        let (c, ev) = t.run(start, -nrm, secs, |c| if c.feet.y > top - 0.1 && c.state == State::Ground { Input::default() } else { fwd() });
        let up = (c.feet.y - top).abs() < 0.1 && c.state == State::Ground;
        if !up || died(&ev) {
            bad.push((base, top, c.feet, c.state));
        }
    }
    println!("ladders: {}/{} of {} climbed", pick.len() - bad.len(), pick.len(), n);
    for b in bad.iter().take(10) {
        println!("  stuck: ladder at {:?} to {:.1}: ended {:?} {:?}", b.0, b.1, b.2, b.3);
    }
    assert!(bad.is_empty(), "{} of {} ladders tried don't get you up", bad.len(), pick.len());
}

/// The objects are there and solid: cars, rooftop units, trees.
#[test]
fn the_city_has_things_in_it() {
    use crate::downtown::kind;
    let t = town();
    let count = |k: u8| t.map.props.iter().filter(|p| p.kind == k).count();
    assert!(count(kind::CAR) > 1000 && count(kind::AC) > 1000 && count(kind::TRUNK) > 300, "cars {} ac {} trees {}", count(kind::CAR), count(kind::AC), count(kind::TRUNK));
    cars_are_solid(&t);
}

/// Walk into a parked car and you stop (or vault it), not pass through.
fn cars_are_solid(t: &Town) {
    use crate::downtown::kind;
    let flat = |p: &&downtown::Prop| {
        let g = &t.map.ground;
        let s = 6.0;
        [(s, 0.0), (-s, 0.0), (0.0, s), (0.0, -s)].iter().all(|(dx, dz)| (g.at(p.base.x + dx, p.base.z + dz) - p.base.y).abs() < 0.3)
    };
    let car = t.map.props.iter().filter(|p| p.kind == kind::CAR && t.roof(p.base.x, p.base.z) == f32::MIN).find(flat).unwrap();
    let side = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos()); // the car's local +Z: across it
    let (c, _) = t.run(car.base + side * 4.0, -side, 3.0, |_| Input { move_axis: Vec2::new(0.0, 0.4), ..Default::default() });
    let across = (c.feet - car.base).dot(side);
    assert!(across > car.size.z * 0.9 || c.feet.y > car.base.y + 0.8, "walked through a car: {:?} vs {:?}", c.feet, car.base);
}

#[test]
fn ziplines_carry_you_across() {
    ziplines_work(&town());
}

fn ziplines_work(t: &Town) {
    let mut bad = vec![];
    for &(a, b) in &t.map.zips {
        let along = Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize();
        let roof_a = a.y - 2.8;
        let roof_b = b.y - 2.6;
        // Run up from behind the mast and jump for the cable, as on the Moves map.
        let start = Vec3::new(a.x, roof_a, a.z) - along * 5.0;
        let mut jumped = false;
        let mut zipped = false;
        let (c, ev) = t.run(start, along, 20.0, |c| {
            if let State::ZipLine { s, .. } = c.state {
                if std::env::var_os("ZIP_TRACE").is_some() { eprintln!("  ride s {s:.1} feet {:?}", c.feet); }
                zipped = true;
                return Input::default();
            }
            if zipped {
                return Input::default();
            }
            let mut i = fwd();
            let to = Vec3::new(a.x - c.feet.x, 0.0, a.z - c.feet.z);
            if !jumped && to.dot(along) < 1.8 && c.state == State::Ground {
                i.jump_pressed = true;
                jumped = true;
            }
            i
        });
        let landed = (c.feet.y - roof_b).abs() < 0.15 && c.state == State::Ground;
        if !zipped || died(&ev) || !landed {
            if std::env::var_os("ZIP_TRACE").is_some() {
                eprintln!("zip {a:?}->{b:?}: {:?}", ev.iter().filter(|e| !matches!(e, Event::Land { .. })).collect::<Vec<_>>());
            }
            bad.push((a, b, zipped, c.feet, c.state));
        }
    }
    let ok = t.map.zips.len() - bad.len();
    println!("ziplines: {ok}/{} ridden", t.map.zips.len());
    for z in bad.iter().take(10) {
        println!("  failed: {:?} -> {:?} zipped {} ended {:?} {:?}", z.0, z.1, z.2, z.3, z.4);
    }
    assert!(bad.is_empty(), "{} of {} ziplines don't work", bad.len(), t.map.zips.len());
}

/// Walk off the top of a skyscraper (Key Tower): that fall is fatal, and you respawn.
#[test]
fn falling_off_a_tall_roof_is_fatal() {
    tall_falls_kill(&town(), 150.0);
}

fn tall_falls_kill(t: &Town, high: f32) {
    let key = t.level.checkpoints.iter().find(|c| c.name.ends_with("roof") && c.spawn.y > high).expect("a tall roof spawn");
    // Whichever way is open (the tower's upper parts stand on its roof).
    let fell = (0..8).any(|k| {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let (_, ev) = t.run(key.spawn, Vec3::new(a.sin(), 0.0, a.cos()), 25.0, |_| fwd());
        died(&ev)
    });
    assert!(fell, "walked off {} and lived", key.name);
}


/// The ground has real depth: it rises and falls (Downtown sits on a bluff over the Flats and
/// the lake), you stand on it where the elevation data says it is, and walking downhill keeps
/// you on it.
#[test]
fn the_ground_follows_the_terrain() {
    ground_is_real(&town(), 25.0);
}

fn ground_is_real(t: &Town, relief: f32) {
    let g = &t.map.ground;
    assert!(t.map.real_ground, "no elevation data baked");
    let lo = g.heights.iter().copied().fold(f32::MAX, f32::min);
    let hi = g.heights.iter().copied().fold(f32::MIN, f32::max);
    assert!(hi - lo > relief, "ground only varies {:.1} m", hi - lo);
    // Drop onto open ground around the map: you land where the terrain says.
    let (x0, z0, x1, z1) = t.map.bounds;
    let mut tried = 0;
    for k in 0..400 {
        let x = x0 + (x1 - x0) * ((k * 37 % 400) as f32 / 400.0);
        let z = z0 + (z1 - z0) * ((k * 91 % 400) as f32 / 400.0);
        let y = g.at(x, z);
        if t.roof(x, z) != f32::MIN || g.lake.is_some_and(|l| y < l + 0.5) {
            continue;
        }
        let near = Aabb::new(Vec3::new(x - 1.5, y + 0.05, z - 1.5), Vec3::new(x + 1.5, y + 2.5, z + 1.5));
        if t.world().overlaps(&near) {
            continue; // a car, a tree, a bench
        }
        tried += 1;
        let (c, ev) = t.run(Vec3::new(x, y + 1.0, z), Vec3::NEG_Z, 1.5, |_| Input::default());
        assert!(!died(&ev), "died dropping onto the ground at {x:.0}, {z:.0}");
        assert!((c.feet.y - y).abs() < 0.25, "ground at {x:.0}, {z:.0} is {y:.2} but stood at {:.2}", c.feet.y);
        // Then run off across it, up and down the slopes, and never sink into it.
        let a = k as f32 * 2.4;
        let mut sank = None;
        t.run(c.feet, Vec3::new(a.sin(), 0.0, a.cos()), 5.0, |c| {
            let under = g.at(c.feet.x, c.feet.z);
            if sank.is_none() && c.feet.y < under - 0.3 && t.roof(c.feet.x, c.feet.z) == f32::MIN {
                sank = Some((c.feet, under));
            }
            fwd()
        });
        assert!(sank.is_none(), "sank into the ground running from {x:.0}, {z:.0}: {sank:?}");
    }
    assert!(tried > 40, "only {tried} open spots");
}

/// Ohio City & Tremont: the same checks on the West Side.
#[test]
fn westside_works() {
    let t = town_of("westside");
    assert!(t.map.buildings.len() > 3000, "{} buildings", t.map.buildings.len());
    assert!(t.level.checkpoints.iter().any(|c| c.name == "West Side Market"));
    spawns_stand(&t);
    ground_is_real(&t, 25.0);
    roofs_reachable(&t, 3000);
    ladders_climb(&t, 60);
    cars_are_solid(&t);
    ziplines_work(&t);
    tall_falls_kill(&t, 30.0);
}

/// Lakewood: the same checks.
#[test]
fn lakewood_works() {
    let t = town_of("lakewood");
    assert!(t.map.buildings.len() > 1500, "{} buildings", t.map.buildings.len());
    assert!(t.level.checkpoints.iter().any(|c| c.name == "Lakewood Park"));
    spawns_stand(&t);
    ground_is_real(&t, 25.0);
    roofs_reachable(&t, 1500);
    ladders_climb(&t, 60);
    cars_are_solid(&t);
    ziplines_work(&t);
    tall_falls_kill(&t, 30.0);
}

#[test]
#[ignore]
fn debug_ladder() {
    let t = town();
    let x: f32 = std::env::var("LX").map(|v| v.parse().unwrap()).unwrap_or(-11.168);
    let i = t.map.ladders.iter().position(|l| (l.0.x - x).abs() < 0.01).unwrap();
    let (base, top, nrm) = t.map.ladders[i];
    let g = &t.map.ground;
    let st = base + nrm * 2.5;
    println!("base {base:?} top {top} nrm {nrm:?} ground@base {} ground@start {} lake {:?}", g.at(base.x, base.z), g.at(st.x, st.z), g.lake);
    let mut k = 0;
    t.run(st, -nrm, 3.0, |c| { k += 1; if k % 6 == 0 { println!("{:?} {:?}", c.feet, c.state); } fwd() });
}

#[test]
#[ignore]
fn debug_zip() {
    let t = town();
    let pts: Vec<Vec3> = match std::env::var("P") {
        Ok(v) => { let f: Vec<f32> = v.split(',').map(|x| x.parse().unwrap()).collect(); vec![Vec3::new(f[0], f[1], f[2])] }
        Err(_) => vec![Vec3::new(287.5, 19.1, 41.1), Vec3::new(698.9, 15.0, -89.6)],
    };
    for p in pts {
        for dy in [-1.0f32, 0.0, 1.0, 2.0] {
            let c = p + Vec3::Y * dy;
            let r = Aabb::new(c - Vec3::splat(0.4), c + Vec3::splat(0.4));
            println!("{c:?}: boxes {} mesh {} ground {:.2}", t.boxes.overlaps(&r), t.mesh.overlaps(&r), t.map.ground.at(c.x, c.z));
        }
        for (look, b, _) in t.map.buildings.iter().filter(|(_, b, _)| b.rings[0].iter().any(|q| (q.0 - p.x).abs() < 15.0 && (q.1 - p.z).abs() < 15.0)) {
            println!("  bldg {:?} {:?} base {:.1} top {:.1}", look, b.name, b.base, b.top);
        }
        for pr in t.map.props.iter().filter(|q| (q.base.x - p.x).abs() < 4.0 && (q.base.z - p.z).abs() < 4.0 && (q.base.y - p.y).abs() < 5.0) {
            println!("  prop kind {} solid {} base {:?} size {:?}", pr.kind, pr.solid, pr.base, pr.size);
        }
    }
}
