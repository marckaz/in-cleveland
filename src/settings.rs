//! The launch menu: volume (master / music / effects), mouse sensitivity and
//! field of view. It opens when the game starts and again on Esc, and saves
//! to `settings.txt` next to where you run the game.
//!
//! Keyboard: ↑/↓ pick a row, ←/→ change it, Enter plays. Or click the − / +
//! buttons and Play.

use bevy::prelude::*;

const FILE: &str = "settings.txt";

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0..=1 each.
    pub master: f32,
    pub music: f32,
    pub effects: f32,
    /// Multiplier on the base mouse/pad sensitivity.
    pub sensitivity: f32,
    /// Horizontal field of view in degrees (Mirror's Edge uses 90).
    pub fov: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { master: 0.8, music: 0.6, effects: 1.0, sensitivity: 1.0, fov: 90.0 }
    }
}

impl Settings {
    pub fn music_gain(&self) -> f32 {
        self.master * self.music
    }

    pub fn effects_gain(&self) -> f32 {
        self.master * self.effects
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Ok(v) = v.trim().parse::<f32>() else { continue };
            match k.trim() {
                "master" => s.master = v,
                "music" => s.music = v,
                "effects" => s.effects = v,
                "sensitivity" => s.sensitivity = v,
                "fov" => s.fov = v,
                _ => {}
            }
        }
        for (i, r) in ROWS.iter().enumerate() {
            let v = r.get(&s).clamp(r.min, r.max);
            ROWS[i].set(&mut s, v);
        }
        s
    }

    pub fn to_text(&self) -> String {
        format!(
            "# In Cleveland settings\nmaster = {:.2}\nmusic = {:.2}\neffects = {:.2}\nsensitivity = {:.2}\nfov = {:.0}\n",
            self.master, self.music, self.effects, self.sensitivity, self.fov
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load() -> Settings {
        std::fs::read_to_string(FILE).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self) {
        if let Err(e) = std::fs::write(FILE, self.to_text()) {
            warn!("couldn't save {FILE}: {e}");
        }
    }

    /// In the browser, settings live in the page's local storage.
    #[cfg(target_arch = "wasm32")]
    pub fn load() -> Settings {
        web_storage().and_then(|s| s.get_item(FILE).ok().flatten()).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn save(&self) {
        if let Some(s) = web_storage() {
            let _ = s.set_item(FILE, &self.to_text());
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn web_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

struct Row {
    label: &'static str,
    min: f32,
    max: f32,
    step: f32,
    get: fn(&Settings) -> f32,
    put: fn(&mut Settings, f32),
    show: fn(f32) -> String,
}

impl Row {
    fn get(&self, s: &Settings) -> f32 {
        (self.get)(s)
    }
    fn set(&self, s: &mut Settings, v: f32) {
        (self.put)(s, v)
    }
    fn nudge(&self, s: &mut Settings, dir: f32) {
        let v = self.get(s) + self.step * dir;
        // Snap to the step so repeated presses land on round numbers.
        let v = ((v / self.step).round() * self.step).clamp(self.min, self.max);
        self.set(s, v);
    }
}

fn pct(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

const ROWS: [Row; 5] = [
    Row { label: "Master volume", min: 0.0, max: 1.0, step: 0.05, get: |s| s.master, put: |s, v| s.master = v, show: pct },
    Row { label: "Music", min: 0.0, max: 1.0, step: 0.05, get: |s| s.music, put: |s, v| s.music = v, show: pct },
    Row { label: "Effects", min: 0.0, max: 1.0, step: 0.05, get: |s| s.effects, put: |s, v| s.effects = v, show: pct },
    Row {
        label: "Mouse sensitivity",
        min: 0.1,
        max: 3.0,
        step: 0.05,
        get: |s| s.sensitivity,
        put: |s, v| s.sensitivity = v,
        show: |v| format!("{v:.2}x"),
    },
    Row { label: "Field of view", min: 70.0, max: 110.0, step: 1.0, get: |s| s.fov, put: |s, v| s.fov = v, show: |v| format!("{v:.0} deg") },
];

#[derive(Resource)]
pub struct Menu {
    pub open: bool,
    selected: usize,
    /// Set when Play is chosen; the cursor-lock system grabs the mouse and clears it.
    pub start: bool,
}

#[derive(Component)]
pub struct MenuRoot;
#[derive(Component)]
pub struct ValueText(usize);
#[derive(Component)]
pub struct RowNode(usize);
#[derive(Component, Clone, Copy)]
pub enum MenuButton {
    Nudge(usize, f32),
    Play,
}

const PANEL: Color = Color::srgba(0.04, 0.04, 0.05, 0.86);
const RED: Color = Color::srgb(0.86, 0.10, 0.07);
const ROW_IDLE: Color = Color::srgba(1.0, 1.0, 1.0, 0.04);
const ROW_SELECTED: Color = Color::srgba(0.86, 0.10, 0.07, 0.35);
const BTN: Color = Color::srgba(1.0, 1.0, 1.0, 0.10);
const BTN_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.22);

pub fn setup(mut commands: Commands) {
    let settings = Settings::load();
    // Screenshot capture runs straight through without the menu.
    let open = std::env::var_os("FAITH_CAPTURE").is_none() && std::env::var_os("FAITH_CAPTURE_TOUR").is_none();
    let font = |px_: f32| TextFont { font_size: px_.into(), ..default() };
    let white = TextColor(Color::srgb(0.95, 0.95, 0.95));
    let dim = TextColor(Color::srgba(0.95, 0.95, 0.95, 0.55));

    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            GlobalZIndex(10),
            if open { Visibility::Visible } else { Visibility::Hidden },
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(28.0)),
                    row_gap: px(8.0),
                    width: px(520.0),
                    ..default()
                },
                BackgroundColor(PANEL),
            ))
            .with_children(|panel| {
                panel.spawn((Text::new("IN CLEVELAND"), font(34.0), TextColor(RED)));
                panel.spawn((Text::new("Settings"), font(18.0), dim.clone(), Node { margin: UiRect::bottom(px(12.0)), ..default() }));
                for (i, r) in ROWS.iter().enumerate() {
                    panel
                        .spawn((
                            RowNode(i),
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                padding: UiRect::axes(px(12.0), px(6.0)),
                                column_gap: px(10.0),
                                ..default()
                            },
                            BackgroundColor(ROW_IDLE),
                        ))
                        .with_children(|row| {
                            row.spawn((Text::new(r.label), font(19.0), white.clone(), Node { flex_grow: 1.0, ..default() }));
                            row.spawn(button(MenuButton::Nudge(i, -1.0))).with_children(|b| {
                                b.spawn((Text::new("-"), font(22.0), white.clone()));
                            });
                            row.spawn((
                                ValueText(i),
                                Text::new((r.show)(r.get(&settings))),
                                font(19.0),
                                white.clone(),
                                Node { width: px(70.0), justify_content: JustifyContent::Center, ..default() },
                            ));
                            row.spawn(button(MenuButton::Nudge(i, 1.0))).with_children(|b| {
                                b.spawn((Text::new("+"), font(22.0), white.clone()));
                            });
                        });
                }
                panel
                    .spawn((
                        MenuButton::Play,
                        Button,
                        Node {
                            margin: UiRect::top(px(16.0)),
                            padding: UiRect::axes(px(16.0), px(10.0)),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(RED),
                    ))
                    .with_children(|b| {
                        b.spawn((Text::new("PLAY"), font(22.0), white.clone()));
                    });
                panel.spawn((
                    Text::new("Up/Down pick  -  Left/Right change  -  Enter play  -  Esc opens this again  -  M next map (Downtown, Ohio City & Tremont and Lakewood are the real city)"),
                    font(14.0),
                    dim.clone(),
                    Node { margin: UiRect::top(px(10.0)), ..default() },
                ));
            });
        });

    commands.insert_resource(Menu { open, selected: 0, start: false });
    commands.insert_resource(settings);
}

