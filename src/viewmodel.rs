//! The runner's first-person body: an original character, built and animated in code (no
//! Mirror's Edge assets), so it works everywhere, the browser included.
//!
//! - **Arms** on their own render layer, drawn by a second camera over the world with its depth
//!   cleared, so they never clip into walls and don't stretch with the sprint FOV kick. Each arm
//!   is a real chain: shoulder, upper arm, elbow, forearm, wrist and a hand with four two-joint
//!   fingers and a thumb. Each move sets where the hand goes, how it's turned and how tightly it
//!   grips (flat palm on a wall, fists running, wrapped round a bar). The elbow is solved with
//!   two-bone IK, and the wrist bends no further than a real one, so the arm always connects.
//! - **Legs and torso** in the world, seen when you look down: hips over the feet, the legs
//!   stepping in time with the camera's footstep phase, tucked in the air, out in front on a
//!   slide, hanging under a zipline or bar, a leg out for the kicks.
//!
//! Hand poses are in camera space; a few (ledge hang) are anchored in the world.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use faith_move::{Controller, Event as MoveEvent, MeleeKind, MeleePhase, State as MoveState, TraverseKind};

use crate::{Game, PlayerCamera};

pub const VIEWMODEL_LAYER: usize = 1;

#[derive(Component)]
pub struct ViewmodelCamera;

/// The body's roots (both arms and the lower body): hidden while the Mirror's Edge body is on.
#[derive(Component)]
pub struct Arm;

// ------------------------------------------------------------------ proportions (metres)

const UPPER_ARM: f32 = 0.29;
const FOREARM: f32 = 0.27;
/// Shoulder, in camera space (x is multiplied by the side).
const SHOULDER: Vec3 = Vec3::new(0.175, -0.19, 0.07);
const THIGH: f32 = 0.45;
const SHIN: f32 = 0.45;
const ANKLE: f32 = 0.07;
const HIP_HEIGHT: f32 = 0.96;
const HIP_WIDTH: f32 = 0.095;

/// Index to little finger: (across the hand toward the little finger, base length, tip length).
const FINGERS: [(f32, f32, f32); 4] = [(0.030, 0.044, 0.036), (0.010, 0.048, 0.040), (-0.010, 0.045, 0.037), (-0.029, 0.036, 0.031)];
const FINGER_BASE_Z: f32 = -0.092;

// ------------------------------------------------------------------ rig

struct ArmRig {
    upper: Entity,
    sleeve: Entity,
    fore: Entity,
    elbow: Entity,
    hand: Entity,
    /// Per finger: (base joint, middle joint).
    fingers: [(Entity, Entity); 4],
    thumb: (Entity, Entity),
}

struct LegRig {
    thigh: Entity,
    shin: Entity,
    knee: Entity,
    foot: Entity,
}

#[derive(Resource)]
pub struct Body {
    arms: [ArmRig; 2],
    legs: [LegRig; 2],
    lower: Entity,
    torso: Entity,
    hips: Entity,
}

/// What each arm is doing now (smoothed toward the move's pose), and the legs' state.
#[derive(Resource, Default)]
pub struct ViewmodelState {
    sway: Vec2,
    last_yaw: f32,
    last_pitch: f32,
    dodge: Option<(Vec3, f32)>,
    time: f32,
    hand: [(Vec3, Quat, f32); 2],
    started: bool,
    body_yaw: f32,
    feet: [Vec3; 2],
    hip_drop: f32,
}

