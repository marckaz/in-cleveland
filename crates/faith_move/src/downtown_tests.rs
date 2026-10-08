//! The Downtown map (real Cleveland from OpenStreetMap): it loads, you can stand at every spawn
//! point, and the parkour layer works: ladders get you onto roofs, ziplines carry you across
//! streets onto the next roof.

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
    let level = downtown::downtown();
    let boxes = level.world();
    let mesh = level.mesh_world().expect("downtown has buildings");
    Town { level, boxes, mesh, map: downtown::read() }
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

    /// Height of the highest roof under (x, z), or 0.
    fn roof(&self, x: f32, z: f32) -> f32 {
        self.map.buildings.iter().filter(|(_, b, _)| b.contains(x, z)).map(|(_, b, _)| b.top).fold(0.0, f32::max)
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
    let t = town();
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
    let t = town();
    let (got, of) = t.map.reachable;
    println!("roofs reachable: {got}/{of}");
    assert!(of > 2000 && got as f32 >= 0.995 * of as f32, "{got}/{of}");
}

/// Ladders work: a spread of them (from the street and from roofs, short and tall, and the
/// tallest of all) climbed by scripted input onto their roofs.
#[test]
fn ladders_climb_onto_the_roofs() {
    let t = town();
    let n = t.map.ladders.len();
    let mut pick: Vec<usize> = (0..n).step_by((n / 160).max(1)).collect();
    // The longest few too (the skyscraper service ladders) and some from roof to roof.
    let mut by_len: Vec<usize> = (0..n).collect();
    by_len.sort_by(|&a, &b| (t.map.ladders[b].1 - t.map.ladders[b].0.y).total_cmp(&(t.map.ladders[a].1 - t.map.ladders[a].0.y)));
    pick.extend(by_len.iter().take(2));
    pick.extend((0..n).filter(|&i| t.map.ladders[i].0.y > 0.5).step_by(10).take(40));
    pick.sort();
    pick.dedup();
    let mut bad = vec![];
    for &i in &pick {
        let (base, top, nrm) = t.map.ladders[i];
        let secs = 20.0 + (top - base.y) * 1.3;
        let (c, ev) = t.run(base + nrm * 2.5, -nrm, secs, |c| if c.feet.y > top - 0.1 && c.state == State::Ground { Input::default() } else { fwd() });
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
    // Walk into a parked car and you stop (or vault it), not pass through.
    let car = t.map.props.iter().find(|p| p.kind == kind::CAR && t.roof(p.base.x, p.base.z) == 0.0).unwrap();
    let side = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos()); // the car's local +Z: across it
    let (c, _) = t.run(car.base + side * 4.0, -side, 3.0, |_| Input { move_axis: Vec2::new(0.0, 0.4), ..Default::default() });
    let across = (c.feet - car.base).dot(side);
    assert!(across > car.size.z * 0.9 || c.feet.y > 0.8, "walked through a car: {:?} vs {:?}", c.feet, car.base);
}

#[test]
fn ziplines_carry_you_across() {
    let t = town();
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
    let t = town();
    let key = t.level.checkpoints.iter().find(|c| c.name.ends_with("roof") && c.spawn.y > 150.0).expect("a skyscraper roof spawn");
    // Whichever way is open (the tower's upper parts stand on its roof).
    let fell = (0..8).any(|k| {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let (_, ev) = t.run(key.spawn, Vec3::new(a.sin(), 0.0, a.cos()), 25.0, |_| fwd());
        died(&ev)
    });
    assert!(fell, "walked off Key Tower and lived");
}

