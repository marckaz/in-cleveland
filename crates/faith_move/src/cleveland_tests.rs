//! The Cleveland map, section by section, beaten with scripted input.

use glam::{Vec2, Vec3};

use crate::cleveland::{self, z, TT_X, Y10, Y11, Y3, Y5, Y7, Y9};
use crate::course_tests::{once, toward_x, AngleIn, Sim};
use crate::*;

fn sim(cp: usize) -> Sim {
    Sim::on(cleveland::cleveland(), cp)
}

fn fwd() -> Input {
    Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() }
}

/// Where to climb on the Public Square roof: right of the HVAC.
const CLIMB_X: f32 = 5.5;

fn hanging(c: &Controller) -> bool {
    matches!(c.state, State::LedgeHang { .. } | State::Traverse(_))
}

#[test]
fn c1_market_rail_skylight_pipe_gap() {
    let mut s = sim(0);
    let (mut v, mut m, mut sl, mut g) = (false, false, false, false);
    s.run(10.0, |c, _| {
        if c.feet.z < z::C2_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed |= once(&mut v, c.feet.z < z::RAIL + 1.0);
        i.jump_pressed |= once(&mut m, c.feet.z < z::SKYLIGHT + 0.8 && c.feet.z > z::SKYLIGHT - 1.0);
        i.crouch_pressed |= once(&mut sl, c.feet.z < z::PIPE + 2.6 && c.state == State::Ground);
        i.crouch_held = sl && c.feet.z > z::PIPE - 0.8;
        i.jump_pressed |= once(&mut g, c.feet.z < z::C1_END + 0.5 && c.state == State::Ground);
        i
    });
    assert!(s.has(Event::Vault), "{:?}", s.events);
    assert!(s.has(Event::Slide), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Lorain Ave"), "ended at {:?} {:?}", s.c.feet, s.events);
}

#[test]
fn c2_lorain_springboard() {
    let mut s = sim(1);
    let mut j = false;
    s.run(8.0, |c, _| {
        if c.feet.z < z::C3_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::STEP_START + 3.0);
        i
    });
    assert!(s.has(Event::SpringBoard), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("West Bank"), "ended at {:?} {:?}", s.c.feet, s.events);
}

/// Without the springboard, a plain jump at the edge doesn't make the West Bank roof.
#[test]
fn c2_plain_jump_falls_short() {
    let mut s = sim(1);
    let mut j = false;
    s.run(6.0, |c, _| {
        let mut i = toward_x(c, 4.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::C2_END + 0.4 && c.state == State::Ground);
        i
    });
    assert!(!s.has(Event::SpringBoard));
    if s.checkpoint() == Some("West Bank") {
        assert!(s.has(Event::WallClimbStart) && s.has(Event::PullUp), "jumped straight up there: {:?}", s.events);
    }
}

fn counter(c: &Controller) -> f32 {
    match c.state {
        State::Balance { lean, .. } => (-lean * 4.0).clamp(-1.0, 1.0),
        _ => 0.0,
    }
}

#[test]
fn c3_west_bank_balance_beam() {
    let mut s = sim(2);
    s.run(14.0, |c, _| {
        if c.feet.z < z::C4_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        if matches!(c.state, State::Balance { .. }) {
            return Input { move_axis: Vec2::new(counter(c), 1.0), ..Default::default() };
        }
        let mut i = toward_x(c, 0.0);
        i.move_axis.y = 0.6;
        i
    });
    assert!(s.has(Event::BalanceStart), "{:?}", s.events);
    assert!(!s.has(Event::BalanceFall), "{:?}", s.events);
    assert_eq!(s.deaths, 0);
    assert_eq!(s.checkpoint(), Some("Cuyahoga River"), "ended at {:?}", s.c.feet);
}