// ------------------------------------------------------------------ setup

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    let mat = |c: Color, rough: f32| StandardMaterial { base_color: c, perceptual_roughness: rough, ..default() };
    let skin = materials.add(mat(Color::srgb(0.76, 0.56, 0.45), 0.65));
    let shirt = materials.add(mat(Color::srgb(0.13, 0.15, 0.20), 0.9));
    let glove = materials.add(mat(Color::srgb(0.10, 0.10, 0.11), 0.75));
    let red = materials.add(StandardMaterial {
        base_color: Color::srgb(0.86, 0.10, 0.07),
        emissive: LinearRgba::rgb(0.12, 0.0, 0.0),
        perceptual_roughness: 0.5,
        ..default()
    });
    let pants = materials.add(mat(Color::srgb(0.20, 0.21, 0.24), 0.9));
    let shoe = materials.add(mat(Color::srgb(0.93, 0.93, 0.95), 0.6));

    // Unit-length limbs along +Y from the origin, scaled to length when posed.
    let limb = |meshes: &mut Assets<Mesh>, r0: f32, r1: f32| {
        meshes.add(Mesh::from(ConicalFrustum { radius_top: r1, radius_bottom: r0, height: 1.0 }).translated_by(Vec3::Y * 0.5))
    };
    let upper_m = limb(&mut meshes, 0.046, 0.038);
    let sleeve_m = limb(&mut meshes, 0.054, 0.050);
    let fore_m = limb(&mut meshes, 0.037, 0.028);
    let elbow_m = meshes.add(Sphere::new(0.039));
    let wrist_m = meshes.add(Cylinder::new(0.029, 0.032));
    // The palm: a flattened rounded block (a capsule squashed into a mitten shape).
    let palm_m = meshes.add(Mesh::from(Capsule3d::new(0.030, 0.034)).scaled_by(Vec3::new(1.3, 1.0, 0.48)).rotated_by(Quat::from_rotation_x(FRAC_PI_2)));
    let knuckle_m = meshes.add(Mesh::from(Capsule3d::new(0.012, 0.062)).rotated_by(Quat::from_rotation_z(FRAC_PI_2)));
    let thigh_m = limb(&mut meshes, 0.085, 0.060);
    let shin_m = limb(&mut meshes, 0.058, 0.042);
    let knee_m = meshes.add(Sphere::new(0.062));
    let shoe_m = meshes.add(Cuboid::new(0.105, 0.075, 0.27));
    let sole_m = meshes.add(Cuboid::new(0.11, 0.022, 0.275));
    let torso_m = limb(&mut meshes, 0.125, 0.11);
    let hips_m = meshes.add(Capsule3d::new(0.105, 0.11));

    let cam = commands
        .spawn((
            ViewmodelCamera,
            Camera3d::default(),
            Camera { order: 1, clear_color: ClearColorConfig::None, ..default() },
            // Fixed FOV: the arms shouldn't warp when sprinting widens the world FOV.
            Projection::Perspective(PerspectiveProjection { fov: 62f32.to_radians(), near: 0.01, ..default() }),
            Transform::default(),
            layer.clone(),
        ))
        .id();

    // Soft fill so the arms read even when the sun is behind them.
    commands.spawn((
        DirectionalLight { illuminance: 2500.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-1.0, 2.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
        layer.clone(),
    ));

    let part = |commands: &mut Commands, parent: Entity, mesh: &Handle<Mesh>, m: &Handle<StandardMaterial>, tf: Transform, layer: &RenderLayers| {
        let e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(m.clone()), tf, layer.clone(), NotShadowCaster)).id();
        commands.entity(parent).add_child(e);
        e
    };
    let joint = |commands: &mut Commands, parent: Entity, tf: Transform, layer: &RenderLayers| {
        let e = commands.spawn((tf, Visibility::default(), layer.clone())).id();
        commands.entity(parent).add_child(e);
        e
    };

    let mut arms = vec![];
    for side in [-1.0f32, 1.0] {
        let root = commands.spawn((Arm, Transform::default(), Visibility::default(), layer.clone())).id();
        commands.entity(cam).add_child(root);
        let upper = part(&mut commands, root, &upper_m, &skin, Transform::default(), &layer);
        let sleeve = part(&mut commands, root, &sleeve_m, &shirt, Transform::default(), &layer);
        let fore = part(&mut commands, root, &fore_m, &skin, Transform::default(), &layer);
        let elbow = part(&mut commands, root, &elbow_m, &skin, Transform::default(), &layer);
        // The hand: wrist at its origin, fingers toward -Z, palm facing -Y, thumb toward -x*side.
        let hand = joint(&mut commands, root, Transform::default(), &layer);
        part(&mut commands, hand, &wrist_m, &red, Transform::from_xyz(0.0, 0.0, 0.012).with_rotation(Quat::from_rotation_x(FRAC_PI_2)), &layer);
        part(&mut commands, hand, &palm_m, &glove, Transform::from_xyz(0.0, 0.0, -0.052), &layer);
        part(&mut commands, hand, &knuckle_m, &glove, Transform::from_xyz(0.0, 0.002, -0.088), &layer);
        let along_z = Quat::from_rotation_x(FRAC_PI_2);
        let mut fingers = vec![];
        for (i, &(across, l0, l1)) in FINGERS.iter().enumerate() {
            let r = if i == 3 { 0.0085 } else { 0.0098 };
            let seg0 = meshes.add(Capsule3d::new(r, l0 - 2.0 * r));
            let seg1 = meshes.add(Capsule3d::new(r * 0.92, l1 - 2.0 * r));
            let x = -side * across;
            let base = joint(&mut commands, hand, Transform::from_xyz(x, -0.002, FINGER_BASE_Z), &layer);
            part(&mut commands, base, &seg0, &glove, Transform::from_xyz(0.0, 0.0, -l0 * 0.5).with_rotation(along_z), &layer);
            let mid = joint(&mut commands, base, Transform::from_xyz(0.0, 0.0, -l0), &layer);
            part(&mut commands, mid, &seg1, &skin, Transform::from_xyz(0.0, 0.0, -l1 * 0.5).with_rotation(along_z), &layer);
            fingers.push((base, mid));
        }
        let t0 = meshes.add(Capsule3d::new(0.0115, 0.022));
        let t1 = meshes.add(Capsule3d::new(0.0105, 0.016));
        let tb = joint(&mut commands, hand, Transform::from_xyz(-side * 0.040, -0.008, -0.030), &layer);
        part(&mut commands, tb, &t0, &glove, Transform::from_xyz(0.0, 0.0, -0.022).with_rotation(along_z), &layer);
        let tm = joint(&mut commands, tb, Transform::from_xyz(0.0, 0.0, -0.044), &layer);
        part(&mut commands, tm, &t1, &skin, Transform::from_xyz(0.0, 0.0, -0.018).with_rotation(along_z), &layer);
        arms.push(ArmRig { upper, sleeve, fore, elbow, hand, fingers: [fingers[0], fingers[1], fingers[2], fingers[3]], thumb: (tb, tm) });
    }

    // Lower body, in the world (layer 0): one root at the origin, parts placed in world space.
    let world = RenderLayers::layer(0);
    let lower = commands.spawn((Arm, Transform::default(), Visibility::default(), world.clone())).id();
    let torso = part(&mut commands, lower, &torso_m, &shirt, Transform::default(), &world);
    let hips = part(&mut commands, lower, &hips_m, &pants, Transform::default(), &world);
    let mut legs = vec![];
    for _ in 0..2 {
        let thigh = part(&mut commands, lower, &thigh_m, &pants, Transform::default(), &world);
        let shin = part(&mut commands, lower, &shin_m, &pants, Transform::default(), &world);
        let knee = part(&mut commands, lower, &knee_m, &pants, Transform::default(), &world);
        let foot = joint(&mut commands, lower, Transform::default(), &world);
        part(&mut commands, foot, &shoe_m, &shoe, Transform::from_xyz(0.0, 0.038, -0.05), &world);
        part(&mut commands, foot, &sole_m, &red, Transform::from_xyz(0.0, 0.011, -0.05), &world);
        legs.push(LegRig { thigh, shin, knee, foot });
    }
    let mut legs = legs.into_iter();
    let mut arms = arms.into_iter();
    commands.insert_resource(Body {
        arms: [arms.next().unwrap(), arms.next().unwrap()],
        legs: [legs.next().unwrap(), legs.next().unwrap()],
        lower,
        torso,
        hips,
    });
    commands.insert_resource(ViewmodelState::default());
}

