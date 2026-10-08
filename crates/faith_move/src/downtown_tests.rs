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
    }
}

#[test]
fn ladders_climb_onto_the_roofs() {
    let t = town();
    let mut bad = vec![];
    for &(base, top, n) in &t.map.ladders {
        let (c, ev) = t.run(base + n * 2.5, -n, 30.0, |c| if c.feet.y > top - 0.1 && c.state == State::Ground { Input::default() } else { fwd() });
        let up = (c.feet.y - top).abs() < 0.1 && c.state == State::Ground;
        if !up || died(&ev) {
            bad.push((base, top, c.feet, c.state));
        }
    }
    let ok = t.map.ladders.len() - bad.len();
    println!("ladders: {ok}/{} climbed", t.map.ladders.len());
    for b in bad.iter().take(10) {
        println!("  stuck: ladder at {:?} to {:.1}: ended {:?} {:?}", b.0, b.1, b.2, b.3);
    }
    assert!(bad.is_empty(), "{} of {} ladders don't get you up", bad.len(), t.map.ladders.len());
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

/// Walk off the top of Key Tower: 289 m down is fatal, and you respawn.
#[test]
fn falling_off_a_tall_roof_is_fatal() {
    let t = town();
    let key = t.level.checkpoints.iter().find(|c| c.name == "Key Tower roof").unwrap();
    let (_, ev) = t.run(key.spawn, Vec3::Z, 25.0, |_| fwd());
    assert!(died(&ev), "walked off Key Tower and lived");
}