fn button(kind: MenuButton) -> impl Bundle {
    (
        kind,
        Button,
        Node { width: px(34.0), height: px(30.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BackgroundColor(BTN),
    )
}

#[allow(clippy::type_complexity)]
pub fn update(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<Menu>,
    mut settings: ResMut<Settings>,
    mut buttons: Query<(&Interaction, &MenuButton, &mut BackgroundColor), (Changed<Interaction>, Without<RowNode>)>,
    mut rows: Query<(&RowNode, &mut BackgroundColor), Without<MenuButton>>,
    mut values: Query<(&ValueText, &mut Text)>,
    mut root: Query<&mut Visibility, With<MenuRoot>>,
) {
    if !menu.open {
        if keys.just_pressed(KeyCode::Escape) {
            menu.open = true;
        }
    } else {
        let before = settings.clone();
        let n = ROWS.len();
        if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
            menu.selected = (menu.selected + 1) % n;
        }
        if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
            menu.selected = (menu.selected + n - 1) % n;
        }
        for (key, dir) in [(KeyCode::ArrowLeft, -1.0), (KeyCode::KeyA, -1.0), (KeyCode::ArrowRight, 1.0), (KeyCode::KeyD, 1.0)] {
            if keys.just_pressed(key) {
                ROWS[menu.selected].nudge(&mut settings, dir);
            }
        }
        let mut play = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
        for (interaction, kind, mut bg) in &mut buttons {
            let play_button = matches!(kind, MenuButton::Play);
            match interaction {
                Interaction::Pressed => match *kind {
                    MenuButton::Nudge(i, dir) => {
                        menu.selected = i;
                        ROWS[i].nudge(&mut settings, dir);
                    }
                    MenuButton::Play => play = true,
                },
                Interaction::Hovered => *bg = BackgroundColor(if play_button { RED.lighter(0.08) } else { BTN_HOVER }),
                Interaction::None => *bg = BackgroundColor(if play_button { RED } else { BTN }),
            }
        }
        if *settings != before {
            settings.save();
        }
        if play {
            menu.open = false;
            menu.start = true;
            settings.save();
        }
    }

    for (row, mut bg) in &mut rows {
        *bg = BackgroundColor(if row.0 == menu.selected { ROW_SELECTED } else { ROW_IDLE });
    }
    if settings.is_changed() {
        for (v, mut text) in &mut values {
            let r = &ROWS[v.0];
            **text = (r.show)(r.get(&settings));
        }
    }
    for mut vis in &mut root {
        let want = if menu.open { Visibility::Visible } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_clamps() {
        let s = Settings { master: 0.35, music: 0.0, effects: 0.9, sensitivity: 1.25, fov: 100.0 };
        assert_eq!(Settings::parse(&s.to_text()), s);
        let wild = Settings::parse("master = 7\nfov = 10\nnonsense\nmusic = x");
        assert_eq!(wild.master, 1.0);
        assert_eq!(wild.fov, 70.0);
        assert_eq!(wild.music, Settings::default().music);
    }

    #[test]
    fn nudges_snap_to_steps() {
        let mut s = Settings { master: 0.33, ..default() };
        ROWS[0].nudge(&mut s, 1.0);
        assert!((s.master - 0.40).abs() < 1e-5, "{}", s.master);
        for _ in 0..30 {
            ROWS[0].nudge(&mut s, -1.0);
        }
        assert_eq!(s.master, 0.0);
    }
}
