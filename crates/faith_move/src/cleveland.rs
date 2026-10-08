//! "Cleveland": a rooftop run across Cleveland, Ohio, west to east, from the
//! West Side Market in Ohio City, over the Cuyahoga, past Terminal Tower and
//! Public Square, down Euclid Avenue to the chandelier at Playhouse Square.
//!
//! The run goes toward −Z, which is **east**; **north is −X** (left as you run),
//! so Lake Erie is off to your left and the ballpark is off to your right.
//! The landmarks are in their real order and on their real sides of the route,
//! but distances are squeezed (the real walk is about 3 km; this run is about
//! 390 m) and the skyscrapers are drawn at 0.6 of their height so their tops
//! sit inside the fog.
//!
//! ```text
//!  1 West Side Market (y 0)    rail vault, skylight, slide the pipe, 4 m gap
//!  2 Lorain Ave (-1.5)         springboard up 3.5 m onto the next roof
//!  3 West Bank (2.0)           balance beam over the gap
//!  4 Cuyahoga River (2.0)      zipline 66 m across the river valley (bridges below)
//!  5 Tower City (-8.0)         wallrun Terminal Tower's base over an 8 m gap (or the girder)
//!  6 Public Square (-8.0)      wallclimb 3.3 m beside the HVAC (or crate + grab on the left)
//!  7 Ontario St (-4.7)         AC slalom, 12 m girder across Ontario St, the square on your left
//!  8 Euclid Ave (-4.7)         swing pole across Euclid
//!  9 Old Arcade (-6.2)         6 m drop: roll it
//! 10 East 4th St (-12.2)       slide the duct under the string lights, hop the vents,
//!                              barge the door, wallclimb
//! 11 Playhouse Square (-8.9)   finish under the GE Chandelier
//! ```
//!
//! Every route is beaten by scripted input in `cleveland_tests.rs`.

use glam::Vec3;

use crate::greybox::{Level, Look};
use crate::world::{Aabb, Fixture};

/// Street level downtown and in Ohio City.
pub const STREET_Y: f32 = -30.0;
/// The floor of the Flats, down by the river.
pub const FLATS_Y: f32 = -40.0;
/// The Cuyahoga and Lake Erie.
pub const WATER_Y: f32 = -40.4;
/// Bottom of every building (below the Flats, so the river-bank ones reach the ground).
const BASE_Y: f32 = -42.0;

pub const Y1: f32 = 0.0; // West Side Market
pub const Y2: f32 = -1.5; // Lorain Ave
pub const Y3: f32 = Y2 + 3.5; // West Bank (springboard up)
pub const Y4: f32 = Y3; // Cuyahoga (zip start)
pub const Y5: f32 = -8.0; // Tower City (zip end)
pub const Y6: f32 = Y5; // Public Square
pub const Y7: f32 = Y6 + 3.3; // Ontario St (wallclimb up)
pub const Y8: f32 = Y7; // Euclid Ave
pub const Y9: f32 = Y8 - 1.5; // Old Arcade (swing down)
pub const Y10: f32 = Y9 - 6.0; // East 4th (drop and roll)
pub const Y11: f32 = Y10 + 3.3; // Playhouse Square (wallclimb up)

/// The springboard's block and step (TdMove_SpringBoard), as on the Moves map.
pub const BLOCK_TOP: f32 = Y2 + 1.2;
pub const STEP_TOP: f32 = Y2 + 0.64;
/// Swing bar over Euclid.
pub const POLE_Y: f32 = Y8 + 2.9;
/// x of Terminal Tower's base, the wall you wallrun (right side).
pub const TT_X: f32 = 8.0;

/// Key z positions (the run goes toward −Z, east).
pub mod z {
    // 1 West Side Market
    pub const RAIL: f32 = -6.0;
    pub const SKYLIGHT: f32 = -14.0;
    pub const PIPE: f32 = -22.0;
    pub const C1_END: f32 = -28.0;
    // 2 Lorain Ave
    pub const C2_START: f32 = -32.0;
    pub const STEP_START: f32 = -59.38;
    pub const BLOCK_START: f32 = -60.5;
    pub const C2_END: f32 = -62.0;
    // 3 West Bank
    pub const C3_START: f32 = -64.5;
    pub const C3_END: f32 = -92.0;
    // 4 Cuyahoga River
    pub const C4_START: f32 = -100.0;
    pub const ZIP_START: f32 = -113.5;
    pub const C4_END: f32 = -116.0;
    /// The river valley's two bluffs, and the water between.
    pub const BLUFF_W: f32 = -118.0;
    pub const RIVER_W: f32 = -132.0;
    pub const RIVER_E: f32 = -158.0;
    pub const BLUFF_E: f32 = -172.0;
    // 5 Tower City
    pub const C5_START: f32 = -174.0;
    pub const ZIP_END: f32 = -180.0;
    pub const C5_END: f32 = -204.0;
    // 6 Public Square
    pub const C6_START: f32 = -212.0;
    pub const CLIMB: f32 = -238.0;
    // 7 Ontario St
    pub const SLALOM_A: f32 = -250.0;
    pub const SLALOM_B: f32 = -256.0;
    pub const C7_END: f32 = -264.0;
    // 8 Euclid Ave
    pub const C8_START: f32 = -276.0;
    pub const C8_END: f32 = -292.0;
    pub const POLE: f32 = -294.5;
    // 9 Old Arcade
    pub const C9_START: f32 = -300.0;
    pub const C9_END: f32 = -316.0;
    // 10 East 4th
    pub const DUCT: f32 = -326.0;
    pub const VENT_A: f32 = -332.0;
    pub const VENT_B: f32 = -337.0;
    pub const DOOR: f32 = -344.0;
    pub const FINISH_WALL: f32 = -356.0;
    // 11 Playhouse Square
    pub const FINISH: f32 = -370.0;
    pub const C11_END: f32 = -384.0;
}