// ------------------------------------------------------------------ hand poses

/// Forearm orientation from yaw (turn toward -X is positive), pitch (hand up is positive) and
/// roll (twist; positive rolls the palm toward +X; for the right arm, inward).
fn orient(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll)
}

struct Pose {
    /// The wrist, in camera space.
    pos: Vec3,
    rot: Quat,
    /// How quickly to blend toward this pose.
    rate: f32,
    /// 0 open, flat hand; 1 a fist or a hand wrapped round a bar.
    grip: f32,
}

fn pose(pos: Vec3, yaw: f32, pitch: f32, roll: f32, grip: f32) -> Pose {
    Pose { pos, rot: orient(yaw, pitch, roll), rate: 16.0, grip }
}

/// Arm pumping in the running gait.
fn run_pose(s: f32, phase: f32, amount: f32) -> Pose {
    // Opposite arm to leg: offset the two arms by half a cycle.
    let sw = (phase + if s > 0.0 { 0.0 } else { PI }).sin() * amount;
    let pos = Vec3::new(s * (0.24 - 0.03 * sw), -0.30 + 0.07 * sw + 0.02 * sw.abs(), -0.40 - 0.11 * sw);
    pose(pos, -s * 0.35, 0.45 + 0.45 * sw, s * 0.75, 0.8)
}

/// Down by your sides, just under the bottom of the screen.
fn rest_pose(s: f32, time: f32) -> Pose {
    let breathe = (time * 1.6).sin() * 0.006;
    pose(Vec3::new(s * 0.27, -0.50 + breathe, -0.26), -s * 0.25, 0.35, s * 0.7, 0.45)
}

