//! Faith Runner: Mirror's Edge-style movement on greybox rooftops.
//!
//! All movement lives in the `faith_move` crate; this file is just the Bevy
//! shell around it: rendering, input, camera, HUD.

mod audio;

// The Mirror's Edge prologue map (FAITH_MAP=prologue): built with `--features prologue`.
#[cfg(feature = "prologue")]
mod me_level;
#[cfg(feature = "prologue")]
mod me_post;
mod me_viewmodel;
mod settings;
mod viewmodel;

use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use faith_move::greybox::{self, Level, Look};
use faith_move::{cleveland, moves, rooftops, springboard, Fixture};
use faith_move::{Aabb, BoxWorld, CameraFx, Controller, Shot, Event as MoveEvent, Input as MoveInput, State as MoveState, Tuning};

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Faith Runner".into(),
                resolution: (1600, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.62, 0.78, 0.93)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.85, 0.92, 1.0),
            brightness: 650.0,
            affects_lightmapped_meshes: true,
        })
        .add_systems(
            Startup,
            (setup_world, setup_hud, viewmodel::setup, me_viewmodel::setup, settings::setup, audio::setup, capture::setup).chain(),
        );
    let game = (switch_level, play, swing_doors, viewmodel::animate, me_viewmodel::animate, audio::play, update_hud).chain();
    #[cfg(feature = "prologue")]
    {
        app.add_plugins((me_level::MeLevelPlugin, me_post::MePostPlugin))
            .add_systems(Startup, me_level::setup.after(capture::setup))
            .add_systems(
                Update,
                (
                    capture::run.run_if(resource_exists::<capture::Capture>),
                    settings::update,
                    cursor_lock,
                    // Flying around a Mirror's Edge map (FAITH_MAP=prologue) replaces the game.
                    (me_level::fly, me_level::capture).chain().run_if(resource_exists::<me_level::Flying>),
                    (me_level::capture_play, game).chain().run_if(not(resource_exists::<me_level::Flying>)),
                )
                    .chain(),
            );
    }
    #[cfg(not(feature = "prologue"))]
    app.add_systems(Update, (capture::run.run_if(resource_exists::<capture::Capture>), settings::update, cursor_lock, game).chain());
    app.run();
}

// ------------------------------------------------------------------ state

#[derive(Resource)]
struct Game {
    ctrl: Controller,
    world: BoxWorld,
    level: Level,
    /// Which of [`LEVELS`] is loaded.
    level_index: usize,
    checkpoint: usize,
    locked: bool,
    /// The mouse was grabbed this frame: that click isn't an attack.
    just_locked: bool,
    show_help: bool,
    mouse_sens: f32,
    pad_sens: f32,
    timer: Option<f32>,
    /// Best time per level.
    best: [Option<f32>; LEVELS.len()],
    last_time: Option<f32>,
    flash: Option<(String, f32)>,
    /// Input supplied by a script instead of the keyboard (screenshot capture mode).
    scripted: Option<MoveInput>,
    /// Bob, shake, tilt and FOV layered on the controller's plain view.
    fx: CameraFx,
    /// This frame's final camera (also drives the viewmodel's timing).
    shot: Shot,
    /// Per-move look constraints (TdMove Min/MaxLookConstraint).
    look: faith_move::LookLimiter,
    /// Mirror's Edge's Reaction Time (Left Alt, left stick click): slows the app's clock.
    reaction: faith_move::reaction::ReactionTime,
}

/// The maps, in the order M cycles through them.
// Training stays last: the screenshot capture scripts its moves on it.
const LEVELS: [fn() -> Level; 5] = [rooftops::rooftops, moves::moves, springboard::springboard, cleveland::cleveland, greybox::greybox];
/// The map the game starts on (Cleveland) when FAITH_MAP doesn't pick one.
const DEFAULT_LEVEL: usize = 3;

/// Marks the boxes of the current map (despawned when switching maps).
#[derive(Component)]
struct LevelGeom;

/// Materials shared by every map.
#[derive(Resource)]
struct LevelMats {
    roof: Handle<StandardMaterial>,
    wall: Handle<StandardMaterial>,
    runner: Handle<StandardMaterial>,
    prop: Handle<StandardMaterial>,
    finish: Handle<StandardMaterial>,
    skyline: Handle<StandardMaterial>,
    /// The Cuyahoga and Lake Erie.
    water: Handle<StandardMaterial>,
    /// Parks and ball fields.
    green: Handle<StandardMaterial>,
    /// String lights, the chandelier, lit signs.
    lights: Handle<StandardMaterial>,
    /// Zipline cables and swing bars.
    metal: Handle<StandardMaterial>,
}

fn spawn_level(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &LevelMats, level: &Level) {
    for (b, look) in level.solids.iter().chain(&level.decor) {
        let material = match look {
            Look::Roof => mats.roof.clone(),
            Look::Wall => mats.wall.clone(),
            Look::Runner => mats.runner.clone(),
            Look::Prop => mats.prop.clone(),
            Look::Finish => mats.finish.clone(),
            Look::Skyline => mats.skyline.clone(),
            Look::Water => mats.water.clone(),
            Look::Green => mats.green.clone(),
            Look::Lights => mats.lights.clone(),
        };
        commands.spawn((LevelGeom, Mesh3d(meshes.add(box_mesh(b))), MeshMaterial3d(material)));
    }
    // Cables and bars: thin boxes stretched between their ends; doors, wire and pads as boxes.
    for (i, f) in level.fixtures.iter().enumerate() {
        if let Fixture::Ladder(l) = *f {
            for (a, b, thick) in l.rods() {
                let len = a.distance(b);
                let rot = Quat::from_rotation_arc(Vec3::Z, (b - a) / len);
                commands.spawn((
                    LevelGeom,
                    Mesh3d(meshes.add(Cuboid::new(thick, thick, len))),
                    MeshMaterial3d(mats.metal.clone()),
                    Transform::from_translation((a + b) * 0.5).with_rotation(rot),
                ));
            }
            continue;
        }
        let (a, b, thick) = match *f {
            Fixture::ZipLine { a, b } => (a, b, 0.025),
            Fixture::SwingPole { a, b } => (a, b, 0.06),
            Fixture::Door { b, n } => {
                // Hinged on its -X edge; swings away from the side you come from.
                let hinge = Vec3::new(b.min.x, b.min.y, (b.min.z + b.max.z) * 0.5);
                let size = b.max - b.min;
                commands
                    .spawn((LevelGeom, DoorVis { fixture: i, hinge, away: -n }, Transform::from_translation(hinge), Visibility::default()))
                    .with_child((
                        Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
                        MeshMaterial3d(mats.prop.clone()),
                        Transform::from_translation(Vec3::new(size.x * 0.5, size.y * 0.5, 0.0)),
                    ));
                continue;
            }
            Fixture::BarbedWire { b } => {
                commands.spawn((LevelGeom, Mesh3d(meshes.add(box_mesh(&b))), MeshMaterial3d(mats.metal.clone())));
                continue;
            }
            Fixture::Beam { .. } | Fixture::SoftPad { .. } | Fixture::Ladder(_) => continue,
        };
        let len = a.distance(b);
        let rot = Quat::from_rotation_arc(Vec3::Z, (b - a) / len);
        commands.spawn((
            LevelGeom,
            Mesh3d(meshes.add(Cuboid::new(thick, thick, len))),
            MeshMaterial3d(mats.metal.clone()),
            Transform::from_translation((a + b) * 0.5).with_rotation(rot),
        ));
    }
}