/// The zipline across the Cuyahoga.
pub const ZIP_A: Vec3 = Vec3::new(0.0, Y4 + 2.8, z::ZIP_START);
pub const ZIP_B: Vec3 = Vec3::new(0.0, Y5 + 2.6, z::ZIP_END);

/// A box from two opposite corners, in either order.
fn bx(l: &mut Level, look: Look, a: [f32; 3], b: [f32; 3]) {
    let (a, b) = (Vec3::from(a), Vec3::from(b));
    l.add(look, a.min(b).to_array(), a.max(b).to_array());
}

/// Drawn only, never collided with.
fn paint(l: &mut Level, look: Look, a: [f32; 3], b: [f32; 3]) {
    let (a, b) = (Vec3::from(a), Vec3::from(b));
    l.paint(look, a.min(b).to_array(), a.max(b).to_array());
}

/// A building on the route: from below the Flats up to its roof.
fn building(l: &mut Level, x0: f32, x1: f32, z0: f32, z1: f32, top: f32) {
    bx(l, Look::Roof, [x0, BASE_Y, z0], [x1, top, z1]);
}

/// Knee-high walls down both long sides of a roof.
fn parapets(l: &mut Level, x0: f32, x1: f32, z0: f32, z1: f32, top: f32) {
    bx(l, Look::Wall, [x0, top, z0], [x0 + 0.3, top + 0.9, z1]);
    bx(l, Look::Wall, [x1 - 0.3, top, z0], [x1, top + 0.9, z1]);
}

/// A tower of setbacks centred on (cx, cz): each tier is (half width x, half depth z, top),
/// stacked from `base`.
fn tower(l: &mut Level, look: Look, cx: f32, cz: f32, base: f32, tiers: &[(f32, f32, f32)]) {
    let mut y = base;
    for &(hx, hz, top) in tiers {
        bx(l, look, [cx - hx, y, cz - hz], [cx + hx, top, cz + hz]);
        y = top;
    }
}

/// An arch of small blocks under a bridge deck, from z `za` to `zb`, springing at `y0`
/// and rising `rise` at the middle (drawn only).
fn arch(l: &mut Level, x0: f32, x1: f32, za: f32, zb: f32, y0: f32, rise: f32, n: usize) {
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let zc = za + (zb - za) * t;
        let yc = y0 + rise * 4.0 * t * (1.0 - t);
        paint(l, Look::Prop, [x0, yc - 0.6, zc - 0.9], [x1, yc + 0.6, zc + 0.9]);
    }
}

/// Block letters 3 pixels wide and 5 tall, each row read top to bottom.
fn glyph(c: char) -> [&'static str; 5] {
    match c {
        'C' => ["###", "#..", "#..", "#..", "###"],
        'L' => ["#..", "#..", "#..", "#..", "###"],
        'E' => ["###", "#..", "##.", "#..", "###"],
        _ => ["...", "...", "...", "...", "..."],
    }
}

/// A sign of lit block letters facing +Z (you read it running toward −Z), left edge at `x0`,
/// bottom at `y0`.
fn sign(l: &mut Level, text: &str, x0: f32, y0: f32, zf: f32, px: f32) {
    for (li, ch) in text.chars().enumerate() {
        for (r, row) in glyph(ch).iter().enumerate() {
            for (c, cell) in row.chars().enumerate() {
                if cell != '#' {
                    continue;
                }
                let x = x0 + (li * 4 + c) as f32 * px;
                let y = y0 + (4 - r) as f32 * px;
                paint(l, Look::Lights, [x, y, zf - 0.12], [x + px, y + px, zf]);
            }
        }
    }
}