#[allow(clippy::too_many_arguments)]
fn target(s: f32, c: &Controller, step_phase: f32, gait: f32, time: f32, cam: &Transform, dodge: Option<(Vec3, f32)>) -> Pose {
    let tu = &c.tuning;
    let speed = c.horizontal_speed();
    let right = Vec3::new(c.yaw.cos(), 0.0, -c.yaw.sin());
    let to_cam = |world: Vec3| cam.rotation.inverse() * (world - cam.translation);
    let rot_to_cam = |world_rot: Quat| cam.rotation.inverse() * world_rot;

    // Punches: the hand that throws it (left or right), the other guards.
    if let Some(m) = &c.melee {
        let hand = if m.left { -1.0 } else { 1.0 };
        match m.kind {
            MeleeKind::Punch | MeleeKind::Crouch => {
                return if s == hand {
                    match m.phase {
                        MeleePhase::Start => Pose { rate: 30.0, ..pose(Vec3::new(s * 0.20, -0.25, -0.30), -s * 0.15, 0.25, s * 1.3, 1.0) },
                        _ => Pose { rate: 34.0, ..pose(Vec3::new(s * 0.05, -0.13, -0.60), s * 0.05, 0.05, s * 1.45, 1.0) },
                    }
                } else {
                    pose(Vec3::new(s * 0.17, -0.24, -0.34), -s * 0.2, 0.6, s * 1.2, 1.0)
                };
            }
            // Kicks: arms out for balance.
            _ => return pose(Vec3::new(s * 0.36, -0.22, -0.36), -s * 0.45, 0.25, s * 1.1, 0.5),
        }
    }

    match c.state {
        MoveState::Ground => {
            let amount = (speed / tu.sprint_speed).clamp(0.0, 1.0) * gait;
            if speed < 0.6 {
                rest_pose(s, time)
            } else {
                run_pose(s, step_phase, 0.35 + 0.65 * amount)
            }
        }
        MoveState::Air => {
            if c.is_coiled() {
                return pose(Vec3::new(s * 0.19, -0.25, -0.42), -s * 0.2, 0.35, s * 0.95, 0.75);
            }
            let fall = (-c.vel.y / 8.0).clamp(0.0, 1.0);
            let mut p = pose(Vec3::new(s * (0.30 + 0.07 * fall), -0.24 + 0.10 * fall, -0.40), -s * 0.3, 0.5 + 0.3 * fall, s * 1.05, 0.35 - 0.25 * fall);
            if let Some((dir, age)) = dodge {
                // Swing both arms toward the dodge.
                let k = (1.0 - age / 0.4).clamp(0.0, 1.0);
                p.pos.x += dir.dot(right) * 0.14 * k;
                p.rate = 22.0;
            }
            p
        }
        MoveState::Slide { .. } => {
            if s < 0.0 {
                // Trailing hand down by the floor, fingers spread.
                pose(Vec3::new(-0.34, -0.30, -0.36), 0.45, -0.35, -0.6, 0.1)
            } else {
                pose(Vec3::new(0.24, -0.26, -0.42), -0.3, 0.4, 0.8, 0.7)
            }
        }
        MoveState::Roll { .. } => pose(Vec3::new(s * 0.15, -0.42, -0.30), -s * 0.4, -0.2, s * 0.8, 0.6),
        MoveState::Stunned { .. } => pose(Vec3::new(s * 0.22, -0.32, -0.40), -s * 0.2, -0.15, s * 0.3, 0.3),
        MoveState::WallRun { normal, t } => {
            // The wall is on the side the normal points away from.
            let wall_side = -normal.dot(right).signum();
            if s == wall_side {
                // Palm flat against the wall, slipping along it.
                let slip = ((t * 7.0).sin() * 0.03).abs();
                pose(Vec3::new(s * 0.40, -0.08, -0.40 + slip), -s * 0.3, 0.35, s * 1.5, 0.05)
            } else {
                run_pose(s, t * 9.0, 0.8)
            }
        }
        MoveState::WallClimb { t, .. } => {
            // Hand over hand up the wall, palms flat.
            let reach = (t * 16.0 + if s > 0.0 { 0.0 } else { PI }).sin();
            pose(Vec3::new(s * 0.17, 0.0 + 0.12 * reach.max(0.0), -0.34 - 0.04 * reach), -s * 0.15, 1.25, s * 0.25, 0.15)
        }
        MoveState::WallClimbTurned { .. } => pose(Vec3::new(s * 0.38, -0.14, -0.38), -s * 0.35, 0.25, s * 1.3, 0.2),
        MoveState::LedgeHang { normal, ledge_y, turned } => {
            if turned {
                return rest_pose(s, time);
            }
            // Anchored in the world: fingers hooked over the lip of the ledge.
            let along = Vec3::new(normal.z, 0.0, -normal.x);
            let along = if along.dot(right) < 0.0 { -along } else { along };
            let grip = Vec3::new(c.feet.x, ledge_y - 0.05, c.feet.z) + -normal * (tu.half_width - 0.02) + along * s * 0.21;
            let dir = (-normal * 0.55 + Vec3::Y * 0.85).normalize();
            let world_rot = Quat::from_rotation_arc(Vec3::NEG_Z, dir) * Quat::from_rotation_z(s * 0.2);
            Pose { pos: to_cam(grip), rot: rot_to_cam(world_rot), rate: 30.0, grip: 0.75 }
        }
        MoveState::Traverse(tr) => {
            let k = tr.t.clamp(0.0, 1.0);
            match tr.kind {
                TraverseKind::Vault => vault_pose(s, k),
                TraverseKind::Mantle | TraverseKind::PullUp => {
                    // Both palms pressing down on the top as you come over.
                    pose(Vec3::new(s * 0.21, -0.22 - 0.12 * k, -0.44 + 0.18 * k), -s * 0.2, -0.35, s * 0.15, 0.1)
                }
                TraverseKind::SpringBoard => pose(Vec3::new(s * 0.30, -0.18, -0.40), -s * 0.25, 0.5, s * 0.9, 0.6),
            }
        }
        MoveState::Vault(v) => {
            let k = (v.t / v.duration()).clamp(0.0, 1.0);
            if v.onto() {
                pose(Vec3::new(s * 0.21, -0.22 - 0.12 * k, -0.44 + 0.18 * k), -s * 0.2, -0.35, s * 0.15, 0.1)
            } else {
                vault_pose(s, k)
            }
        }
        // Arms out to the sides, lying on your back.
        MoveState::LayOnGround { .. } | MoveState::SoftLand { .. } | MoveState::Stumble { .. } => {
            pose(Vec3::new(s * 0.40, -0.30, -0.25), -s * 0.4, -0.2, s * 0.8, 0.2)
        }
        // Shoulder into the door; reaching for someone; stepping up.
        MoveState::Barge { .. } | MoveState::AirBarge { .. } => pose(Vec3::new(s * 0.16, -0.18, -0.30), -s * 0.3, 0.4, s * 1.1, 1.0),
        MoveState::Vertigo { .. } | MoveState::Takedown { .. } | MoveState::StepUp { .. } | MoveState::RumpSlide { .. } | MoveState::GrabTransfer { .. } => {
            pose(Vec3::new(s * 0.20, -0.18, -0.32), -s * 0.3, 0.3, s * 0.7, 0.5)
        }
        MoveState::Balance { lean, .. } => {
            // Arms out for balance, dipping on the side you're leaning to, fingers spread.
            pose(Vec3::new(s * 0.44, -0.16 - 0.10 * lean * s, -0.32), -s * 0.55, 0.15, s * 1.5, 0.15)
        }
        // Both hands overhead, wrapped round the cable / bar / rung.
        MoveState::ZipLine { .. } | MoveState::Swing { .. } | MoveState::SwingJump { .. } | MoveState::IntoClimb { .. } | MoveState::Climb { .. } | MoveState::ClimbExit { .. } => {
            pose(Vec3::new(s * 0.11, 0.24, -0.26), -s * 0.1, 1.45, s * 0.1, 0.95)
        }
    }
}