#[test]
fn c4_zipline_across_the_cuyahoga() {
    let mut s = sim(3);
    let mut j = false;
    let mut top_speed = 0.0f32;
    s.run(14.0, |c, _| {
        if let State::ZipLine { speed, .. } = c.state {
            top_speed = top_speed.max(speed);
            return Input::default();
        }
        if c.feet.z < z::C5_START {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::ZIP_START + 1.8 && c.state == State::Ground);
        i
    });
    assert!(s.has(Event::ZipStart), "{:?}", s.events);
    assert!(s.has(Event::ZipEnd { hit_wall: false }), "{:?}", s.events);
    assert!(top_speed > 8.0, "zip too slow: {top_speed}");
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Tower City"), "ended at {:?}", s.c.feet);
}

/// Miss the cable and it's the river valley: the fall is fatal.
#[test]
fn c4_missing_the_zip_is_fatal() {
    let mut level = cleveland::cleveland();
    level.fixtures.retain(|f| !matches!(f, Fixture::ZipLine { .. }));
    let mut s = Sim::on(level, 3);
    let mut j = false;
    s.run(8.0, |c, _| {
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::C4_END + 0.5 && c.state == State::Ground);
        i
    });
    assert!(s.deaths > 0, "survived at {:?}", s.c.feet);
}

#[test]
fn c5_wallrun_terminal_tower() {
    let mut s = sim(4);
    let mut j = false;
    let mut angle = AngleIn::default();
    s.run(7.0, |c, _| {
        if c.feet.z < z::C6_START - 3.0 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, TT_X - 0.6);
        i.jump_pressed = once(&mut j, c.feet.z < z::C5_END + 1.3);
        angle.apply(c, &mut i, 25.0);
        i
    });
    assert!(s.has(Event::WallRunStart), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Public Square"), "ended at {:?}", s.c.feet);
    assert!((s.c.feet.y - Y5).abs() < 0.05);
}

#[test]
fn c5_girder_walk() {
    let mut s = sim(4);
    s.run(10.0, |c, _| {
        if c.feet.z < z::C6_START - 2.0 {
            return Input::default();
        }
        let mut i = toward_x(c, -6.0);
        if c.feet.z < z::C5_END + 4.0 {
            i.move_axis = Vec2::new(((-6.0 - c.feet.x) * 2.0).clamp(-0.3, 0.3), 0.6);
        }
        i
    });
    assert_eq!(s.deaths, 0, "fell off at {:?} {:?}", s.c.feet, s.events);
    assert_eq!(s.checkpoint(), Some("Public Square"), "ended at {:?}", s.c.feet);
}

#[test]
fn c6_climb_to_ontario() {
    let mut s = sim(5);
    let mut j = false;
    s.run(7.0, |c, _| {
        if c.feet.y > Y7 - 0.1 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, CLIMB_X);
        i.jump_pressed = once(&mut j, c.feet.z < z::CLIMB + 1.2);
        i
    });
    assert!(s.has(Event::WallClimbStart), "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Ontario St"), "ended at {:?} {:?}", s.c.feet, s.events);
}

#[test]
fn c6_crate_route() {
    let mut s = sim(5);
    let (mut a, mut b) = (false, false);
    s.run(8.0, |c, _| {
        if c.feet.y > Y7 - 0.1 && c.state == State::Ground {
            return Input::default();
        }
        if hanging(c) {
            return fwd();
        }
        let mut i = toward_x(c, -6.0);
        i.jump_pressed |= once(&mut a, c.feet.z < z::CLIMB + 4.0 && c.feet.y < Y5 + 0.5);
        i.jump_pressed |= once(&mut b, c.feet.y > Y5 + 1.2 && c.state == State::Ground && c.feet.z < z::CLIMB + 1.5);
        i
    });
    assert!(s.has(Event::Mantle) || s.has(Event::Vault), "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Ontario St"), "ended at {:?} {:?}", s.c.feet, s.events);
}