/// A door's swing: rotates about its hinge as the controller opens it.
#[derive(Component)]
struct DoorVis {
    fixture: usize,
    hinge: Vec3,
    away: Vec3,
}

fn swing_doors(game: Res<Game>, mut doors: Query<(&DoorVis, &mut Transform)>) {
    for (d, mut tf) in &mut doors {
        let open = game.ctrl.doors_open.get(d.fixture).copied().unwrap_or(0.0);
        // Ease out over the swing; 100 degrees open, away from the barge.
        let k = 1.0 - (1.0 - open).powi(3);
        let sign = if d.away.z < 0.0 { 1.0 } else { -1.0 };
        tf.translation = d.hinge;
        tf.rotation = Quat::from_rotation_y(sign * k * 100f32.to_radians());
    }
}

#[derive(Component)]
struct PlayerCamera;
#[derive(Component)]
struct HudStats;
#[derive(Component)]
struct HudFlash;
#[derive(Component)]
struct HudHelp;
#[derive(Component)]
struct HudClickToPlay;
#[derive(Component)]
struct SprintBar;

// ------------------------------------------------------------------ setup

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Screenshot capture scripts its moves on the training course.
    // Screenshot capture scripts its moves on the training course; the
    // "tour" capture photographs the Rooftops checkpoints instead.
    let level_index = if std::env::var_os("FAITH_CAPTURE_MOVES").is_some() {
        1
    } else if std::env::var_os("FAITH_CAPTURE_SLIDE").is_some() || std::env::var_os("FAITH_CAPTURE_LOOK").is_some() {
        2 // the Springboard range: long clear run-ups
    } else if std::env::var_os("FAITH_CAPTURE").is_some() && std::env::var_os("FAITH_CAPTURE_TOUR").is_none() {
        LEVELS.len() - 1
    } else if let Some(i) = std::env::var("FAITH_MAP").ok().and_then(|v| v.parse::<usize>().ok()) {
        i.min(LEVELS.len() - 1)
    } else if let Some(i) = std::env::var("FAITH_MAP").ok().and_then(|v| LEVELS.iter().position(|f| f().name.eq_ignore_ascii_case(v.trim()))) {
        i
    } else if std::env::var_os("FAITH_CAPTURE").is_some() {
        0 // the tour photographs Rooftops unless FAITH_MAP says otherwise
    } else {
        DEFAULT_LEVEL
    };
    let level = LEVELS[level_index]();
    let world = level.world();
    let cp = &level.checkpoints[0];
    let mut ctrl = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
    ctrl.state = MoveState::Ground;

    let grid = images.add(grid_texture());
    let mat = |base: Color, emissive: LinearRgba, rough: f32| StandardMaterial {
        base_color: base,
        base_color_texture: Some(grid.clone()),
        emissive,
        perceptual_roughness: rough,
        ..default()
    };
    let roof = materials.add(mat(Color::srgb(0.80, 0.81, 0.83), LinearRgba::BLACK, 0.9));
    let wall = materials.add(mat(Color::srgb(0.95, 0.95, 0.96), LinearRgba::BLACK, 0.8));
    let runner = materials.add(mat(Color::srgb(0.86, 0.10, 0.07), LinearRgba::rgb(0.25, 0.01, 0.0), 0.5));
    let prop = materials.add(mat(Color::srgb(0.62, 0.70, 0.78), LinearRgba::BLACK, 0.7));
    let finish = materials.add(mat(Color::srgb(1.0, 0.55, 0.1), LinearRgba::rgb(2.0, 0.8, 0.1), 0.4));
    let skyline = materials.add(StandardMaterial {
        base_color: Color::srgb(0.97, 0.98, 1.0),
        perceptual_roughness: 0.9,
        ..default()
    });

    let metal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.19, 0.21),
        metallic: 0.8,
        perceptual_roughness: 0.4,
        ..default()
    });
    let water = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.38, 0.52),
        perceptual_roughness: 0.12,
        reflectance: 0.6,
        ..default()
    });
    let green = materials.add(mat(Color::srgb(0.36, 0.62, 0.30), LinearRgba::BLACK, 0.95));
    let lights = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.86, 0.55),
        emissive: LinearRgba::rgb(4.0, 2.6, 1.0),
        ..default()
    });
    let mats = LevelMats { roof, wall, runner, prop, finish, skyline, water, green, lights, metal };
    spawn_level(&mut commands, &mut meshes, &mats, &level);
    commands.insert_resource(mats);

    // Sun.
    commands.spawn((
        DirectionalLight { illuminance: 11_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::new(0.0, 0.0, -60.0), Vec3::Y),
        // Lights the world and the first-person arms.
        RenderLayers::from_layers(&[0, viewmodel::VIEWMODEL_LAYER]),
    ));

    commands.spawn((
        PlayerCamera,
        IsDefaultUiCamera,
        Camera3d::default(),
        // Near plane 1 cm: the legs render in the world and the camera sits right above them
        // (in a slide, inside the hips), so 5 cm sliced them open at the bottom of the screen.
        Projection::Perspective(PerspectiveProjection { fov: 70f32.to_radians(), near: 0.01, ..default() }),
        Transform::from_translation(cp.spawn + Vec3::Y * 1.7),
        DistanceFog {
            color: Color::srgb(0.70, 0.82, 0.94),
            directional_light_color: Color::srgba(1.0, 0.95, 0.85, 0.4),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::Linear { start: 60.0, end: 260.0 },
        },
    ));

    commands.insert_resource(Game {
        ctrl,
        world,
        level,
        level_index,
        checkpoint: 0,
        locked: false,
        just_locked: false,
        show_help: true,
        mouse_sens: 0.0022,
        pad_sens: 3.2,
        timer: None,
        best: [None; LEVELS.len()],
        last_time: None,
        flash: None,
        scripted: None,
        fx: CameraFx::default(),
        shot: Shot::default(),
        look: Default::default(),
        reaction: Default::default(),
    });
}