pub fn cleveland() -> Level {
    use Look::*;
    let mut l = Level { name: "Cleveland", ..Level::default() };

    // ---- 1 West Side Market: start on a roof in Ohio City ---------------
    building(&mut l, -8.0, 8.0, 6.0, z::C1_END, Y1);
    parapets(&mut l, -8.0, 8.0, 6.0, z::C1_END, Y1);
    bx(&mut l, Wall, [-8.0, Y1, 5.5], [8.0, Y1 + 3.5, 6.0]);
    // AC units with a rail between them: vault the rail.
    bx(&mut l, Prop, [-7.7, Y1, z::RAIL - 1.0], [-2.0, Y1 + 1.7, z::RAIL + 1.0]);
    bx(&mut l, Prop, [2.0, Y1, z::RAIL - 1.0], [7.7, Y1 + 1.7, z::RAIL + 1.0]);
    bx(&mut l, Runner, [-2.0, Y1, z::RAIL - 0.12], [2.0, Y1 + 1.0, z::RAIL + 0.12]);
    // Skylight: mantle onto it, or go round.
    bx(&mut l, Runner, [-3.5, Y1, z::SKYLIGHT - 3.0], [3.5, Y1 + 1.1, z::SKYLIGHT]);
    bx(&mut l, Prop, [-3.3, Y1 + 1.1, z::SKYLIGHT - 2.8], [3.3, Y1 + 1.15, z::SKYLIGHT - 0.2]);
    // Pipe across the roof: slide under it (or coil-jump over).
    bx(&mut l, Runner, [-7.7, Y1 + 1.3, z::PIPE - 0.35], [7.7, Y1 + 1.6, z::PIPE]);
    bx(&mut l, Prop, [-7.7, Y1, z::PIPE - 0.35], [-7.2, Y1 + 1.3, z::PIPE]);
    bx(&mut l, Prop, [7.2, Y1, z::PIPE - 0.35], [7.7, Y1 + 1.3, z::PIPE]);
    paint(&mut l, Runner, [-7.7, Y1 - 0.02, z::C1_END], [7.7, Y1 + 0.004, z::C1_END + 0.4]);

    // ---- 2 Lorain Ave: run-up, then springboard at the edge -------------
    building(&mut l, -6.0, 6.0, z::C2_START, z::C2_END, Y2);
    parapets(&mut l, -6.0, 6.0, z::C2_START, z::C2_END, Y2);
    bx(&mut l, Runner, [-2.0, Y2, z::C2_END], [2.0, BLOCK_TOP, z::BLOCK_START]);
    bx(&mut l, Runner, [-2.0, Y2, z::BLOCK_START], [2.0, STEP_TOP, z::STEP_START]);
    paint(&mut l, Runner, [-2.0, Y2 - 0.02, z::STEP_START], [2.0, Y2 + 0.004, z::STEP_START + 6.0]);
    // A lip under the West Bank roof's edge: out of reach from here without the springboard.
    paint(&mut l, Runner, [-6.0, Y3 - 0.25, z::C3_START + 0.004], [6.0, Y3, z::C3_START + 0.01]);

    // ---- 3 West Bank: beam over the gap at the far end ------------------
    building(&mut l, -6.0, 6.0, z::C3_START, z::C3_END, Y3);
    parapets(&mut l, -6.0, 6.0, z::C3_START - 1.0, z::C3_END, Y3);
    bx(&mut l, Runner, [-0.12, Y3 - 0.3, z::C4_START], [0.12, Y3, z::C3_END]);
    l.fixtures.push(Fixture::Beam { a: Vec3::new(0.0, Y3, z::C3_END), b: Vec3::new(0.0, Y3, z::C4_START) });
    // Walls either side of the beam's start, so you take it.
    bx(&mut l, Wall, [-6.0, Y3, z::C3_END + 0.3], [-0.6, Y3 + 1.2, z::C3_END]);
    bx(&mut l, Wall, [0.6, Y3, z::C3_END + 0.3], [6.0, Y3 + 1.2, z::C3_END]);

    // ---- 4 Cuyahoga River: zipline across the valley --------------------
    building(&mut l, -6.0, 6.0, z::C4_START, z::C4_END, Y4);
    parapets(&mut l, -6.0, 6.0, z::C4_START, z::C4_END, Y4);
    l.fixtures.push(Fixture::ZipLine { a: ZIP_A, b: ZIP_B });
    // Masts the cable hangs from (off to the side, with an arm over the line).
    bx(&mut l, Prop, [0.7, Y4, z::ZIP_START - 0.15], [1.0, ZIP_A.y + 0.5, z::ZIP_START + 0.15]);
    paint(&mut l, Prop, [-0.1, ZIP_A.y + 0.3, z::ZIP_START - 0.08], [1.0, ZIP_A.y + 0.45, z::ZIP_START + 0.08]);
    bx(&mut l, Prop, [0.7, Y5, z::ZIP_END - 0.15], [1.0, ZIP_B.y + 0.5, z::ZIP_END + 0.15]);
    paint(&mut l, Prop, [-0.1, ZIP_B.y + 0.3, z::ZIP_END - 0.08], [1.0, ZIP_B.y + 0.45, z::ZIP_END + 0.08]);
    // "CLE" in lights on the West Bank roof, facing you as you come off the beam.
    let px = 0.4;
    bx(&mut l, Prop, [1.6, Y4, -108.2], [1.8, Y4 + 2.7, -108.0]);
    bx(&mut l, Prop, [5.4, Y4, -108.2], [5.6, Y4 + 2.7, -108.0]);
    paint(&mut l, Prop, [1.4, Y4 + 2.55, -108.2], [5.8, Y4 + 2.7, -108.0]);
    paint(&mut l, Prop, [1.4, Y4 + 0.45, -108.2], [5.8, Y4 + 0.6, -108.0]);
    sign(&mut l, "CLE", 1.4, Y4 + 0.6, -108.0, px);

    // ---- 5 Tower City: wallrun Terminal Tower's base over the gap -------
    building(&mut l, -8.0, TT_X, z::C5_START, z::C5_END, Y5);
    bx(&mut l, Wall, [-8.0, Y5, z::C5_END], [-7.7, Y5 + 0.9, z::C5_START]);
    // Terminal Tower's podium down the right side, one flat face (nothing to snag on), painted
    // red where it spans the gap.
    bx(&mut l, Skyline, [TT_X, BASE_Y, -218.0], [50.0, Y5 + 6.5, z::BLUFF_E - 1.0]);
    paint(&mut l, Runner, [TT_X - 0.02, Y5 - 12.0, -218.0], [TT_X, Y5 + 4.5, -192.0]);
    paint(&mut l, Runner, [3.0, Y5 - 0.02, -196.4], [TT_X, Y5 + 0.004, -196.0]);
    // Girder across the gap on the left: walk it instead.
    bx(&mut l, Runner, [-6.4, Y5 - 0.4, z::C6_START - 0.5], [-5.6, Y5, z::C5_END + 0.5]);

    // ---- 6 Public Square (the Higbee roof): HVAC, then the climb ---------
    building(&mut l, -8.0, TT_X, z::C6_START, z::CLIMB, Y6);
    bx(&mut l, Wall, [-8.0, Y6, z::CLIMB], [-7.7, Y6 + 0.9, z::C6_START]);
    // HVAC block in the middle, against the climb: land the wallrun, keep right, climb.
    bx(&mut l, Prop, [-1.0, Y6, -236.0], [3.0, Y6 + 3.2, -232.0]);
    // Crate against the climb wall on the left: mantle it, then grab the roof.
    bx(&mut l, Prop, [-7.7, Y6, z::CLIMB], [-4.5, Y6 + 1.3, z::CLIMB + 3.0]);

    // ---- 7 Ontario St: upper roof (its front face is the climb) ---------
    building(&mut l, -8.0, 8.0, z::CLIMB, z::C7_END, Y7);
    paint(&mut l, Runner, [-2.0, Y7 - 0.25, z::CLIMB - 0.01], [2.0, Y7 + 0.004, z::CLIMB + 0.004]);
    parapets(&mut l, -8.0, 8.0, z::CLIMB, z::C7_END, Y7);
    bx(&mut l, Prop, [-7.7, Y7, z::SLALOM_A - 0.8], [-1.1, Y7 + 0.9, z::SLALOM_A]);
    bx(&mut l, Prop, [1.1, Y7, z::SLALOM_B - 0.8], [7.7, Y7 + 0.9, z::SLALOM_B]);
    // Girder across Ontario Street, Public Square down on your left.
    bx(&mut l, Runner, [-0.4, Y7 - 0.5, z::C8_START], [0.4, Y7, z::C7_END]);

    // ---- 8 Euclid Ave: swing pole across the avenue ---------------------
    building(&mut l, -6.0, 6.0, z::C8_START, z::C8_END, Y8);
    parapets(&mut l, -6.0, 6.0, z::C8_START, z::C8_END, Y8);
    paint(&mut l, Runner, [-2.5, Y8 - 0.02, z::C8_END + 0.4], [2.5, Y8 + 0.004, z::C8_END]);
    l.fixtures.push(Fixture::SwingPole { a: Vec3::new(-2.5, POLE_Y, z::POLE), b: Vec3::new(2.5, POLE_Y, z::POLE) });
    bx(&mut l, Prop, [-2.8, BASE_Y, z::POLE - 0.15], [-2.5, POLE_Y + 0.1, z::POLE + 0.15]);
    bx(&mut l, Prop, [2.5, BASE_Y, z::POLE - 0.15], [2.8, POLE_Y + 0.1, z::POLE + 0.15]);

    // ---- 9 Old Arcade: land the swing, then the 6 m drop ---------------
    building(&mut l, -6.0, 6.0, z::C9_START, z::C9_END, Y9);
    parapets(&mut l, -6.0, 6.0, z::C9_START, z::C9_END, Y9);
    paint(&mut l, Runner, [-5.7, Y9 - 0.02, z::C9_END], [5.7, Y9 + 0.004, z::C9_END + 0.4]);

    // ---- 10 East 4th St: duct, vents, a door, under the string lights ---
    building(&mut l, -12.0, 12.0, z::C9_END, z::FINISH_WALL, Y10);
    // Duct: slide under (solid above it, so no going over).
    bx(&mut l, Runner, [-12.0, Y10 + 1.3, z::DUCT - 0.5], [12.0, Y10 + 1.6, z::DUCT]);
    bx(&mut l, Prop, [-12.0, Y10 + 1.6, z::DUCT - 0.5], [12.0, Y10 + 3.5, z::DUCT]);
    // Vents to hop over.
    bx(&mut l, Runner, [-4.0, Y10, z::VENT_A - 0.5], [4.0, Y10 + 0.8, z::VENT_A]);
    bx(&mut l, Runner, [-4.0, Y10, z::VENT_B - 0.5], [4.0, Y10 + 0.9, z::VENT_B]);
    bx(&mut l, Wall, [-12.0, Y10, z::FINISH_WALL], [-11.7, Y10 + 1.0, z::C9_END]);
    bx(&mut l, Wall, [11.7, Y10, z::FINISH_WALL], [12.0, Y10 + 1.0, z::C9_END]);
    // A wall across with a door in it: barge it at a run, or kick it standing.
    bx(&mut l, Wall, [-11.7, Y10, z::DOOR - 0.3], [-0.7, Y10 + 3.0, z::DOOR]);
    bx(&mut l, Wall, [0.7, Y10, z::DOOR - 0.3], [11.7, Y10 + 3.0, z::DOOR]);
    bx(&mut l, Wall, [-0.7, Y10 + 2.2, z::DOOR - 0.3], [0.7, Y10 + 3.0, z::DOOR]);
    l.fixtures.push(Fixture::Door {
        b: Aabb::new(Vec3::new(-0.7, Y10, z::DOOR - 0.25), Vec3::new(0.7, Y10 + 2.2, z::DOOR - 0.05)),
        n: Vec3::Z,
    });
    // East 4th's string lights, zigzagging overhead between poles.
    for zs in [-320.0f32, -329.0, -335.0, -340.0, -348.0, -352.0] {
        bx(&mut l, Prop, [-12.0, Y10 + 1.0, zs - 0.15], [-11.7, Y10 + 5.6, zs + 0.15]);
        bx(&mut l, Prop, [11.7, Y10 + 1.0, zs - 0.15], [12.0, Y10 + 5.6, zs + 0.15]);
        let mut x = -11.0;
        while x <= 11.0 {
            let sag = 0.9 * (1.0 - (x / 11.7) * (x / 11.7));
            let y = Y10 + 5.4 - sag;
            paint(&mut l, Lights, [x - 0.09, y - 0.18, zs - 0.09], [x + 0.09, y, zs + 0.09]);
            x += 1.1;
        }
    }

    // ---- 11 Playhouse Square: finish under the GE Chandelier ------------
    building(&mut l, -12.0, 12.0, z::FINISH_WALL, z::C11_END, Y11);
    paint(&mut l, Runner, [-3.0, Y11 - 0.25, z::FINISH_WALL - 0.01], [3.0, Y11 + 0.004, z::FINISH_WALL + 0.004]);
    bx(&mut l, Finish, [-0.5, Y11, z::FINISH - 0.5], [0.5, Y11 + 3.0, z::FINISH + 0.5]);
    l.finish = Some(Aabb::new(
        Vec3::new(-6.0, Y11 - 0.5, z::FINISH - 2.0),
        Vec3::new(6.0, Y11 + 3.0, z::FINISH + 2.0),
    ));
    chandelier(&mut l, Vec3::new(0.0, Y11, z::FINISH));
    // Playhouse Square's tall red blade sign, lit up the edge.
    bx(&mut l, Runner, [10.6, Y11, -366.0], [11.2, Y11 + 9.0, -362.0]);
    let mut y = Y11 + 1.0;
    while y < Y11 + 8.6 {
        paint(&mut l, Lights, [10.5, y, -365.4], [10.6, y + 0.35, -362.6]);
        y += 0.7;
    }

    backdrop(&mut l);

    // ---- checkpoints -------------------------------------------------------
    l.checkpoint("West Side Market", [-8.0, -1.0, -4.0], [8.0, 3.0, 5.5], [0.0, Y1, 3.0]);
    l.checkpoint("Lorain Ave", [-6.0, Y2 - 1.0, z::C2_END], [6.0, Y2 + 3.0, z::C2_START], [0.0, Y2, -34.0]);
    l.checkpoint("West Bank", [-6.0, Y3 - 1.0, z::C3_END], [6.0, Y3 + 3.0, z::C3_START], [0.0, Y3, -82.0]);
    l.checkpoint("Cuyahoga River", [-6.0, Y4 - 1.0, z::C4_END], [6.0, Y4 + 3.0, z::C4_START], [0.0, Y4, -103.0]);
    l.checkpoint("Tower City", [-8.0, Y5 - 1.0, z::C5_END], [TT_X, Y5 + 3.0, z::C5_START], [0.0, Y5, -182.0]);
    l.checkpoint("Public Square", [-8.0, Y6 - 1.0, z::CLIMB], [TT_X, Y6 + 3.0, z::C6_START], [5.5, Y6, -216.0]);
    l.checkpoint("Ontario St", [-8.0, Y7 - 1.0, z::C7_END], [8.0, Y7 + 3.0, z::CLIMB], [0.0, Y7, -241.0]);
    l.checkpoint("Euclid Ave", [-6.0, Y8 - 1.0, z::C8_END], [6.0, Y8 + 3.0, z::C8_START], [0.0, Y8, -280.0]);
    l.checkpoint("Old Arcade", [-6.0, Y9 - 1.0, z::C9_END], [6.0, Y9 + 3.0, z::C9_START], [0.0, Y9, -304.0]);
    l.checkpoint("East 4th St", [-12.0, Y10 - 1.0, z::FINISH_WALL], [12.0, Y10 + 3.0, z::C9_END], [0.0, Y10, -320.0]);
    l.checkpoint("Playhouse Square", [-12.0, Y11 - 1.0, z::C11_END], [12.0, Y11 + 4.0, z::FINISH_WALL], [0.0, Y11, -359.0]);
    l
}