/// Over a rail: the lead (left) hand plants flat and slides back past you; the other swings out.
fn vault_pose(s: f32, k: f32) -> Pose {
    if s < 0.0 {
        pose(Vec3::new(-0.13, -0.24 - 0.06 * k, -0.46 + 0.26 * k), 0.3, -0.35, -0.4, 0.05)
    } else {
        pose(Vec3::new(0.36, -0.12, -0.38), -0.4, 0.35, 1.0, 0.4)
    }
}

// ------------------------------------------------------------------ IK

/// Two-bone IK: from `root` to `target` with bones `l1`, `l2`, bending toward `pole`.
/// Returns (middle joint, end), the end pulled in if the target is out of reach.
fn two_bone(root: Vec3, target: Vec3, l1: f32, l2: f32, pole: Vec3) -> (Vec3, Vec3) {
    let d = target - root;
    let len = d.length().max(1e-4);
    let dir = d / len;
    let dist = len.clamp((l1 - l2).abs() + 0.01, (l1 + l2) * 0.995);
    let end = root + dir * dist;
    let cos_a = ((l1 * l1 + dist * dist - l2 * l2) / (2.0 * l1 * dist)).clamp(-1.0, 1.0);
    let sin_a = (1.0 - cos_a * cos_a).sqrt();
    let mut side = pole - dir * pole.dot(dir);
    if side.length_squared() < 1e-6 {
        side = dir.any_orthonormal_vector();
    }
    let mid = root + dir * (cos_a * l1) + side.normalize() * (sin_a * l1);
    (mid, end)
}