#[test]
fn c7_slalom_and_girder_over_ontario() {
    let mut s = sim(6);
    s.run(9.0, |c, _| {
        if c.feet.z < z::C8_START - 2.0 {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        if c.feet.z < z::C7_END + 3.0 {
            i.move_axis = Vec2::new((-c.feet.x * 2.0).clamp(-0.3, 0.3), 0.7);
        }
        i
    });
    assert_eq!(s.deaths, 0, "{:?} at {:?}", s.events, s.c.feet);
    assert_eq!(s.checkpoint(), Some("Euclid Ave"), "ended at {:?}", s.c.feet);
}

#[test]
fn c8_swing_across_euclid() {
    let mut s = sim(7);
    let (mut j, mut off) = (false, false);
    s.run(12.0, |c, _| {
        if c.feet.z < z::C9_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        if let State::Swing { angle, rate, .. } = c.state {
            let mut i = fwd();
            i.jump_pressed = once(&mut off, rate > 0.5 && angle > 0.45);
            return i;
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::C8_END + 0.5 && c.state == State::Ground);
        i
    });
    assert!(s.has(Event::SwingStart), "{:?}", s.events);
    assert!(s.has(Event::SwingJump), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Old Arcade"), "ended at {:?}", s.c.feet);
    assert!((s.c.feet.y - Y9).abs() < 0.05);
}

#[test]
fn c9_drop_needs_roll() {
    let mut s = sim(8);
    s.run(5.0, |c, _| {
        let mut i = toward_x(c, 0.0);
        i.crouch_pressed = c.state == State::Air && c.feet.y < Y10 + 1.0 && c.vel.y < 0.0;
        i
    });
    assert!(s.has(Event::Roll), "{:?}", s.events);
    assert!(!s.has(Event::HardLand), "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("East 4th St"));
}

#[test]
fn c10_east_4th_duct_vents_door_climb_finish() {
    let mut s = sim(9);
    let (mut sl, mut va, mut vb, mut d, mut cl) = (false, false, false, false, false);
    s.run(14.0, |c, _| {
        if c.feet.z < z::FINISH + 1.0 {
            return Input::default();
        }
        if hanging(c) {
            return fwd();
        }
        let mut i = toward_x(c, 0.0);
        i.crouch_pressed |= once(&mut sl, c.feet.z < z::DUCT + 2.8 && c.state == State::Ground);
        i.crouch_held = c.feet.z < z::DUCT + 2.8 && c.feet.z > z::DUCT - 1.0;
        i.jump_pressed |= once(&mut va, c.feet.z < z::VENT_A + 0.9 && c.feet.z > z::VENT_A - 0.5);
        i.jump_pressed |= once(&mut vb, c.feet.z < z::VENT_B + 0.9 && c.feet.z > z::VENT_B - 0.5);
        i.melee_pressed |= once(&mut d, c.feet.z < z::DOOR + 1.5 && c.state == State::Ground);
        i.jump_pressed |= once(&mut cl, c.feet.z < z::FINISH_WALL + 1.2 && c.feet.y < Y10 + 0.5);
        i
    });
    assert!(s.has(Event::Slide), "{:?}", s.events);
    assert!(s.has(Event::Barge { hands: true }), "{:?}", s.events);
    assert!(s.has(Event::DoorOpened { door: 0 }), "{:?}", s.events);
    assert!(s.has(Event::WallClimbStart), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert!(s.level.in_finish(s.c.feet), "finish? feet {:?} events {:?}", s.c.feet, s.events);
    assert!((s.c.feet.y - Y11).abs() < 0.05);
}

/// The door is shut until you open it: run straight at it without a kick and you stop there.
#[test]
fn c10_door_blocks_until_opened() {
    let mut s = sim(9);
    let (mut sl, mut va, mut vb) = (false, false, false);
    s.run(8.0, |c, _| {
        let mut i = toward_x(c, 0.0);
        i.crouch_pressed |= once(&mut sl, c.feet.z < z::DUCT + 2.8 && c.state == State::Ground);
        i.crouch_held = c.feet.z < z::DUCT + 2.8 && c.feet.z > z::DUCT - 1.0;
        i.jump_pressed |= once(&mut va, c.feet.z < z::VENT_A + 0.9 && c.feet.z > z::VENT_A - 0.5);
        i.jump_pressed |= once(&mut vb, c.feet.z < z::VENT_B + 0.9 && c.feet.z > z::VENT_B - 0.5);
        i
    });
    assert!(s.c.feet.z > z::DOOR, "went through a shut door: {:?}", s.c.feet);
}

/// The whole run, West Side Market to Playhouse Square, in one go: every move on the map.
#[test]
fn full_run_west_side_market_to_playhouse_square() {
    let level = cleveland::cleveland();
    let mut s = Sim::on(level.clone(), 0);
    let mut f = Flags::default();
    let mut angle = AngleIn::default();
    let mut sec = 0usize;
    let mut finished = None;
    s.run(120.0, |c, t| {
        if let Some(i) = level.checkpoint_at(c.feet) {
            sec = sec.max(i);
        }
        if finished.is_none() && level.in_finish(c.feet) {
            finished = Some(t);
        }
        full_run_input(c, sec, &mut f, &mut angle)
    });
    assert_eq!(s.deaths, 0, "died: {:?}", s.events);
    assert!(s.level.in_finish(s.c.feet), "ended at {:?} ({:?}) {:?}", s.c.feet, s.checkpoint(), s.events);
    for e in [
        Event::Vault,
        Event::Slide,
        Event::SpringBoard,
        Event::BalanceStart,
        Event::ZipStart,
        Event::WallRunStart,
        Event::WallClimbStart,
        Event::SwingStart,
        Event::SwingJump,
        Event::Roll,
        Event::Barge { hands: true },
    ] {
        assert!(s.has(e), "never did {e:?}: {:?}", s.events);
    }
    let t = finished.expect("never reached the finish");
    // A bot's run, steady rather than fast: it should come in well under two minutes.
    assert!(t < 120.0, "{t}");
    println!("West Side Market to Playhouse Square: {t:.1} s");
}

#[derive(Default)]
struct Flags {
    rail: bool,
    sky: bool,
    pipe: bool,
    gap: bool,
    board: bool,
    zip: bool,
    wall: bool,
    climb6: bool,
    swing: bool,
    off: bool,
    duct: bool,
    vent_a: bool,
    vent_b: bool,
    door: bool,
    climb10: bool,
}

/// Run forward, turning the view (not strafing) to head for `x` a few metres ahead, the way
/// a player steers at speed.
fn aim(c: &Controller, x: f32) -> Input {
    let want = (-(x - c.feet.x)).atan2(6.0);
    let d = (want - c.yaw + std::f32::consts::PI).rem_euclid(2.0 * std::f32::consts::PI) - std::f32::consts::PI;
    Input { move_axis: Vec2::new(0.0, 1.0), look: Vec2::new(d.clamp(-0.04, 0.04), 0.0), ..Default::default() }
}

/// One player's input for the whole map, by the furthest checkpoint reached (`sec`).
fn full_run_input(c: &Controller, sec: usize, f: &mut Flags, angle: &mut AngleIn) -> Input {
    let zf = c.feet.z;
    if zf < z::FINISH + 1.0 && sec == 10 {
        return Input::default();
    }
    if hanging(c) {
        return fwd();
    }
    match c.state {
        State::Balance { .. } => return Input { move_axis: Vec2::new(counter(c), 1.0), ..Default::default() },
        State::ZipLine { .. } => return Input::default(),
        State::Swing { angle: a, rate, .. } => {
            let mut i = fwd();
            i.jump_pressed = once(&mut f.off, rate > 0.5 && a > 0.45);
            return i;
        }
        _ => {}
    }
    let mut i = aim(c, 0.0);
    match sec {
        0 => {
            i.jump_pressed |= once(&mut f.rail, zf < z::RAIL + 1.0);
            i.jump_pressed |= once(&mut f.sky, zf < z::SKYLIGHT + 0.8 && zf > z::SKYLIGHT - 1.0);
            i.crouch_pressed |= once(&mut f.pipe, zf < z::PIPE + 2.6 && c.state == State::Ground);
            i.crouch_held = f.pipe && zf > z::PIPE - 0.8;
            i.jump_pressed |= once(&mut f.gap, zf < z::C1_END + 0.5 && c.state == State::Ground);
        }
        1 => i.jump_pressed = once(&mut f.board, zf < z::STEP_START + 3.0),
        2 => i.move_axis.y = 0.6,
        3 => i.jump_pressed = once(&mut f.zip, zf < z::ZIP_START + 1.8 && c.state == State::Ground),
        4 => {
            i = toward_x(c, TT_X - 0.6);
            i.jump_pressed = once(&mut f.wall, zf < z::C5_END + 1.3);
            angle.apply(c, &mut i, 25.0);
        }
        5 => {
            i = aim(c, CLIMB_X);
            i.jump_pressed = once(&mut f.climb6, zf < z::CLIMB + 1.2);
        }
        6 => {
            if zf < z::C7_END + 3.0 {
                i.move_axis.y = 0.7;
            }
        }
        7 => i.jump_pressed = once(&mut f.swing, zf < z::C8_END + 0.5 && c.state == State::Ground),
        8 => i.crouch_pressed = c.state == State::Air && c.feet.y < Y10 + 1.0 && c.vel.y < 0.0,
        _ => {
            // Still dropping in from the Old Arcade (East 4th's checkpoint starts mid-fall): roll.
            i.crouch_pressed = zf > z::DUCT + 3.0 && c.state == State::Air && c.vel.y < 0.0 && c.feet.y < Y10 + 1.0;
            i.crouch_pressed |= once(&mut f.duct, zf < z::DUCT + 2.8 && c.state == State::Ground);
            i.crouch_held = zf < z::DUCT + 2.8 && zf > z::DUCT - 1.0;
            i.jump_pressed |= once(&mut f.vent_a, zf < z::VENT_A + 0.9 && zf > z::VENT_A - 0.5);
            i.jump_pressed |= once(&mut f.vent_b, zf < z::VENT_B + 0.9 && zf > z::VENT_B - 0.5);
            i.melee_pressed |= once(&mut f.door, zf < z::DOOR + 1.5 && c.state == State::Ground);
            i.jump_pressed |= once(&mut f.climb10, zf < z::FINISH_WALL + 1.2 && c.feet.y < Y10 + 0.5);
        }
    }
    i
}

#[test]
fn checkpoints_are_grounded() {
    let level = cleveland::cleveland();
    let w = level.world();
    for cp in &level.checkpoints {
        let mut c = Controller::new(Tuning::default(), cp.spawn + Vec3::Y * 0.01, cp.yaw);
        for _ in 0..30 {
            c.step(1.0 / 60.0, &Input::default(), &w);
        }
        assert_eq!(c.state, State::Ground, "{} spawn not grounded: {:?}", cp.name, c.feet);
        assert_eq!(level.checkpoint_at(c.feet).map(|i| level.checkpoints[i].name), Some(cp.name));
    }
}

/// Nothing in the scenery pokes into the run: the space above every roof on the route is clear
/// of anything that isn't part of the course.
#[test]
fn scenery_stays_off_the_route() {
    let level = cleveland::cleveland();
    let corridor = Aabb::new(Vec3::new(-12.0, Y10 + 0.1, z::C11_END), Vec3::new(TT_X - 0.01, 20.0, 6.0));
    for (b, look) in &level.solids {
        if *look == crate::greybox::Look::Skyline || *look == crate::greybox::Look::Green || *look == crate::greybox::Look::Water {
            let hit = b.min.x < corridor.max.x && b.max.x > corridor.min.x && b.min.z < corridor.max.z && b.max.z > corridor.min.z && b.max.y > corridor.min.y;
            assert!(!hit, "scenery box in the route: {:?}", b);
        }
    }
    let _ = (Y3, Y9);
}