/// The GE Chandelier on a gantry over the finish (feet of the marker at `at`): tiers of lit
/// crystal hanging from a crossbeam.
fn chandelier(l: &mut Level, at: Vec3) {
    use Look::*;
    let (y0, zc) = (at.y, at.z);
    // Gantry posts (solid) and crossbeam.
    bx(l, Prop, [-7.2, y0, zc - 0.2], [-6.8, y0 + 8.6, zc + 0.2]);
    bx(l, Prop, [6.8, y0, zc - 0.2], [7.2, y0 + 8.6, zc + 0.2]);
    paint(l, Prop, [-7.2, y0 + 8.3, zc - 0.2], [7.2, y0 + 8.6, zc + 0.2]);
    // Cable and canopy.
    paint(l, Prop, [-0.04, y0 + 7.5, zc - 0.04], [0.04, y0 + 8.3, zc + 0.04]);
    paint(l, Lights, [-0.6, y0 + 7.3, zc - 0.6], [0.6, y0 + 7.5, zc + 0.6]);
    // Three rings, wider as they go down, each hung with strands of crystal.
    for (half, y) in [(1.0f32, 6.9f32), (1.6, 6.2), (2.1, 5.4)] {
        let yr = y0 + y;
        let t = 0.07;
        paint(l, Lights, [-half, yr - t, zc - half], [half, yr + t, zc - half + 2.0 * t]);
        paint(l, Lights, [-half, yr - t, zc + half - 2.0 * t], [half, yr + t, zc + half]);
        paint(l, Lights, [-half, yr - t, zc - half], [-half + 2.0 * t, yr + t, zc + half]);
        paint(l, Lights, [half - 2.0 * t, yr - t, zc - half], [half, yr + t, zc + half]);
        // Struts from the cable to the ring.
        paint(l, Prop, [-0.03, yr, zc - half], [0.03, yr + 0.03, zc + half]);
        paint(l, Prop, [-half, yr, zc - 0.03], [half, yr + 0.03, zc + 0.03]);
        let n = (half * 4.0) as i32;
        for i in 0..=n {
            let s = -half + 2.0 * half * i as f32 / n as f32;
            let len = 0.5 + 0.15 * ((i % 3) as f32);
            for (x, z) in [(s, zc - half), (s, zc + half), (-half, zc + s), (half, zc + s)] {
                paint(l, Lights, [x - 0.035, yr - len, z - 0.035], [x + 0.035, yr - t, z + 0.035]);
            }
        }
    }
    // The drop at the bottom.
    paint(l, Lights, [-0.25, y0 + 4.6, zc - 0.25], [0.25, y0 + 5.3, zc + 0.25]);
}