/// A unit limb (along +Y from its base) stretched from `a` to `b`.
fn limb_tf(a: Vec3, b: Vec3) -> Transform {
    let d = b - a;
    let len = d.length().max(1e-4);
    Transform { translation: a, rotation: Quat::from_rotation_arc(Vec3::Y, d / len), scale: Vec3::new(1.0, len, 1.0) }
}

/// Limit how far the hand bends off the forearm (a wrist only goes so far).
fn wrist_limit(hand: Quat, forearm_dir: Vec3, max: f32) -> Quat {
    let fwd = hand * Vec3::NEG_Z;
    let ang = fwd.angle_between(forearm_dir);
    if ang <= max {
        return hand;
    }
    let back = Quat::from_rotation_arc(fwd, forearm_dir);
    Quat::IDENTITY.slerp(back, (ang - max) / ang) * hand
}

// ------------------------------------------------------------------ animate

#[allow(clippy::type_complexity)]
pub fn animate(
    time: Res<Time>,
    game: Res<Game>,
    body: Option<Res<Body>>,
    mut state: ResMut<ViewmodelState>,
    main_cam: Query<&Transform, (With<PlayerCamera>, Without<ViewmodelCamera>)>,
    mut vm_cam: Query<(&mut Transform, &mut Projection), (With<ViewmodelCamera>, Without<PlayerCamera>)>,
    mut parts: Query<&mut Transform, (Without<PlayerCamera>, Without<ViewmodelCamera>)>,
    mut vis: Query<&mut Visibility, With<Arm>>,
) {
    let Some(body) = body else { return };
    let dt = time.delta_secs().min(0.1);
    let Ok(cam) = main_cam.single() else { return };
    if let Ok((mut vt, mut proj)) = vm_cam.single_mut() {
        *vt = *cam;
        // The arms use their own fixed FOV (the Mirror's Edge body overrides this with the world
        // projection when it's active).
        *proj = Projection::Perspective(PerspectiveProjection { fov: 62f32.to_radians(), near: 0.01, ..default() });
    }
    let c = &game.ctrl;
    let shot = game.shot;
    state.time += dt;

    for e in &c.events {
        if let MoveEvent::Dodge { dir } | MoveEvent::WallRunDodge { dir } = *e {
            state.dodge = Some((dir, 0.0));
        }
    }
    if let Some((_, age)) = &mut state.dodge {
        *age += dt;
    }
    if state.dodge.is_some_and(|(_, a)| a > 0.5) {
        state.dodge = None;
    }

    // Arms lag behind fast looks (sway), then catch up.
    let dyaw = shot.view.yaw - state.last_yaw;
    let dpitch = shot.view.pitch - state.last_pitch;
    state.last_yaw = shot.view.yaw;
    state.last_pitch = shot.view.pitch;
    let big_move = dyaw.abs() > 1.0 || dpitch.abs() > 1.0; // teleports, 180s, roll flips
    let target_sway = if big_move || dt <= 0.0 { Vec2::ZERO } else { Vec2::new(dyaw, dpitch).clamp_length_max(0.08) * 1.6 };
    let cur = state.sway;
    state.sway = cur + (target_sway - cur) * (1.0 - (-12.0 * dt).exp());
    let sway_pos = Vec3::new(state.sway.x * 0.35, -state.sway.y * 0.3, 0.0);
    let sway_rot = Quat::from_rotation_y(state.sway.x * 0.4);

    // ---- arms
    for (i, s) in [-1.0f32, 1.0].into_iter().enumerate() {
        let p = target(s, c, shot.step_phase, shot.gait, state.time, cam, state.dodge);
        let k = 1.0 - (-p.rate * dt).exp();
        let (pos, rot, grip) = if state.started { state.hand[i] } else { (p.pos, p.rot, p.grip) };
        let pos = pos.lerp(p.pos, k);
        let rot = rot.slerp(p.rot, k);
        let grip = grip + (p.grip - grip) * (1.0 - (-14.0 * dt).exp());
        state.hand[i] = (pos, rot, grip);

        let wrist_target = pos + sway_pos;
        let shoulder = Vec3::new(s * SHOULDER.x, SHOULDER.y, SHOULDER.z);
        // Elbows out and down, as when you run and reach.
        let pole = Vec3::new(s * 0.75, -1.0, 0.35);
        let (elbow, wrist) = two_bone(shoulder, wrist_target, UPPER_ARM, FOREARM, pole);
        let fore_dir = (wrist - elbow).normalize_or_zero();
        let hand_rot = wrist_limit(sway_rot * rot, fore_dir, 0.95);

        let rig = &body.arms[i];
        let set = |parts: &mut Query<&mut Transform, (Without<PlayerCamera>, Without<ViewmodelCamera>)>, e: Entity, tf: Transform| {
            if let Ok(mut t) = parts.get_mut(e) {
                *t = tf;
            }
        };
        set(&mut parts, rig.upper, limb_tf(shoulder, elbow));
        // The sleeve covers the top of the upper arm.
        set(&mut parts, rig.sleeve, limb_tf(shoulder, shoulder.lerp(elbow, 0.42)));
        set(&mut parts, rig.fore, limb_tf(elbow, wrist + fore_dir * 0.012));
        set(&mut parts, rig.elbow, Transform::from_translation(elbow));
        set(&mut parts, rig.hand, Transform::from_translation(wrist).with_rotation(hand_rot));
        // Fingers curl toward the palm (-Y): a rotation about X by a negative angle.
        for (f, &(base, mid)) in rig.fingers.iter().enumerate() {
            let spread = (f as f32 - 1.5) * 0.06 * (1.0 - grip) * -s;
            let curl0 = -(0.15 + 1.35 * grip) - 0.05 * f as f32 * grip;
            let curl1 = -(0.15 + 1.45 * grip);
            if let Ok(mut t) = parts.get_mut(base) {
                t.rotation = Quat::from_rotation_y(spread) * Quat::from_rotation_x(curl0);
            }
            if let Ok(mut t) = parts.get_mut(mid) {
                t.rotation = Quat::from_rotation_x(curl1);
            }
        }
        // The thumb lies along the side of the hand, and folds across the fingers in a grip.
        if let Ok(mut t) = parts.get_mut(rig.thumb.0) {
            t.rotation = Quat::from_rotation_y(s * (0.55 - 0.35 * grip)) * Quat::from_rotation_z(s * 0.5) * Quat::from_rotation_x(-0.25 - 0.55 * grip);
        }
        if let Ok(mut t) = parts.get_mut(rig.thumb.1) {
            t.rotation = Quat::from_rotation_x(-0.2 - 0.8 * grip);
        }
    }
    state.started = true;

    // ---- legs and torso, in the world
    let hidden = matches!(c.state, MoveState::Roll { .. } | MoveState::LayOnGround { .. } | MoveState::Takedown { .. } | MoveState::SoftLand { .. });
    if let Ok(mut v) = vis.get_mut(body.lower) {
        // (The Mirror's Edge body sets this every frame too; it runs after us and wins.)
        *v = if hidden { Visibility::Hidden } else { Visibility::Inherited };
    }
    if hidden {
        return;
    }
    legs(&body, &mut state, c, shot.step_phase, dt, &mut parts);
}