/// Box mesh with world-space UVs (1 texture tile per metre), so the grid
/// stays square on every surface and you can read your speed off it.
fn box_mesh(b: &Aabb) -> Mesh {
    let (min, max) = (b.min, b.max);
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    // (normal, four corners counter-clockwise seen from outside, uv axes)
    let faces: [(Vec3, [Vec3; 4]); 6] = [
        (Vec3::X, [
            Vec3::new(max.x, min.y, max.z), Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z), Vec3::new(max.x, max.y, max.z),
        ]),
        (Vec3::NEG_X, [
            Vec3::new(min.x, min.y, min.z), Vec3::new(min.x, min.y, max.z),
            Vec3::new(min.x, max.y, max.z), Vec3::new(min.x, max.y, min.z),
        ]),
        (Vec3::Y, [
            Vec3::new(min.x, max.y, max.z), Vec3::new(max.x, max.y, max.z),
            Vec3::new(max.x, max.y, min.z), Vec3::new(min.x, max.y, min.z),
        ]),
        (Vec3::NEG_Y, [
            Vec3::new(min.x, min.y, min.z), Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, min.y, max.z), Vec3::new(min.x, min.y, max.z),
        ]),
        (Vec3::Z, [
            Vec3::new(min.x, min.y, max.z), Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, max.y, max.z), Vec3::new(min.x, max.y, max.z),
        ]),
        (Vec3::NEG_Z, [
            Vec3::new(max.x, min.y, min.z), Vec3::new(min.x, min.y, min.z),
            Vec3::new(min.x, max.y, min.z), Vec3::new(max.x, max.y, min.z),
        ]),
    ];
    for (n, corners) in faces {
        let base = pos.len() as u32;
        for c in corners {
            pos.push(c.to_array());
            nrm.push(n.to_array());
            let t = if n.x.abs() > 0.5 {
                Vec2::new(c.z, c.y)
            } else if n.y.abs() > 0.5 {
                Vec2::new(c.x, c.z)
            } else {
                Vec2::new(c.x, c.y)
            };
            uv.push([t.x, -t.y]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

/// White tile with a soft darker border: a classic greybox grid.
fn grid_texture() -> Image {
    const N: u32 = 128;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let edge = x.min(y).min(N - 1 - x).min(N - 1 - y);
            let half = (x == N / 2 || y == N / 2) as u8;
            let v: u8 = match edge {
                0 => 150,
                1 => 185,
                _ if half == 1 => 222,
                _ => 245,
            };
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

fn setup_hud(mut commands: Commands) {
    let font = |px: f32| TextFont { font_size: px.into(), ..default() };

    commands.spawn((
        HudStats,
        Text::new(""),
        font(17.0),
        TextColor(Color::srgb(0.08, 0.08, 0.1)),
        Node { position_type: PositionType::Absolute, top: px(14.0), left: px(16.0), ..default() },
    ));

    // Sprint charge bar.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(122.0),
                left: px(16.0),
                width: px(180.0),
                height: px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.18)),
        ))
        .with_children(|p| {
            p.spawn((
                SprintBar,
                Node { width: percent(0.0), height: percent(100.0), ..default() },
                BackgroundColor(Color::srgb(0.86, 0.10, 0.07)),
            ));
        });

    // Reticle.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: percent(50.0),
            top: percent(50.0),
            width: px(4.0),
            height: px(4.0),
            margin: UiRect { left: px(-2.0), top: px(-2.0), ..default() },
            ..default()
        },
        BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.55)),
    ));

    commands.spawn((
        HudFlash,
        Text::new(""),
        font(30.0),
        TextColor(Color::srgb(0.86, 0.10, 0.07)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: percent(62.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
    ));

    commands.spawn((
        HudHelp,
        Text::new(HELP),
        font(14.0),
        TextColor(Color::srgb(0.08, 0.08, 0.1)),
        TextLayout::justify(Justify::Right),
        Node { position_type: PositionType::Absolute, top: px(14.0), right: px(16.0), ..default() },
    ));

    commands.spawn((
        HudClickToPlay,
        Text::new("CLICK TO PLAY"),
        font(40.0),
        TextColor(Color::srgb(0.86, 0.10, 0.07)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: percent(40.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
    ));
}

pub(crate) const HELP: &str = "\
WASD move  |  Mouse look  |  Space jump, vault, wall moves
Shift / C crouch, slide, roll (tap before landing)
A or D + Space dodge (look 90 right, dodge left = top speed)
Q 180 turn (on a wall: climb, Q, Space to kick)  |  Left mouse (or F) attack, barge doors  |  G idle
Balance beam: left/right against the lean  |  Swing: hold W, Space on the forward swing
Left Alt Reaction Time  |  R respawn  |  1-9, 0 checkpoints  |  M next map  |  F1 help  |  F2 ME/procedural  |  Esc mouse
Pad: sticks  |  A/LB jump  |  B/LT crouch  |  Y turn  |  X kick  |  Select respawn";

// ------------------------------------------------------------------ systems

fn cursor_lock(
    mut game: ResMut<Game>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    capture: Option<Res<capture::Capture>>,
    mut menu: ResMut<settings::Menu>,
) {
    if capture.is_some() {
        return;
    }
    let Ok(mut cursor) = cursor.single_mut() else { return };
    let focused = windows.single().map(|w| w.focused).unwrap_or(true);
    let start = std::mem::take(&mut menu.start);
    let want = if menu.open || keys.just_pressed(KeyCode::Escape) || !focused {
        false
    } else if start || mouse.just_pressed(MouseButton::Left) {
        true
    } else {
        game.locked
    };
    if want != game.locked {
        game.just_locked = want;
        game.locked = want;
        cursor.visible = !want;
        cursor.grab_mode = if want { CursorGrabMode::Locked } else { CursorGrabMode::None };
    }
}

/// M cycles through the maps.
fn switch_level(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Res<LevelMats>,
    geom: Query<Entity, With<LevelGeom>>,
) {
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    for e in &geom {
        commands.entity(e).despawn();
    }
    let game = &mut *game;
    game.level_index = (game.level_index + 1) % LEVELS.len();
    game.level = LEVELS[game.level_index]();
    game.world = game.level.world();
    spawn_level(&mut commands, &mut meshes, &mats, &game.level);
    game.checkpoint = 0;
    game.timer = None;
    game.last_time = None;
    let cp = &game.level.checkpoints[0];
    game.ctrl.spawn = cp.spawn;
    game.ctrl.spawn_yaw = cp.yaw;
    game.ctrl.respawn();
    game.flash = Some((format!("MAP - {}", game.level.name), 0.0));
}

fn play(
    mut game: ResMut<Game>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<(&mut Transform, &mut Projection), With<PlayerCamera>>,
    settings: Res<settings::Settings>,
    menu: Res<settings::Menu>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    let dt = time.delta_secs();
    let game = &mut *game;
    game.fx.settings.fov_base = settings.fov;

    // ---- gather input
    let mut input = MoveInput::default();
    let mut mv = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) { mv.y += 1.0; }
    if keys.pressed(KeyCode::KeyS) { mv.y -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { mv.x += 1.0; }
    if keys.pressed(KeyCode::KeyA) { mv.x -= 1.0; }
    let crouch_keys = [KeyCode::ShiftLeft, KeyCode::KeyC, KeyCode::ControlLeft];
    input.jump_pressed = keys.just_pressed(KeyCode::Space);
    input.jump_held = keys.pressed(KeyCode::Space);
    input.crouch_pressed = crouch_keys.iter().any(|k| keys.just_pressed(*k));
    input.crouch_held = crouch_keys.iter().any(|k| keys.pressed(*k));
    input.turn_pressed = keys.just_pressed(KeyCode::KeyQ);
    // Attack (kick, punch, barging a door) is the left mouse button, as in the game.
    input.melee_pressed = keys.just_pressed(KeyCode::KeyF) || (game.locked && !game.just_locked && mouse.just_pressed(MouseButton::Left));
    game.just_locked = false;
    if game.locked {
        let d = mouse_motion.delta;
        input.look = Vec2::new(-d.x, -d.y) * game.mouse_sens * settings.sensitivity;
    }
    let mut respawn = keys.just_pressed(KeyCode::KeyR);
    let mut reaction = keys.just_pressed(KeyCode::AltLeft);

    for pad in &pads {
        let l = pad.left_stick();
        if l.length() > 0.15 {
            mv += l;
        }
        let r = pad.right_stick();
        let r = if r.length() > 0.12 { r * r.length() } else { Vec2::ZERO };
        input.look += Vec2::new(-r.x, r.y) * game.pad_sens * settings.sensitivity * dt;
        let jump = [GamepadButton::South, GamepadButton::LeftTrigger];
        let crouch = [GamepadButton::East, GamepadButton::LeftTrigger2];
        input.jump_pressed |= jump.iter().any(|b| pad.just_pressed(*b));
        input.jump_held |= jump.iter().any(|b| pad.pressed(*b));
        input.crouch_pressed |= crouch.iter().any(|b| pad.just_pressed(*b));
        input.crouch_held |= crouch.iter().any(|b| pad.pressed(*b));
        input.turn_pressed |= pad.just_pressed(GamepadButton::North);
        input.melee_pressed |= pad.just_pressed(GamepadButton::West);
        reaction |= pad.just_pressed(GamepadButton::LeftThumb);
        respawn |= pad.just_pressed(GamepadButton::Select);
    }
    input.strafe_raw = mv.x.clamp(-1.0, 1.0);
    input.move_axis = mv.clamp_length_max(1.0);
    if let Some(scripted) = game.scripted {
        input = scripted;
    }

    if keys.just_pressed(KeyCode::F1) {
        game.show_help = !game.show_help;
    }

    // ---- checkpoints / teleports
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    for (i, k) in digits.iter().enumerate() {
        if keys.just_pressed(*k) && i < game.level.checkpoints.len() {
            game.checkpoint = i;
            respawn = true;
        }
    }
    if respawn {
        let cp = &game.level.checkpoints[game.checkpoint];
        game.ctrl.spawn = cp.spawn;
        game.ctrl.spawn_yaw = cp.yaw;
        game.ctrl.respawn();
        if game.checkpoint == 0 {
            game.timer = None;
        }
    }

    // ---- simulate (freeze while the mouse is released so you can alt-tab)
    if (game.locked || !pads.is_empty() || game.scripted.is_some()) && !menu.open {
        // TdPlayerController.UpdateReactionTime: the game's speed is the app's clock (dt is
        // already slowed by it).
        let (was_full, was_on) = (game.reaction.energy >= 100.0, game.reaction.active);
        if reaction {
            game.reaction.attempt();
        }
        let speed = game.reaction.update(dt, game.ctrl.vel.length());
        if !was_full && game.reaction.energy >= 100.0 {
            game.flash = Some(("REACTION TIME READY - Left Alt".to_string(), 0.0));
        }
        if !was_on && game.reaction.active {
            game.flash = Some(("REACTION TIME".to_string(), 0.0));
        }
        virtual_time.set_relative_speed(speed);
        let world = &game.world;
        game.ctrl.step(dt, &input, world);
        let g = &mut *game;
        g.look.apply(&mut g.ctrl, dt);
        game.shot = game.fx.update(dt, &game.ctrl, &input);
    }

    for e in game.ctrl.events.clone() {
        if let Some(label) = flash_label(e) {
            game.flash = Some((label.to_string(), 0.0));
        }
    }
    if let Some((_, t)) = &mut game.flash {
        *t += dt;
    }

    if let Some(i) = game.level.checkpoint_at(game.ctrl.feet) {
        if i > game.checkpoint {
            game.checkpoint = i;
            let cp = &game.level.checkpoints[i];
            game.ctrl.spawn = cp.spawn;
            game.ctrl.spawn_yaw = cp.yaw;
            game.flash = Some((format!("CHECKPOINT - {}", cp.name), 0.0));
        }
    }

    // ---- time trial: starts when you leave the start area, ends at the finish
    let f = game.ctrl.feet;
    if game.timer.is_none() && game.checkpoint == 0 && game.level.checkpoint_at(f) != Some(0) && game.ctrl.state == MoveState::Ground {
        game.timer = Some(0.0);
    }
    let at_finish = game.level.in_finish(f);
    if let Some(t) = &mut game.timer {
        *t += dt;
        if at_finish {
            let t = *t;
            game.timer = None;
            game.last_time = Some(t);
            let li = game.level_index;
            let pb = game.best[li].is_none_or(|b| t < b);
            if pb {
                game.best[li] = Some(t);
            }
            game.flash = Some((format!("FINISH {}{}", fmt_time(t), if pb { " - NEW BEST" } else { "" }), -2.0));
            game.checkpoint = 0;
        }
    }

    // ---- camera
    let view = game.shot.view;
    if let Ok((mut tf, mut proj)) = camera.single_mut() {
        tf.translation = view.eye;
        tf.rotation = Quat::from_euler(EulerRot::YXZ, view.yaw, view.pitch.clamp(-FRAC_PI_2, FRAC_PI_2), -view.roll);
        if let Projection::Perspective(p) = &mut *proj {
            let aspect = windows.single().map(|w| w.width() / w.height().max(1.0)).unwrap_or(16.0 / 9.0);
            // Controller gives horizontal FOV; Bevy wants vertical.
            let h = view.fov_deg.to_radians();
            p.fov = 2.0 * ((h * 0.5).tan() / aspect).atan();
        }
    }
}

fn flash_label(e: MoveEvent) -> Option<&'static str> {
    Some(match e {
        MoveEvent::Vault => "VAULT",
        MoveEvent::WallRunStart => "WALLRUN",
        MoveEvent::WallJump => "WALL JUMP",
        MoveEvent::WallClimbStart => "WALLCLIMB",
        MoveEvent::WallKick => "WALL KICK",
        MoveEvent::LedgeGrab => "LEDGE",
        MoveEvent::Slide => "SLIDE",
        MoveEvent::Roll => "ROLL",
        MoveEvent::Mantle => "MANTLE",
        MoveEvent::Dodge { .. } => "DODGE",
        MoveEvent::WallRunDodge { .. } => "WALLRUN DODGE",
        MoveEvent::WallClimbDodge { .. } => "WALLCLIMB DODGE",
        MoveEvent::RumpSlide => "RUMP SLIDE",
        MoveEvent::GrabTransfer => "GRAB TRANSFER",
        MoveEvent::AirBarge => "AIR BARGE",
        MoveEvent::Vertigo => "VERTIGO",
        MoveEvent::SwingToSwing => "SWING JUMP",
        MoveEvent::ClimbStart { .. } => "LADDER",
        MoveEvent::HardLand => "HARD LANDING - tap crouch just before you land",
        MoveEvent::Death => "RESPAWN",
        _ => return None,
    })
}

fn fmt_time(t: f32) -> String {
    format!("{:02}:{:05.2}", (t / 60.0) as u32, t % 60.0)
}

fn update_hud(
    game: Res<Game>,
    me: Option<Res<me_viewmodel::MeArms>>,
    mut stats: Query<&mut Text, (With<HudStats>, Without<HudFlash>, Without<HudHelp>)>,
    mut flash: Query<(&mut Text, &mut TextColor), (With<HudFlash>, Without<HudStats>, Without<HudHelp>)>,
    mut help: Query<&mut Visibility, (With<HudHelp>, Without<HudClickToPlay>)>,
    mut click: Query<&mut Visibility, (With<HudClickToPlay>, Without<HudHelp>)>,
    mut bar: Query<&mut Node, With<SprintBar>>,
    menu: Res<settings::Menu>,
) {
    let c = &game.ctrl;
    let speed = c.horizontal_speed();
    if let Ok(mut t) = stats.single_mut() {
        let timer = match (game.timer, game.last_time) {
            (Some(t), _) => format!("Time {}", fmt_time(t)),
            (None, Some(l)) => format!("Last {}", fmt_time(l)),
            _ => "Time --:--.--".into(),
        };
        let best = game.best[game.level_index].map(|b| format!("   Best {}", fmt_time(b))).unwrap_or_default();
        let anim = me.as_ref().filter(|m| m.active).map(|m| format!("   [{}]", m.current_anim())).unwrap_or_default();
        t.0 = format!(
            "{}{}\n{:.1} m/s  ({:.0} km/h)\n{}{}\n{} - {}\nSprint",
            c.state.name(),
            anim,
            speed,
            speed * 3.6,
            timer,
            best,
            game.level.name,
            game.level.checkpoints[game.checkpoint].name,
        );
    }
    if let Ok(mut n) = bar.single_mut() {
        n.width = percent(c.sprint_charge * 100.0);
    }
    if let Ok((mut t, mut color)) = flash.single_mut() {
        match &game.flash {
            Some((s, age)) => {
                let a = (1.0 - (age.max(0.0) - 0.6) / 0.6).clamp(0.0, 1.0);
                t.0 = s.clone();
                color.0 = Color::srgba(0.86, 0.10, 0.07, a);
            }
            None => t.0.clear(),
        }
    }
    if let Ok(mut v) = help.single_mut() {
        *v = if game.show_help { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Ok(mut v) = click.single_mut() {
        *v = if game.locked || menu.open { Visibility::Hidden } else { Visibility::Inherited };
    }
}

/// Screenshot capture mode for checking the game without a person at the
/// keyboard: `FAITH_CAPTURE=<dir> cargo run` plays a few scripted moves from
/// the checkpoints, saves a PNG mid-move for each into <dir>, and quits.
mod capture {
    use super::*;
    use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
    use std::f32::consts::FRAC_PI_2;

    #[derive(Resource)]
    pub struct Capture {
        dir: String,
        stage: usize,
        frame: u32,
        /// Frame the shot was taken on, if it has been.
        shot_at: Option<u32>,
        flags: Flags,
    }

    #[derive(Default)]
    struct Flags {
        a: bool,
        b: bool,
        t: f32,
    }

    pub fn setup(mut commands: Commands) {
        if let Ok(dir) = std::env::var("FAITH_CAPTURE") {
            let _ = std::fs::create_dir_all(&dir);
            // FAITH_CAPTURE_FROM=<n> starts at stage n (0-based), for re-shooting one move.
            // FAITH_CAPTURE_MOVES shoots the special moves on the Moves map instead.
            let first = if std::env::var_os("FAITH_CAPTURE_MOVES").is_some() { FIRST_MOVES_STAGE } else { 0 };
            let stage = first + std::env::var("FAITH_CAPTURE_FROM").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            commands.insert_resource(Capture { dir, stage, frame: 0, shot_at: None, flags: Flags::default() });
        }
    }

    /// Stages from here on run on the Moves map (FAITH_CAPTURE_MOVES).
    const FIRST_MOVES_STAGE: usize = 10;

    /// (file name, starting checkpoint)
    const STAGES: [(&str, usize); 16] = [
        ("01_start", 0),
        ("02_sprint", 0),
        ("03_vault", 0),
        ("04_slide", 0),
        ("05_dodge", 0),
        ("06_wallrun", 1),
        ("07_wallclimb", 2),
        ("08_ledge_hang", 2),
        ("09_look_down_running", 0),
        ("10_pull_up", 2),
        ("11_warmup", 0),
        ("12_springboard", 0),
        ("13_balance", 1),
        ("14_swing", 2),
        ("15_zipline", 3),
        ("16_kick", 4),
    ];

    fn steer(c: &Controller, x: f32) -> MoveInput {
        let strafe = ((x - c.feet.x) * 1.2).clamp(-0.6, 0.6);
        MoveInput { move_axis: Vec2::new(strafe, 1.0), ..default() }
    }

    /// Input for this frame, and whether to take the picture now.
    fn script(stage: usize, c: &Controller, f: &mut Flags, dt: f32) -> (MoveInput, bool) {
        use greybox::z;
        match stage {
            0 => (MoveInput::default(), f.t > 150.0),
            1 => (steer(c, 0.0), c.feet.z < -5.0),
            2 => {
                let mut i = steer(c, 0.0);
                i.jump_pressed = !f.a && c.feet.z < z::RAIL + 0.85;
                f.a |= i.jump_pressed;
                let shoot = matches!(c.state, MoveState::Traverse(tr) if tr.t > 0.3);
                (i, shoot)
            }
            3 => {
                let mut i = steer(c, 0.0);
                if c.feet.z < z::RAIL + 0.85 && !f.a {
                    i.jump_pressed = true;
                    f.a = true;
                }
                i.crouch_pressed = !f.b && c.feet.z < z::PIPE + 2.5;
                f.b |= i.crouch_pressed;
                i.crouch_held = f.b;
                // Look 40° left mid-slide: the body stays on the slide.
                if matches!(c.state, MoveState::Slide { .. }) && f.t == 0.0 {
                    i.look.x = 0.7;
                    f.t = 1.0;
                }
                (i, matches!(c.state, MoveState::Slide { t } if t > 0.35))
            }
            4 => {
                // Run, look 90 right, dodge left.
                if f.b {
                    f.t += dt;
                    return (MoveInput::default(), f.t > 0.12);
                }
                if f.a {
                    // The runner has turned to look right (see `run`): now dodge left.
                    f.b = true;
                    return (MoveInput { move_axis: Vec2::new(-1.0, 0.0), jump_pressed: true, ..default() }, false);
                }
                f.a = c.feet.z < -3.0;
                (steer(c, 0.0), false)
            }
            5 => {
                let mut i = steer(c, greybox::WALLRUN_X - 0.5);
                i.jump_pressed = !f.a && c.feet.z < z::ROOF_B1_END + 1.3;
                f.a |= i.jump_pressed;
                (i, matches!(c.state, MoveState::WallRun { t, .. } if t > 0.25))
            }
            6 | 7 => {
                if let MoveState::LedgeHang { .. } = c.state {
                    f.t += dt;
                    return (MoveInput::default(), stage == 7 && f.t > 1.0);
                }
                if matches!(c.state, MoveState::WallClimb { .. }) {
                    // Let go of forward so a hang doesn't auto-climb.
                    return (MoveInput::default(), stage == 6 && matches!(c.state, MoveState::WallClimb { t, .. } if t > 0.1));
                }
                let mut i = steer(c, 0.0);
                i.jump_pressed = !f.a && c.feet.z < z::CLIMB_WALL + 1.2;
                f.a |= i.jump_pressed;
                (i, false)
            }
            8 => (steer(c, 0.0), c.feet.z < -4.0),
            9 => {
                if let MoveState::LedgeHang { .. } = c.state {
                    f.b = true;
                    return (MoveInput { move_axis: Vec2::new(0.0, 1.0), ..default() }, false);
                }
                if let MoveState::Traverse(tr) = c.state {
                    return (MoveInput::default(), tr.t > 0.45);
                }
                if matches!(c.state, MoveState::WallClimb { .. }) || f.b {
                    return (MoveInput::default(), false);
                }
                let mut i = steer(c, 0.0);
                i.jump_pressed = !f.a && c.feet.z < z::CLIMB_WALL + 1.2;
                f.a |= i.jump_pressed;
                (i, false)
            }
            10 => (MoveInput::default(), f.t > 150.0),
            11 => {
                use moves::z;
                let mut i = steer(c, 0.0);
                i.jump_pressed = !f.a && c.feet.z < z::STEP_START + 2.5;
                f.a |= i.jump_pressed;
                f.b |= matches!(c.state, MoveState::Traverse(tr) if tr.kind == faith_move::TraverseKind::SpringBoard);
                (i, f.b && c.state == MoveState::Air && c.vel.y < 5.0)
            }
            12 => {
                use moves::z;
                if let MoveState::Balance { lean, .. } = c.state {
                    let counter = (-lean * 4.0).clamp(-1.0, 1.0);
                    return (MoveInput { move_axis: Vec2::new(counter, 1.0), ..default() }, c.feet.z < z::M2_END - 3.0);
                }
                let mut i = steer(c, 0.0);
                i.move_axis.y = 0.6;
                (i, false)
            }
            13 => {
                use moves::z;
                if let MoveState::Swing { angle, .. } = c.state {
                    return (MoveInput { move_axis: Vec2::new(0.0, 1.0), ..default() }, angle > 0.5);
                }
                let mut i = steer(c, 0.0);
                i.jump_pressed = !f.a && c.feet.z < z::M3_END + 0.5 && c.state == MoveState::Ground;
                f.a |= i.jump_pressed;
                (i, false)
            }
            14 => {
                use moves::z;
                if let MoveState::ZipLine { s, .. } = c.state {
                    return (MoveInput::default(), s > 6.0);
                }
                let mut i = steer(c, 0.0);
                i.jump_pressed = !f.a && c.feet.z < z::ZIP_START + 1.8 && c.state == MoveState::Ground;
                f.a |= i.jump_pressed;
                (i, false)
            }
            15 => {
                f.t += dt;
                let i = MoveInput { melee_pressed: !f.a && f.t > 0.3, ..default() };
                f.a |= i.melee_pressed;
                (i, c.melee.is_some_and(|m| m.t > 0.25))
            }
            _ => (MoveInput::default(), false),
        }
    }

    pub fn run(
        mut commands: Commands,
        mut cap: ResMut<Capture>,
        mut game: ResMut<Game>,
        time: Res<Time>,
        mut exit: MessageWriter<AppExit>,
    ) {
        if std::env::var_os("FAITH_CAPTURE_TOUR").is_some() {
            // One still per checkpoint of the current map.
            let n = game.level.checkpoints.len();
            if cap.stage >= n {
                println!("capture: done");
                exit.write(AppExit::Success);
                return;
            }
            cap.frame += 1;
            if cap.frame == 1 {
                let cp = game.level.checkpoints[cap.stage].clone();
                // FAITH_CAPTURE_YAW / _PITCH (degrees; right and up are positive) turn the view
                // off the run's direction, to photograph the scenery beside it.
                let deg = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok()).map(f32::to_radians);
                game.ctrl.spawn = cp.spawn;
                game.ctrl.spawn_yaw = cp.yaw - deg("FAITH_CAPTURE_YAW").unwrap_or(0.0);
                game.ctrl.respawn();
                game.ctrl.pitch = deg("FAITH_CAPTURE_PITCH").unwrap_or(-0.12);
                game.checkpoint = cap.stage;
                game.locked = true;
                game.scripted = Some(MoveInput::default());
            }
            if cap.frame == 150 {
                let path = format!("{}/tour_{:02}.png", cap.dir, cap.stage + 1);
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
                println!("capture: tour {}", game.level.checkpoints[cap.stage].name);
            }
            if cap.frame > 170 {
                cap.stage += 1;
                cap.frame = 0;
            }
            return;
        }
        if std::env::var_os("FAITH_CAPTURE_LOOK").is_some() {
            look_run(&mut commands, &mut cap, &mut game, &mut exit);
            return;
        }
        if std::env::var_os("FAITH_CAPTURE_WALLS").is_some() {
            walls_run(&mut commands, &mut cap, &mut game, &mut exit, time.delta_secs());
            return;
        }
        if std::env::var_os("FAITH_CAPTURE_SLIDE").is_some() {
            slide_run(&mut commands, &mut cap, &mut game, &mut exit);
            return;
        }
        let moves_run = std::env::var_os("FAITH_CAPTURE_MOVES").is_some();
        let past_end = !moves_run && cap.stage >= FIRST_MOVES_STAGE;
        let Some(&(name, cp_index)) = STAGES.get(cap.stage).filter(|_| !past_end) else {
            println!("capture: done");
            exit.write(AppExit::Success);
            return;
        };
        cap.frame += 1;
        let dt = time.delta_secs();
        if cap.frame == 1 {
            let cp = game.level.checkpoints[cp_index].clone();
            game.ctrl.spawn = cp.spawn;
            game.ctrl.spawn_yaw = cp.yaw;
            game.ctrl.respawn();
            game.ctrl.state = MoveState::Ground;
            game.checkpoint = cp_index;
            game.locked = true;
            if cap.stage == 8 {
                game.ctrl.pitch = -1.0; // look down at your legs
            }
            // FAITH_CAPTURE_PITCH=<radians> overrides the starting look pitch.
            if let Some(p) = std::env::var("FAITH_CAPTURE_PITCH").ok().and_then(|v| v.parse::<f32>().ok()) {
                game.ctrl.pitch = p;
            }
            cap.flags = Flags::default();
            cap.shot_at = None;
        }
        if cap.stage == 4 && cap.flags.a && !cap.flags.b {
            game.ctrl.yaw = -FRAC_PI_2;
        }

        let stage = cap.stage;
        let (input, shoot) = {
            let cap = &mut *cap;
            if stage == 0 || stage == FIRST_MOVES_STAGE {
                cap.flags.t += 1.0; // counts frames: give the renderer time to compile shaders
            }
            script(stage, &game.ctrl, &mut cap.flags, dt)
        };
        game.scripted = Some(input);

        if shoot && cap.shot_at.is_none() {
            cap.shot_at = Some(cap.frame);
            let path = format!("{}/{name}.png", cap.dir);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            println!("capture: {name} state={} speed={:.2}", game.ctrl.state.name(), game.ctrl.horizontal_speed());
        }
        let done = cap.shot_at.is_some_and(|f| cap.frame > f + 25);
        if done || cap.frame > 1500 {
            if cap.shot_at.is_none() {
                println!("capture: {name} TIMED OUT (state {})", game.ctrl.state.name());
            }
            cap.stage += 1;
            cap.frame = 0;
        }
    }

    /// FAITH_CAPTURE_SLIDE: slide down a clear lane, press one thing mid-slide, and shoot it
    /// (for chasing the body clipping into the view).
    const SLIDE_SHOTS: [&str; 10] =
        ["plain", "look_left", "look_right", "turn_q", "melee_f", "strafe_a", "back_s", "jump_out", "let_go", "look_down"];

    fn slide_run(commands: &mut Commands, cap: &mut Capture, game: &mut Game, exit: &mut MessageWriter<AppExit>) {
        let Some(&name) = SLIDE_SHOTS.get(cap.stage) else {
            println!("capture: done");
            exit.write(AppExit::Success);
            return;
        };
        cap.frame += 1;
        if cap.frame == 1 {
            let cp = game.level.checkpoints[0].clone();
            game.ctrl.spawn = cp.spawn;
            game.ctrl.spawn_yaw = cp.yaw;
            game.ctrl.respawn();
            game.ctrl.state = MoveState::Ground;
            game.ctrl.pitch = -0.1;
            game.locked = true;
            cap.flags = Flags::default();
            cap.shot_at = None;
        }
        let c = &game.ctrl;
        // Wait for shaders on the first shot.
        let warm = cap.stage > 0 || cap.frame > 120;
        let mut i = MoveInput { move_axis: Vec2::new(0.0, 1.0), ..default() };
        let slide_t = if let MoveState::Slide { t } = c.state { Some(t) } else { None };
        if warm && !cap.flags.a && c.horizontal_speed() > 6.0 && c.state == MoveState::Ground {
            i.crouch_pressed = true;
            cap.flags.a = true;
        }
        i.crouch_held = cap.flags.a && !(name == "let_go" && cap.flags.b);
        // The key, 0.2 s into the slide.
        if slide_t.is_some_and(|t| t > 0.2) && !cap.flags.b {
            cap.flags.b = true;
            match name {
                "look_left" => i.look.x = 0.9,
                "look_right" => i.look.x = -0.9,
                "turn_q" => i.turn_pressed = true,
                "melee_f" => i.melee_pressed = true,
                "jump_out" => i.jump_pressed = true,
                "look_down" => i.look.y = -1.0,
                _ => {}
            }
        }
        if cap.flags.b {
            match name {
                "strafe_a" => i.move_axis = Vec2::new(-1.0, 1.0),
                "back_s" => i.move_axis = Vec2::new(0.0, -1.0),
                _ => {}
            }
            cap.flags.t += 1.0;
        }
        game.scripted = Some(i);
        // FAITH_CAPTURE_DELAY=<frames> after the key (default 9).
        let delay = std::env::var("FAITH_CAPTURE_DELAY").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(9.0);
        if cap.flags.t == delay && cap.shot_at.is_none() {
            cap.shot_at = Some(cap.frame);
            let path = format!("{}/slide_{:02}_{name}_{delay}.png", cap.dir, cap.stage + 1);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            println!("capture: slide {name} state={} yaw={:.2}", game.ctrl.state.name(), game.ctrl.yaw);
        }
        if cap.shot_at.is_some_and(|f| cap.frame > f + 25) || cap.frame > 900 {
            cap.stage += 1;
            cap.frame = 0;
        }
    }

    /// FAITH_CAPTURE_LOOK: look down by steps, standing and then running, and shoot each
    /// (for the body clipping into the view as the camera moves).
    const LOOK_SHOTS: [(&str, f32, bool); 10] = [
        ("stand_00", 0.0, false),
        ("stand_25", -0.44, false),
        ("stand_45", -0.79, false),
        ("stand_65", -1.13, false),
        ("stand_83", -1.45, false),
        ("run_00", 0.0, true),
        ("run_25", -0.44, true),
        ("run_45", -0.79, true),
        ("run_65", -1.13, true),
        ("run_83", -1.45, true),
    ];

    fn look_run(commands: &mut Commands, cap: &mut Capture, game: &mut Game, exit: &mut MessageWriter<AppExit>) {
        let Some(&(name, pitch, running)) = LOOK_SHOTS.get(cap.stage) else {
            println!("capture: done");
            exit.write(AppExit::Success);
            return;
        };
        cap.frame += 1;
        if cap.frame == 1 {
            let cp = game.level.checkpoints[0].clone();
            game.ctrl.spawn = cp.spawn;
            game.ctrl.spawn_yaw = cp.yaw;
            game.ctrl.respawn();
            game.ctrl.state = MoveState::Ground;
            game.locked = true;
            cap.shot_at = None;
        }
        game.ctrl.pitch = pitch;
        let mv = if running { Vec2::new(0.0, 1.0) } else { Vec2::ZERO };
        game.scripted = Some(MoveInput { move_axis: mv, ..default() });
        // Shaders on the first shot; then time for the swan neck and the run to settle.
        let at = if cap.stage == 0 { 150 } else { 60 };
        if cap.frame == at && cap.shot_at.is_none() {
            cap.shot_at = Some(cap.frame);
            let path = format!("{}/look_{:02}_{name}.png", cap.dir, cap.stage + 1);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            println!("capture: look {name}");
        }
        if cap.shot_at.is_some_and(|f| cap.frame > f + 20) {
            cap.stage += 1;
            cap.frame = 0;
        }
    }

    /// FAITH_CAPTURE_WALLS: hang on the Training climb wall and look 100° left and right; then
    /// wallrun Roof B1's wall, press Q, and jump off. Shots of each.
    fn walls_run(commands: &mut Commands, cap: &mut Capture, game: &mut Game, exit: &mut MessageWriter<AppExit>, dt: f32) {
        use greybox::z;
        cap.frame += 1;
        let dir = cap.dir.clone();
        let shoot = |commands: &mut Commands, name: &str| {
            let path = format!("{dir}/walls_{name}.png");
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            println!("capture: {name}");
        };
        let c = (game.ctrl.state, game.ctrl.feet);
        let stage = cap.stage;
        let cap_frame = cap.frame;
        let f = &mut cap.flags;
        let mut next = false;
        match stage {
            // ---- hang, then look away
            0 | 1 => {
                if cap_frame == 1 {
                    let cp = game.level.checkpoints[2].clone();
                    game.ctrl.spawn = cp.spawn;
                    game.ctrl.spawn_yaw = cp.yaw;
                    game.ctrl.respawn();
                    game.ctrl.state = MoveState::Ground;
                    game.locked = true;
                    *f = Flags::default();
                }
                let mut i = MoveInput::default();
                if let MoveState::LedgeHang { .. } = c.0 {
                    f.t += dt;
                    let sign = if stage == 0 { 1.0 } else { -1.0 };
                    if (1.0..1.5).contains(&f.t) {
                        i.look.x = sign * 100f32.to_radians() * dt / 0.5;
                    }
                    if f.t > 3.0 && !f.b {
                        f.b = true;
                        shoot(commands, if stage == 0 { "hang_look_left" } else { "hang_look_right" });
                    }
                    if f.t > 3.3 {
                        next = true;
                    }
                } else if matches!(c.0, MoveState::WallClimb { .. }) {
                } else {
                    let strafe = ((0.0 - c.1.x) * 1.2).clamp(-0.6, 0.6);
                    i = MoveInput { move_axis: Vec2::new(strafe, 1.0), ..default() };
                    i.jump_pressed = !f.a && c.1.z < z::CLIMB_WALL + 1.2;
                    f.a |= i.jump_pressed;
                }
                game.scripted = Some(i);
            }
            // ---- wallrun, Q, jump
            2 => {
                if cap_frame == 1 {
                    let cp = game.level.checkpoints[1].clone();
                    game.ctrl.spawn = cp.spawn;
                    game.ctrl.spawn_yaw = cp.yaw;
                    game.ctrl.respawn();
                    game.ctrl.state = MoveState::Ground;
                    *f = Flags::default();
                }
                let strafe = ((greybox::WALLRUN_X - 0.5 - c.1.x) * 1.2).clamp(-0.6, 0.6);
                let mut i = MoveInput { move_axis: Vec2::new(strafe, 1.0), ..default() };
                i.jump_pressed = !f.a && c.1.z < z::ROOF_B1_END + 1.3;
                f.a |= i.jump_pressed;
                if c.0 == MoveState::Air && f.a && !f.b && f.t == 0.0 {
                    i.look.x = -0.008; // angle into the wall on the right
                }
                if let MoveState::WallRun { t, .. } = c.0 {
                    if (t - 0.2).abs() < dt * 0.5 {
                        shoot(commands, "wallrun_right");
                    }
                    if t > 0.25 && !f.b {
                        f.b = true;
                        f.t = 0.0;
                        i.turn_pressed = true;
                    }
                }
                if f.b {
                    f.t += dt;
                    i.move_axis = Vec2::ZERO;
                    if (f.t - 0.2).abs() < dt * 0.5 {
                        shoot(commands, "wallrun_after_q");
                    }
                    if (f.t - 0.3).abs() < dt * 0.5 {
                        i.jump_pressed = true;
                    }
                    for (at, name) in [(0.45, "wallrun_jump_0.15s"), (0.7, "wallrun_jump_0.4s")] {
                        if (f.t - at).abs() < dt * 0.5 {
                            shoot(commands, name);
                        }
                    }
                    if f.t > 1.0 {
                        next = true;
                    }
                }
                game.scripted = Some(i);
            }
            // ---- the same wall the other way: wall on the left
            3 => {
                if cap_frame == 1 {
                    game.ctrl.spawn = Vec3::new(4.0, greybox::ROOF_B_Y + 0.05, -66.0);
                    game.ctrl.spawn_yaw = std::f32::consts::PI;
                    game.ctrl.respawn();
                    game.ctrl.state = MoveState::Ground;
                    *f = Flags::default();
                }
                // Line up 0.6 m off the wall's line (strafe left is +x here, facing +z).
                let strafe = -((greybox::WALLRUN_X - 0.6 - c.1.x) * 1.2).clamp(-0.6, 0.6);
                let mut i = MoveInput { move_axis: Vec2::new(strafe, 1.0), ..default() };
                i.jump_pressed = cap_frame > 5 && !f.a && c.0 == MoveState::Ground && c.1.z > z::ROOF_B2_START - 1.3;
                f.a |= i.jump_pressed;
                if c.0 == MoveState::Air && f.a && f.t < 0.1 {
                    f.t += dt;
                    i.look.x = 0.03; // angle into the wall on the left
                    i.move_axis = Vec2::new(-1.0, 1.0);
                }
                if let MoveState::WallRun { t, .. } = c.0 {
                    f.b = true;
                    for (at, name) in [(0.1, "wl_0.10"), (0.15, "wl_0.15"), (0.2, "wl_0.20"), (0.25, "wl_0.25"), (0.3, "wl_0.30"), (0.4, "wl_0.40")] {
                        if (t - at).abs() < dt * 0.5 {
                            shoot(commands, name);
                        }
                    }
                }
                if f.b && !matches!(c.0, MoveState::WallRun { .. }) || cap_frame > 2000 {
                    next = true;
                }
                game.scripted = Some(i);
            }
            // ---- walk into the climb wall: hands up against it
            4 => {
                if cap_frame == 1 {
                    let cp = game.level.checkpoints[2].clone();
                    game.ctrl.spawn = cp.spawn;
                    game.ctrl.spawn_yaw = cp.yaw;
                    game.ctrl.respawn();
                    game.ctrl.state = MoveState::Ground;
                    game.ctrl.pitch = -0.1;
                    *f = Flags::default();
                }
                let near = c.1.z < z::CLIMB_WALL + 1.5;
                let i = MoveInput { move_axis: Vec2::new(0.0, if near { 0.25 } else { 0.6 }), ..default() };
                if game.ctrl.against_wall.left.is_some() || game.ctrl.against_wall.right.is_some() {
                    f.t += dt;
                    if f.t > 0.8 && !f.b {
                        f.b = true;
                        shoot(commands, "against_wall");
                    }
                }
                if f.t > 1.0 || cap_frame > 900 {
                    next = true;
                }
                game.scripted = Some(i);
            }
            _ => {
                println!("capture: done");
                exit.write(AppExit::Success);
            }
        }
        if next {
            cap.stage += 1;
            cap.frame = 0;
        }
    }
}