/// The city around the run: the Cuyahoga and its bridges, Lake Erie, Terminal Tower, Public
/// Square and Key Tower, the ballpark, the stadium and the Rock Hall. Scenery, with collision
/// you'll never reach (the bridges are drawn only).
fn backdrop(l: &mut Level) {
    use Look::*;
    const LAKE_X: f32 = -175.0;
    const FAR_S: f32 = 260.0;

    // ---- ground, the river valley and the lake ---------------------------
    bx(l, Skyline, [LAKE_X, STREET_Y - 1.0, 160.0], [FAR_S, STREET_Y, z::BLUFF_W]);
    bx(l, Skyline, [LAKE_X, STREET_Y - 1.0, z::BLUFF_E], [FAR_S, STREET_Y, -520.0]);
    // The Flats either side of the water.
    bx(l, Roof, [LAKE_X, FLATS_Y - 1.0, z::BLUFF_W], [FAR_S, FLATS_Y, z::RIVER_W]);
    bx(l, Roof, [LAKE_X, FLATS_Y - 1.0, z::RIVER_E], [FAR_S, FLATS_Y, z::BLUFF_E]);
    // The Cuyahoga, running north into the lake.
    bx(l, Water, [LAKE_X - 10.0, WATER_Y - 1.0, z::RIVER_W], [FAR_S, WATER_Y, z::RIVER_E]);
    // Lake Erie.
    bx(l, Water, [-480.0, WATER_Y - 1.0, 160.0], [LAKE_X, WATER_Y, -520.0]);
    // Breakwall out in the lake.
    bx(l, Wall, [-300.0, WATER_Y, -60.0], [-297.0, WATER_Y + 1.5, -420.0]);

    // ---- Ohio City ---------------------------------------------------------
    // The West Side Market hall and its clock tower, on your left at the start.
    bx(l, Skyline, [-50.0, BASE_Y, -30.0], [-16.0, -19.0, 4.0]);
    paint(l, Wall, [-48.0, -19.0, -28.0], [-18.0, -17.5, 2.0]);
    tower(l, Skyline, -18.5, -1.5, BASE_Y, &[(3.5, 3.5, 6.0), (4.1, 4.1, 9.5), (3.0, 3.0, 12.0), (1.8, 1.8, 14.0), (0.5, 0.5, 16.0)]);
    // Clock faces.
    for (a, b) in [([-14.4, 6.8, -3.0], [-14.3, 9.0, 0.0]), ([-20.0, 6.8, 2.6], [-17.0, 9.0, 2.7]), ([-20.0, 6.8, -5.7], [-17.0, 9.0, -5.6])] {
        paint(l, Lights, a, b);
    }
    // Brick blocks around it.
    for (a, b) in [
        ([-60.0, BASE_Y, -40.0], [-22.0, -16.0, -90.0]),
        ([16.0, BASE_Y, 10.0], [50.0, -14.0, -30.0]),
        ([14.0, BASE_Y, -40.0], [46.0, -12.0, -95.0]),
        ([-40.0, BASE_Y, 20.0], [40.0, -10.0, 50.0]),
        ([-70.0, BASE_Y, 30.0], [-30.0, -20.0, -25.0]),
        ([24.0, BASE_Y, 30.0], [70.0, -18.0, 70.0]),
        ([-36.0, BASE_Y, -95.0], [-14.0, -8.0, -114.0]),
        ([14.0, BASE_Y, -100.0], [36.0, -6.0, -114.0]),
    ] {
        bx(l, Skyline, a, b);
    }

    // ---- the river: the Flats and the bridges ------------------------------
    // Warehouses down on the Flats.
    for (a, b) in [
        ([-110.0, FLATS_Y, -120.0], [-72.0, -33.0, -129.0]),
        ([14.0, FLATS_Y, -120.0], [40.0, -34.0, -128.0]),
        ([-100.0, FLATS_Y, -161.0], [-62.0, -33.0, -170.0]),
        ([22.0, FLATS_Y, -162.0], [45.0, -32.0, -170.0]),
        ([90.0, FLATS_Y, -120.0], [130.0, -31.0, -130.0]),
    ] {
        bx(l, Skyline, a, b);
    }
    // Veterans Memorial (Detroit-Superior) Bridge, north of you: deck, piers and the steel arch.
    paint(l, Prop, [-46.0, STREET_Y - 1.4, z::BLUFF_W + 4.0], [-34.0, STREET_Y + 0.25, z::BLUFF_E - 4.0]);
    paint(l, Wall, [-46.0, STREET_Y + 0.25, z::BLUFF_W + 4.0], [-45.6, STREET_Y + 1.3, z::BLUFF_E - 4.0]);
    paint(l, Wall, [-34.4, STREET_Y + 0.25, z::BLUFF_W + 4.0], [-34.0, STREET_Y + 1.3, z::BLUFF_E - 4.0]);
    for zp in [-124.0f32, -134.0, -156.0, -166.0] {
        paint(l, Prop, [-45.0, FLATS_Y, zp - 1.0], [-35.0, STREET_Y - 1.4, zp + 1.0]);
    }
    arch(l, -45.0, -44.0, -134.0, -156.0, FLATS_Y + 1.0, 8.6, 14);
    arch(l, -36.0, -35.0, -134.0, -156.0, FLATS_Y + 1.0, 8.6, 14);
    arch(l, -45.0, -35.0, -124.0, -134.0, FLATS_Y + 2.0, 6.0, 6);
    arch(l, -45.0, -35.0, -156.0, -166.0, FLATS_Y + 2.0, 6.0, 6);
    // Hope Memorial (Lorain-Carnegie) Bridge, south of you, with the Guardians of Traffic.
    paint(l, Prop, [50.0, STREET_Y - 1.4, z::BLUFF_W + 4.0], [60.0, STREET_Y + 0.25, z::BLUFF_E - 4.0]);
    for zp in [-126.0f32, -138.0, -152.0, -164.0] {
        paint(l, Prop, [51.0, FLATS_Y, zp - 0.8], [59.0, STREET_Y - 1.4, zp + 0.8]);
    }
    arch(l, 51.0, 59.0, -138.0, -152.0, FLATS_Y + 2.0, 9.0, 10);
    for zg in [z::BLUFF_W + 2.0, z::BLUFF_E - 2.0] {
        for xg in [48.4f32, 61.6] {
            // Pylon, then the guardian on top (narrower), holding his truck.
            paint(l, Skyline, [xg - 1.3, STREET_Y, zg - 1.3], [xg + 1.3, STREET_Y + 10.0, zg + 1.3]);
            paint(l, Skyline, [xg - 0.7, STREET_Y + 10.0, zg - 0.6], [xg + 0.7, STREET_Y + 13.6, zg + 0.6]);
            paint(l, Skyline, [xg - 0.35, STREET_Y + 13.6, zg - 0.35], [xg + 0.35, STREET_Y + 14.4, zg + 0.35]);
            paint(l, Skyline, [xg - 0.5, STREET_Y + 11.6, zg - 1.2], [xg + 0.5, STREET_Y + 12.4, zg + 1.2]);
        }
    }
    // The Norfolk Southern lift bridge at the river mouth, span raised between its towers.
    for zt in [z::RIVER_W + 1.0, z::RIVER_E - 1.0] {
        paint(l, Prop, [-146.0, FLATS_Y, zt - 2.0], [-134.0, 22.0, zt + 2.0]);
    }
    paint(l, Prop, [-145.0, 16.0, z::RIVER_E + 1.0], [-135.0, 19.5, z::RIVER_W - 1.0]);

    // ---- Tower City and Terminal Tower (right, past the river) -------------
    tower(l, Skyline, 28.0, -210.0, BASE_Y, &[
        (12.0, 12.0, 70.0),
        (10.0, 10.0, 88.0),
        (7.5, 7.5, 98.0),
        (5.0, 5.0, 106.0),
        (3.0, 3.0, 112.0),
        (1.2, 1.2, 118.0),
        (0.3, 0.3, 124.0),
    ]);
    // Corner piers up the shaft.
    for (cx, cz) in [(16.0f32, -222.0f32), (40.0, -222.0), (16.0, -198.0), (40.0, -198.0)] {
        paint(l, Wall, [cx - 0.6, Y5 + 6.5, cz - 0.6], [cx + 0.6, 70.5, cz + 0.6]);
    }
    // The rest of the Tower City block behind it.
    bx(l, Skyline, [40.0, BASE_Y, -226.0], [70.0, 22.0, -250.0]);

    // ---- Public Square (left) ----------------------------------------------
    let sq_y = STREET_Y + 0.05;
    for (x0, x1) in [(-90.0f32, -57.0f32), (-51.0, -18.0)] {
        for (z0, z1) in [(-237.0f32, -263.0f32), (-269.0, -295.0)] {
            bx(l, Green, [x0, STREET_Y, z0], [x1, sq_y, z1]);
        }
    }
    // Soldiers' and Sailors' Monument, southeast quadrant.
    bx(l, Skyline, [-40.0, sq_y, -276.0], [-28.0, -25.0, -288.0]);
    bx(l, Skyline, [-35.2, -25.0, -280.8], [-32.8, -1.0, -283.2]);
    // Lady Liberty on top, shield and all.
    bx(l, Skyline, [-34.6, -1.0, -281.4], [-33.4, 2.4, -282.6]);
    bx(l, Skyline, [-34.3, 2.4, -281.7], [-33.7, 3.1, -282.3]);
    // Key Tower, north side of the square, and its hotel wing.
    tower(l, Skyline, -116.0, -266.0, BASE_Y, &[
        (12.0, 12.0, 70.0),
        (10.5, 10.5, 92.0),
        (9.0, 9.0, 108.0),
        (7.5, 7.5, 120.0),
        (5.5, 5.5, 128.0),
        (3.5, 3.5, 134.0),
        (1.8, 1.8, 139.0),
        (0.4, 0.4, 152.0),
    ]);
    bx(l, Skyline, [-128.0, BASE_Y, -282.0], [-104.0, -4.0, -300.0]);
    // Old Stone Church, northwest corner: nave and tower.
    bx(l, Skyline, [-104.0, BASE_Y, -226.0], [-92.0, -20.0, -242.0]);
    tower(l, Skyline, -101.5, -228.5, -20.0, &[(2.5, 2.5, -4.0), (1.2, 1.2, 2.0)]);
    // 200 Public Square, southeast corner, on Euclid (right).
    bx(l, Skyline, [14.0, BASE_Y, -282.0], [46.0, 50.0, -316.0]);
    bx(l, Skyline, [16.0, 50.0, -284.0], [44.0, 80.0, -314.0]);
    bx(l, Skyline, [20.0, 80.0, -288.0], [40.0, 90.0, -310.0]);
    // The Sherwin-Williams tower and the rest of the square's west side (left, by the river).
    bx(l, Skyline, [-60.0, BASE_Y, -180.0], [-20.0, 20.0, -224.0]);
    tower(l, Skyline, -78.0, -200.0, BASE_Y, &[(11.0, 14.0, 60.0), (9.0, 12.0, 76.0), (6.0, 9.0, 82.0)]);

    // ---- Euclid Ave and downtown -------------------------------------------
    for (a, b) in [
        ([-60.0, BASE_Y, -320.0], [-22.0, 40.0, -350.0]),
        ([-60.0, BASE_Y, -355.0], [-22.0, 15.0, -390.0]),
        ([-100.0, BASE_Y, -310.0], [-70.0, 55.0, -340.0]),
        ([-100.0, BASE_Y, -350.0], [-70.0, 30.0, -380.0]),
        ([20.0, BASE_Y, -232.0], [60.0, 25.0, -270.0]),
        ([16.0, BASE_Y, -326.0], [50.0, 6.0, -356.0]), // low, so East 4th gets the sun
        ([16.0, BASE_Y, -362.0], [40.0, 4.0, -392.0]),
        ([-40.0, BASE_Y, -410.0], [-8.0, 45.0, -440.0]),
        ([8.0, BASE_Y, -405.0], [40.0, 20.0, -445.0]),
        ([-90.0, BASE_Y, -400.0], [-50.0, 25.0, -440.0]),
        ([-30.0, BASE_Y, -460.0], [30.0, 60.0, -490.0]),
        ([-60.0, BASE_Y, -440.0], [-40.0, 10.0, -470.0]),
    ] {
        bx(l, Skyline, a, b);
    }

    // ---- Progressive Field (right) and the arena ---------------------------
    bx(l, Green, [95.0, STREET_Y, -255.0], [160.0, STREET_Y + 0.05, -325.0]);
    // Grandstands down both baselines, three tiers each.
    for (i, (x0, x1)) in [(86.0f32, 95.0f32), (78.0, 86.0), (70.0, 78.0)].iter().enumerate() {
        bx(l, Skyline, [*x0, STREET_Y, -240.0], [*x1, -24.0 + 6.0 * i as f32, -340.0]);
    }
    for (i, (z0, z1)) in [(-325.0f32, -330.0f32), (-330.0, -335.0), (-335.0, -340.0)].iter().enumerate() {
        bx(l, Skyline, [70.0, STREET_Y, *z0], [170.0, -24.0 + 6.0 * i as f32, *z1]);
    }
    // Outfield wall and the big scoreboard.
    bx(l, Wall, [160.0, STREET_Y, -240.0], [161.0, STREET_Y + 3.0, -325.0]);
    bx(l, Wall, [95.0, STREET_Y, -240.0], [161.0, STREET_Y + 3.0, -241.0]);
    bx(l, Skyline, [148.0, STREET_Y, -236.0], [170.0, 2.0, -240.0]);
    // The toothbrush light towers.
    for (lx, lz) in [(68.0f32, -238.0f32), (68.0, -342.0), (172.0, -342.0), (120.0, -236.0)] {
        bx(l, Prop, [lx - 0.6, STREET_Y, lz - 0.6], [lx + 0.6, 10.0, lz + 0.6]);
        paint(l, Lights, [lx - 3.0, 10.0, lz - 0.5], [lx + 3.0, 13.0, lz + 0.5]);
    }
    // Rocket Arena.
    bx(l, Skyline, [75.0, BASE_Y, -350.0], [125.0, -4.0, -392.0]);

    // ---- the lakefront (left) ----------------------------------------------
    // Huntington Bank Field: field and four stands, open corners.
    bx(l, Green, [-160.0, STREET_Y, -235.0], [-124.0, STREET_Y + 0.05, -285.0]);
    bx(l, Skyline, [-172.0, STREET_Y, -235.0], [-160.0, -8.0, -285.0]);
    bx(l, Skyline, [-124.0, STREET_Y, -235.0], [-112.0, -8.0, -285.0]);
    bx(l, Skyline, [-160.0, STREET_Y, -222.0], [-124.0, -12.0, -235.0]);
    bx(l, Skyline, [-160.0, STREET_Y, -285.0], [-124.0, -12.0, -298.0]);
    // Great Lakes Science Center and its wind turbine.
    bx(l, Skyline, [-170.0, BASE_Y, -325.0], [-146.0, -18.0, -355.0]);
    bx(l, Prop, [-142.4, STREET_Y, -340.4], [-141.6, 6.0, -339.6]);
    paint(l, Prop, [-143.0, 5.4, -341.0], [-141.0, 6.6, -339.0]);
    paint(l, Wall, [-142.2, -2.0, -340.2], [-141.8, 14.0, -339.8]);
    paint(l, Wall, [-142.2, 5.8, -348.0], [-141.8, 6.2, -332.0]);
    // Rock & Roll Hall of Fame: the glass pyramid, its tower and the drum.
    tower(l, Skyline, -150.0, -392.0, STREET_Y, &[(18.0, 18.0, -24.0), (14.0, 14.0, -18.0), (10.0, 10.0, -12.0), (6.0, 6.0, -6.0), (2.5, 2.5, -2.0)]);
    bx(l, Skyline, [-156.0, STREET_Y, -406.0], [-148.0, 8.0, -414.0]);
    bx(l, Skyline, [-136.0, STREET_Y, -372.0], [-124.0, -18.0, -384.0]);
    bx(l, Wall, [-172.0, STREET_Y, -360.0], [-128.0, -29.0, -420.0]);
}