fn forward(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

#[allow(clippy::type_complexity)]
fn legs(body: &Body, state: &mut ViewmodelState, c: &Controller, phase: f32, dt: f32, parts: &mut Query<&mut Transform, (Without<PlayerCamera>, Without<ViewmodelCamera>)>) {
    let vel = Vec3::new(c.vel.x, 0.0, c.vel.z);
    let speed = vel.length();
    let on_ground = matches!(c.state, MoveState::Ground | MoveState::WallRun { .. } | MoveState::Balance { .. } | MoveState::Barge { .. } | MoveState::Stumble { .. });

    // The body faces where you're running (within 100 degrees of the view; past that you're
    // backing up), or where you look.
    let mut want = c.yaw;
    if speed > 0.6 && on_ground {
        let vy = (-vel.x).atan2(-vel.z);
        let d = (vy - c.yaw + PI).rem_euclid(2.0 * PI) - PI;
        want = if d.abs() < 1.75 { vy } else { vy + PI };
    }
    if let MoveState::Slide { .. } = c.state {
        want = (-vel.x).atan2(-vel.z);
    }
    let d = (want - state.body_yaw + PI).rem_euclid(2.0 * PI) - PI;
    state.body_yaw += d * (1.0 - (-10.0 * dt).exp());
    let fwd = forward(state.body_yaw);
    let right = Vec3::new(state.body_yaw.cos(), 0.0, -state.body_yaw.sin());
    let up = Vec3::Y;

    // How far the hips sit down, by move.
    let drop = match c.state {
        MoveState::Slide { .. } => 0.58,
        _ if c.crouched => 0.40,
        _ if c.is_coiled() => 0.30,
        _ => 0.0,
    };
    state.hip_drop += (drop - state.hip_drop) * (1.0 - (-12.0 * dt).exp());
    let base = c.feet;
    let hip_c = base + up * (HIP_HEIGHT - state.hip_drop) + fwd * -0.06;

    // Torso: from the hips up to the belly, set back behind the eye so looking down you see
    // over it to your legs and feet; leaning back on a slide.
    let lean = if matches!(c.state, MoveState::Slide { .. }) { -0.30 } else { 0.08 * (speed / 7.0).min(1.0) };
    let spine = hip_c - fwd * 0.13;
    let chest = spine + (up + fwd * lean).normalize() * 0.30;
    if let Ok(mut t) = parts.get_mut(body.torso) {
        *t = limb_tf(spine, chest);
    }
    if let Ok(mut t) = parts.get_mut(body.hips) {
        *t = Transform::from_translation(hip_c).with_rotation(Quat::from_rotation_y(state.body_yaw) * Quat::from_rotation_z(FRAC_PI_2));
    }

    let kick = c.melee.as_ref().filter(|m| !matches!(m.kind, MeleeKind::Punch | MeleeKind::Crouch)).map(|m| if m.left { 0 } else { 1 });
    for (i, s) in [-1.0f32, 1.0].into_iter().enumerate() {
        let hip = hip_c + right * s * HIP_WIDTH;
        let under = Vec3::new(hip.x, base.y, hip.z);
        // Where the ankle wants to be.
        let ph = phase + if s > 0.0 { PI } else { 0.0 };
        let mut foot = match c.state {
            _ if kick == Some(i) => hip + fwd * 0.75 - up * 0.25,
            MoveState::Ground | MoveState::WallRun { .. } | MoveState::Balance { .. } | MoveState::Stumble { .. } | MoveState::Barge { .. } => {
                if speed < 0.4 {
                    under + right * s * 0.03
                } else {
                    // Stride along the way you're moving; the foot lifts on its swing forward.
                    let dir = vel / speed;
                    let stride = (0.16 + 0.05 * speed).min(0.52);
                    let lift = (0.05 + 0.025 * speed).min(0.2);
                    under + dir * stride * ph.sin() + up * lift * ph.cos().max(0.0)
                }
            }
            MoveState::Slide { .. } => {
                if s > 0.0 {
                    hip + fwd * 0.82 - up * 0.30
                } else {
                    hip + fwd * 0.45 - up * 0.22 + right * 0.08
                }
            }
            MoveState::Air | MoveState::SwingJump { .. } | MoveState::Vault(_) | MoveState::Traverse(_) | MoveState::StepUp { .. } => {
                let tuck = if c.is_coiled() { 0.45 } else { 0.2 };
                let fall = (-c.vel.y / 8.0).clamp(0.0, 1.0);
                hip - up * (0.86 - tuck * (1.0 - fall)) + fwd * (0.10 * s) + right * s * 0.05
            }
            // Hanging: legs dangling, a little sway.
            _ => hip - up * 0.88 + fwd * (0.04 + 0.03 * (phase * 0.5 + s).sin()),
        };
        if !matches!(c.state, MoveState::Ground) && foot.y < base.y {
            foot.y = base.y;
        }
        let ankle = foot + up * ANKLE;
        state.feet[i] = ankle;
        let (knee, ankle) = two_bone(hip, ankle, THIGH, SHIN, fwd + right * s * 0.15);
        let leg = &body.legs[i];
        if let Ok(mut t) = parts.get_mut(leg.thigh) {
            *t = limb_tf(hip, knee);
        }
        if let Ok(mut t) = parts.get_mut(leg.shin) {
            *t = limb_tf(knee, ankle);
        }
        if let Ok(mut t) = parts.get_mut(leg.knee) {
            *t = Transform::from_translation(knee);
        }
        // The shoe along the body, toe dipping as the foot swings.
        let pitch = if matches!(c.state, MoveState::Ground) && speed > 0.4 { -0.35 * ph.cos().max(0.0) } else if c.state == MoveState::Air { -0.4 } else { 0.0 };
        if let Ok(mut t) = parts.get_mut(leg.foot) {
            *t = Transform::from_translation(ankle - up * ANKLE).with_rotation(Quat::from_rotation_y(state.body_yaw) * Quat::from_rotation_x(pitch));
        }
    }
}
