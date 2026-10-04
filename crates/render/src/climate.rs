//! Climates: which of the game's climates the world is in, from the
//! ecosystem map (`asset_format::eco`), like `ksys::world::Manager::
//! getClimate` does with the player's position (the camera's without a
//! player). The climate tints the light and the sky and picks the row of
//! palette sets (see `daynight.rs`).
//!
//! One climate at a time, as the game's world manager keeps it
//! ([`Climate`]): a change eases the climate's factors over from the former
//! climate's, and the environment eases from the former row of palette
//! sets to the new one ([`PaletteRows`]). While the world manager's timer
//! runs ([`StageTimer`]: a new scene, a jump of the camera or the player,
//! entering the Dark Woods) these, the weather and the sky take their
//! targets at once. Without a dump everything is the first climate,
//! the central plains.
//!
//! Weather: every few game hours each climate rolls a new weather from its
//! odds like `WeatherMgr::rollNewWeather` (clear, cloudy, rain, heavy rain,
//! else a thunderstorm), clear if the climate locks the sky by day or night;
//! rain is snow where it is freezing. The weather tints the light and the
//! sky (`WeatherInfluence_N`) and drives the sky manager's cloudiness,
//! which turns the cloud layers from their clear look to their cloudy one
//! (`PrCloudV0_N`, `PrCloudV1_N`).

use std::path::PathBuf;
use std::sync::Arc;

use asset_format::eco::{CLIMATES, Ecosystem};
use asset_format::env::{Climate as ClimateDefines, Influence, SKY_CLEAR, SKY_OVERCAST, WEATHERS};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::daynight::{Environment, TimeOfDay};

#[derive(Resource, Default)]
pub struct ClimateOverride(pub Option<usize>);

pub struct ClimatePlugin {
    /// The `assets/` folder to read the ecosystem map from.
    pub assets: PathBuf,
    /// A weather to keep everywhere (index into `WEATHERS`).
    pub weather: Option<usize>,
}

impl Plugin for ClimatePlugin {
    fn build(&self, app: &mut App) {
        let path = self.assets.join(asset_format::paths::ECOSYSTEM);
        let loading = path.exists().then(|| {
            AsyncComputeTaskPool::get().spawn(async move {
                asset_format::read_ron::<Ecosystem>(&path)
                    .inspect_err(|error| warn!("climate map unavailable: {error}"))
                    .ok()
                    .map(Arc::new)
            })
        });
        app.insert_resource(ClimateMap {
            loading,
            map: None,
            located: false,
        })
        .init_resource::<Climate>()
        .init_resource::<ClimateOverride>()
        .init_resource::<StageTimer>()
        .init_resource::<Moisture>()
        .insert_resource(Weather {
            forced: self.weather,
            ..default()
        })
        // The game's frame (`FUN_03414c80`): the managers' updates, then
        // the world's (the timer, the positions, the climate).
        .add_systems(
            Update,
            (
                finish_loading,
                start_stage,
                forecast,
                follow_palette_rows,
                dampen,
            )
                .chain()
                .before(crate::daynight::update_sky_state),
        )
        .add_systems(
            PostUpdate,
            world_frame
                .after(crate::clouds::update_clouds)
                .before(TransformSystems::Propagate),
        );
    }
}

/// The climate map, once read.
#[derive(Resource)]
pub struct ClimateMap {
    loading: Option<Task<Option<Arc<Ecosystem>>>>,
    map: Option<Arc<Ecosystem>>,
    /// The climate has been placed from the map for the scene.
    located: bool,
}

impl ClimateMap {
    /// The map is read and the climate placed (without a dump there is
    /// nothing to wait for).
    pub fn is_settled(&self) -> bool {
        self.loading.is_none() && (self.map.is_none() || self.located)
    }

    /// The climate (index into `eco::CLIMATES`) at (`x`, `z`) like the
    /// game's `Manager::getClimate`; none while there is no map.
    pub fn climate_at(&self, x: f32, z: f32) -> Option<usize> {
        self.map.as_ref().map(|eco| eco.climate_at(x, z))
    }
}

/// The world's climate like the game's world manager keeps it (Wii U v208,
/// docs/research/wiiu-sky-resources.md, "`WorldMgr+0x53c`" and "Climate
/// consumers"): one climate, the one it changed from and how far the change
/// has come. Only the environment's palette update reads the last two, for
/// the climates' factors (`FeatureColor`, `CalcRayleigh`, `CalcMie`,
/// `CalcMieSymmetrical`, `CalcSfParam*`, `CalcVolumeMaskIntencity`); all
/// else (the weather, the wind, the temperature, the row of palette sets)
/// follows the climate at once.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Climate {
    /// The climate (index into `eco::CLIMATES`, `WorldMgr+0x5f8`).
    pub current: usize,
    /// The climate it changed from (`+0x5fc`).
    pub previous: usize,
    /// How far the change has come, 0–1 (`+0x5cc`; see
    /// [`StageTimer::step`]).
    pub transition: f32,
    /// The environment's rows of palette sets.
    pub rows: PaletteRows,
    /// The area's name where the climate is taken.
    pub area: String,
}

impl Default for Climate {
    fn default() -> Self {
        Self::settled(0, 0)
    }
}

impl Climate {
    /// Climate `index` (into `eco::CLIMATES`) with its row of palette sets
    /// `row` (its `PaletteSetSelect`), with no change under way.
    pub fn settled(index: usize, row: usize) -> Self {
        Self {
            current: index,
            previous: index,
            transition: 1.0,
            rows: PaletteRows::settled(row),
            area: String::new(),
        }
    }

    /// The climates' shares in their factors: the former climate's
    /// `1 − transition` and the current one's `transition`, as the palette
    /// update lerps them (@`0x03642e04…0x036431b4`).
    pub fn shares(&self) -> [(usize, f32); 2] {
        [
            (self.previous, 1.0 - self.transition),
            (self.current, self.transition),
        ]
    }

    /// The current climate's name in words, for the HUD: `EldinClimateLv1`
    /// is "Eldin Lv1".
    pub fn name(&self) -> String {
        let name = CLIMATES[self.current]
            .replace("Climate", "")
            .replace("Hyrule", "Central");
        words(name.strip_suffix("Climat").unwrap_or(&name))
    }
}

/// The row of palette sets of the darkness (`EnvMgr::
/// getConcentrationDarkness`, `FUN_0364be3c`), the Dark Woods' only.
const DARKNESS_ROW: usize = 7;

/// Updates of the environment the climate's row is held after the climate
/// last set it (`EnvMgr+0x3ced0`: 4 from `FUN_03641114`, counted down by
/// `FUN_0364becc`).
const ROW_HOLD: f32 = 4.0;

/// The environment's rows of palette sets (`EnvMgr+0x190` the former,
/// `+0x194` the active, `+0x198` how far from one to the other; rows of
/// `env::PALETTE_SET_ROWS`, a climate's `PaletteSetSelect`), as the
/// palette update moves them (`ENV_UpdateWeatherPalettes` `0x036425b8`,
/// Wii U v208). A climate whose row is not the field's sets it each update
/// (`FUN_03641140` → `FUN_03641114`) and holds it four more after it no
/// longer does, then the row falls back to the field's, 0. Once a change
/// is done (1) a different row is taken, from 0, and eases to 1 by exactly
/// 0.005 a frame (some 7 s), 0.0025 from the darkness's row (13 s); while
/// [`StageTimer`] runs at once. Every field is blended per row, then the
/// two rows by the share. Not repeated: the rows set by the map
/// (`ChangeWeatherTag` `PaletteSel`, `EnvMgr+0x3cec4`), its steps
/// (`+0x3cef8`: 0.0025, 0.1, 0.025) and the other stages' row 5.
// SI-WTH-04: map PaletteSel rows and their steps are left out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaletteRows {
    pub previous: usize,
    pub active: usize,
    pub transition: f32,
    /// The climate's row as the environment holds it (`+0x3cecc`); none
    /// once the hold has run out.
    held: Option<usize>,
    /// Frames of the game's 30 a second the row is still held.
    hold: f32,
}

impl Default for PaletteRows {
    /// The environment's reset (`FUN_0363d104`): the field's row, done.
    fn default() -> Self {
        Self::settled(0)
    }
}

impl PaletteRows {
    /// Row `row`, reached.
    pub fn settled(row: usize) -> Self {
        Self {
            previous: row,
            active: row,
            transition: 1.0,
            held: (row != 0).then_some(row),
            hold: ROW_HOLD,
        }
    }

    /// One palette update under a climate whose row is `row`, `frames` of
    /// the game's 30 a second; `snap` while [`StageTimer`] runs.
    fn step(&mut self, row: usize, frames: f32, snap: bool) {
        if row != 0 {
            self.held = Some(row);
            self.hold = ROW_HOLD;
        }
        if self.transition >= 1.0 {
            let wanted = self.held.unwrap_or(0);
            if wanted != self.active {
                self.previous = self.active;
                self.active = wanted;
                self.transition = 0.0;
            }
        }
        // `0x10300220`, `0x10300210`; the multiple `+0x3ce5c` is 1.
        let step = if self.previous == DARKNESS_ROW {
            0.0025
        } else {
            0.005
        } * frames;
        self.transition = chase(self.transition, 1.0, 0.9, frames, step, step);
        if snap {
            self.transition = 1.0;
        }
    }

    /// From row `previous` to row `active`, `transition` of the way.
    #[cfg(test)]
    pub(crate) fn between(previous: usize, active: usize, transition: f32) -> Self {
        Self {
            previous,
            active,
            transition,
            ..Self::settled(active)
        }
    }

    /// The world's frame (`FUN_036781fc` → `FUN_0364becc`): the hold runs
    /// down, and once it has the climate's row is let go.
    fn count_down(&mut self, frames: f32) {
        if self.hold <= 0.0 {
            self.held = None;
        } else {
            self.hold = (self.hold - frames).max(0.0);
        }
    }

    /// How far the darkness's row has come in (`FUN_0364be3c`): the
    /// transition while it is the active row, else 0.
    pub fn darkness(&self) -> f32 {
        if self.active == DARKNESS_ROW {
            self.transition
        } else {
            0.0
        }
    }

    /// The two rows with their shares: the former's `1 − transition` and
    /// the active one's `transition`.
    pub fn shares(&self) -> [(usize, f32); 2] {
        [
            (self.previous, 1.0 - self.transition),
            (self.active, self.transition),
        ]
    }
}

/// `WorldMgr+0x53c` (`world::Manager::mTimer`, Wii U v208): frames in
/// which the world's managers take their targets at once, with what sets
/// it. The constructor (`FUN_03671e5c`) and every stage's init
/// (`FUN_03673f6c`, Switch `Manager::onStageInit`) set it to 30. Then each
/// frame (`FUN_03414c80`): the world's update (`FUN_03677ef0`) counts it
/// down by one; `FUN_036781fc` sets 30 when the camera or the player moved
/// [`JUMP`] or more in the frame before (Switch
/// `Manager::hasCameraOrPlayerMoved`, `FUN_0367811c`); `FUN_03678d78`
/// moves the positions on and follows the climate under the player (the
/// camera without one), and a change to `DarkWoodsClimat` sets 90 (see
/// [`StageTimer::step`]). The stage's init puts the camera's former
/// position 200 m above it, so the first frame counts as a jump.
///
/// Here frames run at 30 fps from a new scene. Not followed: 30 while the
/// `Fade` or `FadeDemo_00` screen is open (`FUN_036414d8`; it also holds
/// the countdown), the viewer has neither; and the exception for the
/// player within 100 m during an event, the viewer has no events.
#[derive(Resource)]
pub struct StageTimer {
    timer: f32,
    /// The camera's and the player's position (`WorldMgr+0x500`, `+0x518`)
    /// and theirs a frame before (`+0x50c`, `+0x524`); none before the
    /// stage's init.
    camera: Option<[Vec3; 2]>,
    player: Option<[Vec3; 2]>,
    /// The climate has been taken from the map (the stage's init takes it
    /// if the map is read by then, else the first frame that has it).
    placed: bool,
}

impl Default for StageTimer {
    fn default() -> Self {
        Self {
            timer: 30.0,
            camera: None,
            player: None,
            placed: false,
        }
    }
}

/// How far the camera or the player moves in a frame for the world's
/// managers to take their targets at once: 20 m (`FUN_036781fc`
/// `0x40340000…`, no frame scale).
const JUMP: f32 = 20.0;

/// `DarkWoodsClimat` (Switch `worldDefines.h` `Climate`, 15).
const DARK_WOODS: usize = 15;

/// What the world's frame sees: the camera, the player if there is one,
/// and the climate at a place (none without a climate map).
pub struct WorldFrame<'a> {
    pub camera: Vec3,
    pub player: Option<Vec3>,
    pub climate_at: &'a dyn Fn(Vec3) -> Option<usize>,
}

impl StageTimer {
    /// The managers take their targets at once.
    pub fn running(&self) -> bool {
        self.timer > 0.0
    }

    /// The stage's init (`FUN_03673f6c`): the camera's former place 200 m
    /// above it, the player where it is, the climate under the camera with
    /// no change under way.
    fn start(&mut self, world: &WorldFrame, climate: &mut Climate) {
        let camera = world.camera;
        self.camera = Some([camera, camera + Vec3::Y * 200.0]);
        self.player = world.player.map(|p| [p, p]);
        self.place((world.climate_at)(camera), climate);
    }

    fn place(&mut self, found: Option<usize>, climate: &mut Climate) {
        if let Some(found) = found {
            (climate.current, climate.previous, climate.transition) = (found, found, 1.0);
            self.placed = true;
        }
    }

    /// The world's frame after the managers' (`FUN_03677ef0`,
    /// `FUN_036781fc`, then `FUN_03678d78`), `frames` frames at 30 fps:
    /// the countdown, the jump, the positions and the climate. While the
    /// climate's transition is under way it chases 1 by `1 − 0.99^t` of
    /// the way, at least 0.005·t and at most 0.01·t (at once while the
    /// timer runs), and no new climate is looked for; else a different
    /// climate under the player (the camera) is taken from 0, except that
    /// entering the Dark Woods while the darkness has not fully come in
    /// takes it at once and sets the timer to 90.
    fn step(&mut self, world: &WorldFrame, frames: f32, climate: &mut Climate) {
        if self.camera.is_none() {
            self.start(world, climate);
        }
        // `FUN_03677ef0`.
        self.timer = (self.timer - frames).max(0.0);
        // `FUN_036781fc`.
        if self.moved(JUMP) {
            self.timer = 30.0;
        }
        climate.rows.count_down(frames);
        // `FUN_03678d78`: the positions move on (the player's only while
        // there is one).
        if let Some([now, before]) = &mut self.camera {
            *before = *now;
            *now = world.camera;
        }
        if let Some(player) = world.player {
            let [now, _] = self.player.get_or_insert([player; 2]);
            self.player = Some([player, *now]);
        }
        let at = world.player.unwrap_or(world.camera);
        if climate.transition < 1.0 {
            let (least, most) = (0.005 * frames, 0.01 * frames);
            climate.transition = chase(climate.transition, 1.0, 0.99, frames, least, most);
            if self.running() {
                climate.transition = 1.0;
            }
            return;
        }
        let found = (world.climate_at)(at);
        if !self.placed {
            // The map is read only now: as if the stage had begun with it.
            self.place(found, climate);
            return;
        }
        let Some(found) = found else { return };
        if found != climate.current {
            climate.previous = climate.current;
            climate.current = found;
            climate.transition = 0.0;
            if found == DARK_WOODS && climate.rows.darkness() < 1.0 {
                climate.transition = 1.0;
                self.timer = 90.0;
            }
        }
    }

    /// `hasCameraOrPlayerMoved` (`FUN_0367811c`) over the last frame's
    /// positions.
    fn moved(&self, distance: f32) -> bool {
        let far = |pair: Option<[Vec3; 2]>| {
            pair.is_some_and(|[now, before]| now.distance(before) >= distance)
        };
        far(self.camera) || far(self.player)
    }
}

fn finish_loading(mut map: ResMut<ClimateMap>) {
    let Some(task) = &mut map.loading else { return };
    let Some(result) = block_on(poll_once(task)) else {
        return;
    };
    map.loading = None;
    if let Some(eco) = &result {
        info!("climate map: {} areas", eco.areas.len());
    }
    map.map = result;
}

/// Where the world's frame looks: the camera and the player (there is no
/// player here: the camera's place stands for it, as in the original renderer's
/// free flight).
#[derive(bevy::ecs::system::SystemParam)]
pub struct WorldPlaces<'w, 's> {
    cameras: Query<'w, 's, &'static GlobalTransform, crate::camera::MainCamera>,
}

impl WorldPlaces<'_, '_> {
    fn camera(&self) -> Option<Vec3> {
        self.cameras.single().ok().map(GlobalTransform::translation)
    }

    fn player(&self) -> Option<Vec3> {
        None
    }
}

/// A new scene (the next camera of a batch) starts like the game's stage:
/// the timer at 30, the environment's rows reset (`FUN_0363d104`), the
/// climate under the camera (`FUN_03673f6c`), before the managers' first
/// update.
fn start_stage(
    forced: Res<ClimateOverride>,
    epoch: Option<Res<crate::ready::SceneEpoch>>,
    mut map: ResMut<ClimateMap>,
    places: WorldPlaces,
    mut stage: ResMut<StageTimer>,
    mut climate: ResMut<Climate>,
) {
    if epoch.is_some_and(|epoch| epoch.is_changed()) {
        *stage = StageTimer::default();
        climate.rows = PaletteRows::default();
        map.located = false;
    }
    if stage.camera.is_some() {
        return;
    }
    let Some(camera) = places.camera() else {
        return;
    };
    let climate_at = |at: Vec3| forced.0.or_else(|| map.climate_at(at.x, at.z));
    let world = WorldFrame {
        camera,
        player: places.player(),
        climate_at: &climate_at,
    };
    let mut next = climate.clone();
    stage.start(&world, &mut next);
    if next != *climate {
        *climate = next;
    }
    if stage.placed && !map.located {
        map.located = true;
    }
}

/// The environment's rows of palette sets follow the climate's (see
/// [`PaletteRows`]).
fn follow_palette_rows(
    real: Res<Time>,
    environment: Res<Environment>,
    stage: Res<StageTimer>,
    mut climate: ResMut<Climate>,
) {
    let row = environment
        .climates
        .get(climate.current)
        .map_or(0, |c| c.palette_set);
    let mut rows = climate.rows;
    rows.step(row, real.delta_secs() * 30.0, stage.running());
    if rows != climate.rows {
        climate.rows = rows;
    }
}

/// The world's frame after the managers' (see [`StageTimer::step`]), and
/// the area's name for the HUD.
fn world_frame(
    forced: Res<ClimateOverride>,
    real: Res<Time>,
    mut map: ResMut<ClimateMap>,
    places: WorldPlaces,
    mut stage: ResMut<StageTimer>,
    mut climate: ResMut<Climate>,
) {
    let Some(camera) = places.camera() else {
        return;
    };
    let player = places.player();
    let climate_at = |at: Vec3| forced.0.or_else(|| map.climate_at(at.x, at.z));
    let world = WorldFrame {
        camera,
        player,
        climate_at: &climate_at,
    };
    let mut next = climate.clone();
    stage.step(&world, real.delta_secs() * 30.0, &mut next);
    let at = player.unwrap_or(camera);
    next.area = map
        .map
        .as_ref()
        .and_then(|eco| eco.area_at(at.x, at.z))
        .map(|a| a.name.clone())
        .unwrap_or_default();
    if next != *climate {
        *climate = next;
    }
    if stage.placed && !map.located {
        map.located = true;
    }
}

/// Game hours a weather lasts before its climate rolls the next.
// SI-WTH-03: weather schedule blocks and roll are ours.
const WEATHER_HOURS: f32 = 4.0;

/// The weather transition from which the game takes a new weather
/// (`WeatherMgr+0x14` ≥ 0.99, `0x1030215c`; weather calc `0x03667d20`, Wii U
/// v208).
const WEATHER_TAKEN: f32 = 0.99;
/// The transition's step per 30 Hz frame towards 1 (`0x03667d20`): 0.002
/// while no weather is set for the world (`WorldMgr+0x649` = 0xff,
/// `0x103021d0`), 0.0025 otherwise (`0x103021cc`); the chase's rate
/// (`1 − 0.99^t`, `0x1030215c`) stays below the step, so it is the step
/// throughout: a new weather blows in over 500 frames, about 17 s.
const WEATHER_STEP: f32 = 0.002;

/// How fast the sky manager's cloudiness (`SkyMgr+0x2120`) follows its
/// target, per 30 Hz frame: at most 0.0015 and at least a fifth of it
/// (`FUN_03655de8`, Wii U v208; × the time step's multiple of the usual,
/// `TimeMgr+0xb0`, 1 in ordinary play), so from clear to cloudy in some
/// 22 s.
const CLOUDINESS_STEP: f32 = 0.0015;

/// The cloudiness above which a weather other than the clear ones turns
/// the sky overcast (`SKY_UpdateWeatherStateBlend` `0x03641c44`).
const CLOUDINESS_OVERCAST: f32 = 0.2;

/// Indices into `WEATHERS`.
pub(crate) const CLEAR: usize = 0;
pub(crate) const CLOUDY: usize = 1;
pub(crate) const RAIN: usize = 2;
pub(crate) const HEAVY_RAIN: usize = 3;
pub(crate) const SNOW: usize = 4;
pub(crate) const HEAVY_SNOW: usize = 5;
pub(crate) const THUNDERSTORM: usize = 6;
pub(crate) const THUNDER_RAIN: usize = 7;
pub(crate) const BLUESKY_RAIN: usize = 8;

/// The world's weather, like the game's `WeatherMgr`: one weather
/// for the climate and the one it replaces while it blows in.
#[derive(Resource, Clone, Debug)]
pub struct Weather {
    /// The weather (index into `WEATHERS`): the game's `WeatherMgr+0x18`,
    /// the new weather from the moment it is taken.
    pub current: usize,
    /// Share of each weather: the previous one's `1 − transition` and the
    /// current one's `transition`, as the game blends the weathers'
    /// `WeatherInfluence`.
    pub weights: [f32; WEATHERS.len()],
    /// The air temperature at the camera in °C, with a dump.
    pub temperature: Option<f32>,
    /// The weather blowing in, the same as `current` (the game's
    /// `WeatherMgr+0x18`; `TempMgr` reads it with the transition).
    pub arriving: usize,
    /// The weather it replaces (`WeatherMgr+0x19`).
    pub previous: usize,
    /// How far `arriving` has blown in, 0–1 (`WeatherMgr+0x14`).
    pub transition: f32,
    /// The sky state it heads for ([`SkyState`]).
    pub sky: SkyState,
    /// The sky manager's cloudiness (`SkyMgr+0x2120`), 0–1: it heads for 1
    /// under any weather but `Bluesky` and `BlueskyRain`, for 0 under those
    /// ([`CLOUDINESS_STEP`]); the sky overcasts only above
    /// [`CLOUDINESS_OVERCAST`].
    pub cloudiness: f32,
    /// The climate's weather this frame (`0x03672890`): what is wanted,
    /// before it is taken.
    pub wanted: usize,
    forced: Option<usize>,
    /// The weather has been placed for the scene (the first is taken as it
    /// is; afterwards new ones blow in).
    settled: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Self::settled(CLEAR)
    }
}

impl Weather {
    /// Weather `weather` (an index into `WEATHERS`) fully blown in.
    pub fn settled(weather: usize) -> Self {
        let mut weights = [0.0; WEATHERS.len()];
        weights[weather] = 1.0;
        Self {
            current: weather,
            weights,
            temperature: None,
            arriving: weather,
            previous: weather,
            transition: 1.0,
            sky: SkyState::settled(weather),
            cloudiness: cloudiness_target(weather),
            wanted: weather,
            forced: None,
            settled: false,
        }
    }

    /// One frame of the game's weather calc (`0x03667d20`, Wii U v208):
    /// `wanted` is the climate's weather; `frames` of the game's 30 per
    /// second have passed. A new weather is taken only once the last has
    /// (nearly) blown in; then it starts from nothing and the one before it
    /// fades out. A weather kept everywhere (`forced`, like a weather set
    /// for the world) is there at once (the game's immediate path,
    /// `WorldMgr+0x649` set without `+0x64d`), and so is its cloudiness;
    /// so are both, and the sky state, while [`StageTimer`] runs (`snap`;
    /// a weather just taken still starts from nothing in its first frame).
    fn step(&mut self, wanted: usize, frames: f32, snap: bool) {
        if !self.settled {
            *self = Self {
                forced: self.forced,
                temperature: self.temperature,
                settled: true,
                ..Self::settled(wanted)
            };
            return;
        }
        if wanted != self.arriving && self.transition >= WEATHER_TAKEN {
            self.transition = 0.0;
            self.previous = self.arriving;
            self.arriving = wanted;
        } else {
            let step = WEATHER_STEP * frames;
            self.transition = chase(self.transition, 1.0, 0.99, frames, step, step);
            if snap {
                self.transition = 1.0;
            }
        }
        if self.forced.is_some() {
            self.transition = 1.0;
        }
        self.current = self.arriving;
        self.weights = [0.0; WEATHERS.len()];
        self.weights[self.previous] += 1.0 - self.transition;
        self.weights[self.arriving] += self.transition;
        // The cloudiness follows the climate's weather (`0x03672890`), the
        // sky state the weather taken (`WeatherMgr+0x18`).
        let target = cloudiness_target(wanted);
        self.cloudiness = if self.forced.is_some() || snap {
            target
        } else {
            let step = CLOUDINESS_STEP * frames;
            chase(self.cloudiness, target, 0.0, frames, 0.2 * step, step)
        };
        self.sky.step(self.arriving, self.cloudiness, frames, snap);
    }

    /// Keeps weather `forced` (an index into `WEATHERS`) everywhere, or lets
    /// the climates roll theirs again.
    pub fn force(&mut self, forced: Option<usize>) {
        self.forced = forced;
    }

    /// The current weather's name in words ("Heavy Rain").
    pub fn name(&self) -> String {
        words(WEATHERS[self.current])
    }

    /// The weather kept everywhere, if any (see [`Self::force`]).
    pub fn forced(&self) -> Option<usize> {
        self.forced
    }

    /// Weather `index`'s name in words.
    pub fn name_of(index: usize) -> String {
        WEATHERS
            .get(index)
            .map_or_else(String::new, |name| words(name))
    }

    /// The weight of the game's overcast sky state, 0 clear to 1 overcast
    /// ([`SkyState::overcast`]), which picks between a climate's clear and
    /// overcast palette sets and scales the sun's glint on water
    /// (`uking_dynamic_cloud_ratio`).
    pub fn overcast_sky(&self) -> f32 {
        self.sky.overcast()
    }

    /// Which `WeatherInfluence_N` a weather uses, like the game's palette
    /// update (`ENV_UpdateWeatherPalettes` `0x036425b8`, Wii U v208, for
    /// `FeatureColor` and `FeatureFogColor`; `TempMgr` picks the same):
    /// 2 for rain and snow, 3 for heavy rain, heavy snow and thunder rain,
    /// 1 for rain in sunshine, else 0 — cloudy and thunderstorm too.
    pub fn influence_index(weather: usize) -> usize {
        match weather {
            RAIN | SNOW => 2,
            HEAVY_RAIN | HEAVY_SNOW | THUNDER_RAIN => 3,
            BLUESKY_RAIN => 1,
            _ => 0,
        }
    }
}

/// The cloudiness (`SkyMgr+0x2120`) weather `weather` heads for.
fn cloudiness_target(weather: usize) -> f32 {
    if matches!(weather, CLEAR | BLUESKY_RAIN) {
        0.0
    } else {
        1.0
    }
}

/// The game's sky state for the palettes (`EnvMgr+0x184` the state it
/// comes from, `+0x188` the one it heads for, `+0x18c` how far it is; 0
/// clear, 1 overcast, 2 the change of day), as `SKY_UpdateWeatherStateBlend`
/// (`0x03641c44`, Wii U v208) moves it: once a change is done (≥ 0.999) it
/// heads for the clear state under `Bluesky` and `BlueskyRain`
/// (`WeatherMgr+0x18`, getter `0x0366ad14`) and for the overcast one under
/// any other weather once the sky manager's cloudiness `SkyMgr+0x2120`
/// ([`Weather::cloudiness`]) exceeds 0.2; the change chases 1 at
/// `1 − 0.99^t`, at least 0.001 and at most 0.0025 a frame (about 16 s).
///
/// While [`StageTimer`] runs (`WorldMgr+0x53c`) the change is done at
/// once, from the frame after it began.
///
/// Not repeated: the change of
/// day (state 2, `SKY_UpdateDayChangeStateBlend` while `EnvMgr+0x3ceac` >
/// 0.001, near midnight), the event branch (`EnvMgr+0x3cf11`, `0x03641624`),
/// the special row (`WorldMgr+0x624` with row 8: by `WorldMgr+0x5e8`,
/// `0.9^t`) and the immediate settling of `+0x530` = 1 with `+0x638` and
/// `+0x63c` are left out (docs/research/wiiu-deferred-shading.md).
// SI-WTH-02: day-change branches and the special row are left out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyState {
    pub from: usize,
    pub to: usize,
    pub transition: f32,
}

impl SkyState {
    /// The state weather `weather` settles in.
    fn wanted(weather: usize) -> usize {
        match weather {
            CLEAR | BLUESKY_RAIN => SKY_CLEAR,
            _ => SKY_OVERCAST,
        }
    }

    /// The state under weather `weather`, reached.
    pub fn settled(weather: usize) -> Self {
        let state = Self::wanted(weather);
        Self {
            from: state,
            to: state,
            transition: 1.0,
        }
    }

    /// One frame under weather `weather` and the sky manager's cloudiness
    /// `cloudiness`, `frames` of the game's 30 a second; `snap` while
    /// [`StageTimer`] runs.
    fn step(&mut self, weather: usize, cloudiness: f32, frames: f32, snap: bool) {
        if self.transition >= 0.999 {
            self.transition = 1.0;
            let wanted = Self::wanted(weather);
            // An overcast sky waits for the clouds to gather.
            let ready = wanted == SKY_CLEAR || cloudiness > CLOUDINESS_OVERCAST;
            if wanted != self.to && ready {
                self.from = self.to;
                self.to = wanted;
                self.transition = 0.0;
            }
        } else {
            self.transition = chase(
                self.transition,
                1.0,
                0.99,
                frames,
                0.001 * frames,
                0.0025 * frames,
            );
            if snap {
                self.transition = 1.0;
            }
        }
    }

    /// The weight of the overcast state, as the palette update takes it
    /// (`0x036425b8`, the `cloud_ratio` it passes on): the overcast share
    /// of the two states, blended by the transition.
    pub fn overcast(&self) -> f32 {
        let overcast = |state: usize| if state == SKY_OVERCAST { 1.0 } else { 0.0 };
        overcast(self.from) + (overcast(self.to) - overcast(self.from)) * self.transition
    }
}

/// The climate and the weather for the HUD.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ClimateReport<'w> {
    climate: Option<Res<'w, Climate>>,
    weather: Option<Res<'w, Weather>>,
}

impl ClimateReport<'_> {
    /// "climate Central Plain   weather Cloudy   13 C" (the
    /// climate only once the map is read).
    pub fn line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(climate) = self.climate.as_ref().filter(|c| !c.area.is_empty()) {
            parts.push(format!("climate {}   {}", climate.name(), climate.area));
        }
        if let Some(weather) = &self.weather {
            let mut line = format!("weather {}", weather.name());
            if let Some(temperature) = weather.temperature {
                line += &format!("   {temperature:.0} C");
            }
            parts.push(line);
        }
        (!parts.is_empty()).then(|| parts.join("   "))
    }
}

/// `CamelCase` as words.
fn words(name: &str) -> String {
    let mut words = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            words.push(' ');
        }
        words.push(c);
    }
    words
}

/// The weather a climate rolls with `roll` (0–98, like the game's
/// `getU32(99)`), as `WeatherMgr::rollNewWeather` does: clear, cloudy, rain
/// and heavy rain by their odds, a thunderstorm otherwise.
// SI-WTH-03: weather schedule blocks and roll are ours.
pub fn roll_weather(climate: &ClimateDefines, roll: u32) -> usize {
    let [clear, cloudy, rain, heavy, _] = climate.weather_rates;
    let mut random = roll as i32;
    for (weather, rate) in [
        (CLEAR, clear - 1),
        (1, cloudy),
        (RAIN, rain),
        (HEAVY_RAIN, heavy),
    ] {
        if random <= rate {
            return weather;
        }
        random -= rate;
    }
    THUNDER_RAIN
}

/// A stable pseudo-random roll (0–98) for a climate's weather block.
fn weather_roll(climate: usize, block: i64) -> u32 {
    let mut h = (block as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (climate as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h % 99) as u32
}

/// The weather of climate `index` in weather block `block` (counted from the
/// first day's midnight); `cold` turns rain into snow. On the first day the
/// sky stays dry, as the game keeps it until the player has the paraglider
/// (every game starts without it).
fn weather_in_block(defines: &ClimateDefines, index: usize, block: i64, cold: bool) -> usize {
    let start = (block as f32 * WEATHER_HOURS).rem_euclid(24.0);
    let night = !(6.0..18.0).contains(&start);
    if (night && defines.night_lock_blue_sky) || (!night && defines.day_lock_blue_sky) {
        return CLEAR;
    }
    let first_day = (block as f32 * WEATHER_HOURS) < 24.0;
    match roll_weather(defines, weather_roll(index, block)) {
        weather @ (CLEAR | CLOUDY) => weather,
        _ if first_day => CLEAR,
        RAIN if cold => SNOW,
        HEAVY_RAIN if cold => HEAVY_SNOW,
        weather => weather,
    }
}

/// Rolls the weather of the climate and blows it in, like the game's
/// `WeatherMgr` (one weather, the climate's: `0x03672890` asks for the
/// current climate's).
#[allow(clippy::too_many_arguments)]
pub(crate) fn forecast(
    real: Res<Time>,
    epoch: Option<Res<crate::ready::SceneEpoch>>,
    time: Res<TimeOfDay>,
    environment: Res<Environment>,
    climate: Res<Climate>,
    stage: Res<StageTimer>,
    cameras: Query<&GlobalTransform, crate::camera::MainCamera>,
    mut weather: ResMut<Weather>,
) {
    // A new scene (the next camera of a batch) starts in its weather.
    if epoch.is_some_and(|epoch| epoch.is_changed()) {
        weather.settled = false;
    }
    let height = cameras.single().map_or(0.0, |c| c.translation().y);
    let hours = time.day as f32 * 24.0 + time.hours;
    let block = (hours / WEATHER_HOURS).floor() as i64;
    let defines = environment.climates.get(climate.current);
    // SI-WTH-03: weather schedule blocks and roll are ours.
    let temperature = defines.map(|c| c.temperature(height, time.is_night()));
    let wanted = match (weather.forced, defines) {
        (Some(forced), _) => forced,
        (None, Some(defines)) => {
            let cold = temperature.is_some_and(|t| t < 0.0);
            weather_in_block(defines, climate.current, block, cold)
        }
        (None, None) => CLEAR,
    };
    let mut next = weather.clone();
    next.step(wanted, real.delta_secs() * 30.0, stage.running());
    next.temperature = temperature;
    next.wanted = wanted;
    let changed = next
        .weights
        .iter()
        .zip(weather.weights)
        .any(|(a, b)| (a - b).abs() > 1e-4)
        || next.current != weather.current
        || next.previous != weather.previous
        || next.settled != weather.settled
        || next.wanted != weather.wanted
        || (next.transition - weather.transition).abs() > 1e-6
        || match (next.temperature, weather.temperature) {
            (Some(a), Some(b)) => (a - b).abs() > 0.05,
            (a, b) => a.is_some() != b.is_some(),
        };
    if changed {
        *weather = next;
    }
}

/// The weather's factors on the palette's bloom, like the game's
/// `TempMgr` (`TEMPMGR_UpdateMoisture` `0x0365d3c4`, Wii U v208): once a
/// weather has nearly blown in they head for its `WeatherInfluence_N`
/// entry's `BloomThreshhold` and `BloomIntencity`, stay a while after it,
/// then head back to 1. The weather update copies them into `EnvMgr`
/// while the wet timer runs and otherwise heads for 1 the same way
/// (`0x0364e27c`), so one value stands for both; it applies on the field's
/// row of palette sets only (`Environment::scalars`;
/// docs/research/wiiu-render-cpu.md, "The scalar factors").
#[derive(Resource, Clone, Debug)]
pub struct Moisture {
    /// Factor on `BloomThreshhold` (`TempMgr+0x74`).
    pub bloom_threshold: f32,
    /// Factor on `BloomIntencity` (`TempMgr+0x78`).
    pub bloom_intensity: f32,
    /// Game hours the factors still hold after the weather (`TempMgr+0x64`,
    /// in the game's units of 15 per hour).
    wet_hours: f32,
    /// The clock at the last update (game hours since the start); none
    /// before the scene's first.
    clock: Option<f32>,
}

impl Default for Moisture {
    /// `TempMgr`'s reset (`0x0365cd68`): 1, 1, dry.
    fn default() -> Self {
        Self {
            bloom_threshold: 1.0,
            bloom_intensity: 1.0,
            wet_hours: 0.0,
            clock: None,
        }
    }
}

/// The weather transition from which `TempMgr` takes the weather's entry
/// (`TEMPMGR_UpdateMoisture` `0x0365d3c4`: `WeatherMgr+0x14` ≥ 0.9).
const MOISTURE_ARRIVED: f32 = 0.9;

impl Moisture {
    /// Which `WeatherInfluence_N` entry weather `weather` sets the bloom
    /// to and for how many game hours it holds afterwards (`TempMgr+0x64`:
    /// 15 or 0.5 of the game's units, an hour or two minutes); none under
    /// a clear sky.
    fn entry(weather: usize) -> Option<(usize, f32)> {
        // `TimeMgr`'s units: 15 per game hour.
        let hours = |units: f32| units / 15.0;
        match weather {
            CLOUDY | THUNDERSTORM => Some((0, hours(0.5))),
            RAIN => Some((2, hours(15.0))),
            SNOW => Some((2, hours(0.5))),
            HEAVY_RAIN | THUNDER_RAIN => Some((3, hours(15.0))),
            HEAVY_SNOW => Some((3, hours(0.5))),
            BLUESKY_RAIN => Some((1, hours(15.0))),
            _ => None,
        }
    }

    /// Where the factors end up under `weather` and stay (what the viewer
    /// shows without the chase, e.g. in tests).
    pub fn settled(weather: &Weather, entries: &[Influence]) -> Self {
        let mut moisture = Self::default();
        if weather.transition >= MOISTURE_ARRIVED
            && let Some((index, _)) = Self::entry(weather.arriving)
        {
            let entry = entries.get(index).copied().unwrap_or_default();
            moisture.bloom_threshold = entry.bloom_threshold;
            moisture.bloom_intensity = entry.bloom_intensity;
        }
        moisture
    }

    /// One frame: `frames` of the game's 30 per second have passed and the
    /// clock reads `clock` game hours.
    fn update(&mut self, weather: &Weather, entries: &[Influence], frames: f32, clock: f32) {
        let Some(last) = self.clock.replace(clock) else {
            // A scene starts settled, like the climate: the game's reset to
            // 1 and the chase after it (about 3 s) are left out.
            *self = Self {
                clock: Some(clock),
                wet_hours: Self::entry(weather.arriving)
                    .filter(|_| weather.transition >= MOISTURE_ARRIVED)
                    .map_or(0.0, |(_, hold)| hold),
                ..Self::settled(weather, entries)
            };
            return;
        };
        let passed = (clock - last).max(0.0);
        let arrived = (weather.transition >= MOISTURE_ARRIVED)
            .then(|| Self::entry(weather.arriving))
            .flatten();
        let target = match arrived {
            Some((index, hold)) => {
                self.wet_hours = hold;
                let entry = entries.get(index).copied().unwrap_or_default();
                (entry.bloom_threshold, entry.bloom_intensity)
            }
            None => {
                // The timer runs down with the game's clock (`TimeMgr`'s
                // time step, only while time flows).
                self.wet_hours = (self.wet_hours - passed).max(0.0);
                (1.0, 1.0)
            }
        };
        if arrived.is_some() || self.wet_hours <= 0.0 {
            self.bloom_threshold = chase_bloom(self.bloom_threshold, target.0, frames);
            self.bloom_intensity = chase_bloom(self.bloom_intensity, target.1, frames);
        }
    }
}

/// A step of the game's chase towards `target` after `frames` frames at
/// 30 fps: `(1 − base^t)` of the way, at least `least` and at most `most`
/// (the helper shape of `TEMPMGR_UpdateMoisture` `0x0364e27c`, the weather
/// calc `0x03667d20` and `SKY_UpdateWeatherStateBlend` `0x03641c44`).
pub(crate) fn chase(value: f32, target: f32, base: f32, frames: f32, least: f32, most: f32) -> f32 {
    let gap = target - value;
    if gap.abs() <= least {
        return target;
    }
    let step = (gap.abs() * (1.0 - base.powf(frames))).clamp(least, most);
    value + step.copysign(gap)
}

/// Moves the weather's bloom factors on (see [`Moisture`]).
fn dampen(
    real: Res<Time>,
    epoch: Option<Res<crate::ready::SceneEpoch>>,
    time: Res<TimeOfDay>,
    environment: Res<Environment>,
    weather: Res<Weather>,
    mut moisture: ResMut<Moisture>,
) {
    // A new scene (the next camera of a batch) starts afresh.
    if epoch.is_some_and(|epoch| epoch.is_changed()) {
        moisture.clock = None;
    }
    let clock = time.day as f32 * 24.0 + time.hours;
    moisture.update(
        &weather,
        &environment.weather_influences,
        real.delta_secs() * 30.0,
        clock,
    );
}

/// `TempMgr`'s chase of the bloom factors: rate 0.99, steps `0.005·t` to
/// `0.01·t` (`0x0364e27c`).
fn chase_bloom(value: f32, target: f32, frames: f32) -> f32 {
    chase(value, target, 0.99, frames, 0.005 * frames, 0.01 * frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_rolls_like_the_game() {
        let climate = ClimateDefines {
            weather_rates: [60, 20, 10, 5, 5],
            ..ClimateDefines::default()
        };
        let rolls: Vec<usize> = (0..99).map(|r| roll_weather(&climate, r)).collect();
        let count = |w: usize| rolls.iter().filter(|x| **x == w).count();
        assert_eq!(
            (
                count(CLEAR),
                count(1),
                count(RAIN),
                count(HEAVY_RAIN),
                count(THUNDER_RAIN)
            ),
            (60, 20, 10, 5, 4)
        );
        // A desert's clear sky is certain.
        let desert = ClimateDefines {
            weather_rates: [100, 0, 0, 0, 0],
            ..ClimateDefines::default()
        };
        assert!((0..99).all(|r| roll_weather(&desert, r) == CLEAR));
        // Locked by day, rain is snow in the cold.
        let locked = ClimateDefines {
            weather_rates: [0, 0, 100, 0, 0],
            day_lock_blue_sky: true,
            ..ClimateDefines::default()
        };
        assert_eq!(weather_in_block(&locked, 3, 9, false), CLEAR);
        assert_eq!(weather_in_block(&locked, 3, 6, false), RAIN);
        assert_eq!(weather_in_block(&locked, 3, 6, true), SNOW);
        // The first day stays dry.
        assert_eq!(weather_in_block(&locked, 3, 0, false), CLEAR);
        assert_eq!(Weather::influence_index(HEAVY_SNOW), 3);
        // Like the game, clouds alone keep the clear weather's tint; only
        // rain in sunshine takes influence 1.
        assert_eq!(Weather::influence_index(CLOUDY), 0);
        assert_eq!(Weather::influence_index(BLUESKY_RAIN), 1);
        assert_eq!(
            Weather {
                current: HEAVY_RAIN,
                ..default()
            }
            .name(),
            "Heavy Rain"
        );
    }

    #[test]
    fn default_is_the_plains() {
        let climate = Climate::default();
        assert_eq!(climate.shares(), [(0, 0.0), (0, 1.0)]);
        assert_eq!(climate.rows.shares(), [(0, 0.0), (0, 1.0)]);
        assert_eq!(climate.name(), "Central Plain");
        assert_eq!(
            Climate {
                current: 15,
                ..default()
            }
            .name(),
            "Dark Woods"
        );
        assert_eq!(
            Climate {
                current: 19,
                ..default()
            }
            .name(),
            "Gerudo Desert Lv2"
        );
        assert_eq!(
            Climate {
                current: 18,
                ..default()
            }
            .name(),
            "Korog Forest"
        );
    }

    #[test]
    fn a_new_weather_blows_in_over_some_seventeen_seconds() {
        let mut weather = Weather::default();
        // A scene starts in its weather.
        weather.step(RAIN, 1.0, false);
        assert_eq!((weather.current, weather.transition), (RAIN, 1.0));
        // A new weather is taken at once and starts from nothing…
        weather.step(CLOUDY, 1.0, false);
        assert_eq!(
            (weather.current, weather.previous, weather.transition),
            (CLOUDY, RAIN, 0.0)
        );
        assert_eq!(weather.weights[RAIN], 1.0);
        // …then gains 0.002 a frame at 30 fps.
        for _ in 0..250 {
            weather.step(CLOUDY, 1.0, false);
        }
        assert!((weather.transition - 0.5).abs() < 1e-3);
        assert!((weather.weights[RAIN] - 0.5).abs() < 1e-3);
        // Another weather waits until this one has (nearly) blown in.
        let mut frames = 250;
        while weather.current == CLOUDY {
            weather.step(CLEAR, 1.0, false);
            frames += 1;
        }
        assert!((495..=497).contains(&frames), "{frames}");
        assert_eq!((weather.current, weather.previous), (CLEAR, CLOUDY));
        // A weather kept everywhere is there at once.
        weather.force(Some(HEAVY_RAIN));
        weather.step(HEAVY_RAIN, 1.0, false);
        assert_eq!(weather.weights[HEAVY_RAIN], 0.0);
        for _ in 0..500 {
            weather.step(HEAVY_RAIN, 1.0, false);
        }
        weather.step(SNOW, 1.0, false);
        assert_eq!((weather.current, weather.transition), (SNOW, 1.0));
        assert_eq!(weather.weights[SNOW], 1.0);
    }

    #[test]
    fn the_sky_overcasts_with_the_weather_not_with_its_share() {
        let mut sky = SkyState::settled(CLEAR);
        assert_eq!(sky.overcast(), 0.0);
        // Rain in sunshine keeps the sky clear.
        sky.step(BLUESKY_RAIN, 0.0, 1.0, false);
        assert_eq!(sky.overcast(), 0.0);
        // Any other weather waits for the clouds to gather ...
        sky.step(CLOUDY, 0.2, 1.0, false);
        assert_eq!(sky.to, SKY_CLEAR);
        // ... then heads for the overcast state at once, whatever its
        // share, and gets there in some 16 s.
        sky.step(CLOUDY, 0.21, 1.0, false);
        assert_eq!(
            (sky.from, sky.to, sky.transition),
            (SKY_CLEAR, SKY_OVERCAST, 0.0)
        );
        let mut frames = 0;
        while sky.transition < 1.0 {
            sky.step(CLOUDY, 1.0, 1.0, false);
            frames += 1;
        }
        assert!((450..550).contains(&frames), "{frames}");
        assert_eq!(sky.overcast(), 1.0);
        // The first 300 frames at most 0.0025 each.
        let mut clearing = sky;
        clearing.step(CLEAR, 1.0, 1.0, false);
        for _ in 0..100 {
            clearing.step(CLEAR, 1.0, 1.0, false);
        }
        assert!((clearing.overcast() - 0.75).abs() < 1e-3);
        // The weather's state is followed through the transition, once the
        // clouds have gathered: 0.0015 a frame, over 0.2 after 134 frames
        // and all there after 667 (some 22 s).
        let mut weather = Weather::default();
        weather.step(CLEAR, 1.0, false);
        weather.step(RAIN, 1.0, false);
        assert_eq!((weather.sky.to, weather.weights[RAIN]), (SKY_CLEAR, 0.0));
        let mut frames = 1;
        while weather.sky.to != SKY_OVERCAST {
            weather.step(RAIN, 1.0, false);
            frames += 1;
        }
        assert!((134..=135).contains(&frames), "{frames}");
        while weather.cloudiness < 1.0 {
            weather.step(RAIN, 1.0, false);
            frames += 1;
        }
        assert!((665..=668).contains(&frames), "{frames}");
        // A weather set for the world brings its clouds at once.
        let mut set = Weather::default();
        set.step(CLEAR, 1.0, false);
        set.force(Some(RAIN));
        set.step(RAIN, 1.0, false);
        assert_eq!((set.cloudiness, set.sky.to), (1.0, SKY_OVERCAST));
    }

    #[test]
    fn rain_lowers_the_bloom_threshold_gradually_and_it_lingers() {
        let entry = |bloom_threshold, bloom_intensity| Influence {
            bloom_threshold,
            bloom_intensity,
            ..Influence::default()
        };
        let entries = [
            entry(1.0, 1.0),
            entry(0.1, 1.0),
            entry(0.5, 1.25),
            entry(0.25, 1.5),
        ];
        let under = |weather: usize, transition: f32| Weather {
            arriving: weather,
            transition,
            ..Weather::default()
        };
        let mut moisture = Moisture::default();
        let mut clock = 10.0;
        // One frame at 30 fps, the clock at a game minute per second.
        let mut frame = |moisture: &mut Moisture, weather: &Weather| {
            clock += 1.0 / 60.0 / 30.0;
            moisture.update(weather, &entries, 1.0, clock);
        };
        // Rain still blowing in: nothing yet.
        frame(&mut moisture, &under(RAIN, 0.5));
        assert_eq!(moisture.bloom_threshold, 1.0);
        // Once it has blown in, the threshold falls a little each frame…
        frame(&mut moisture, &under(RAIN, 0.95));
        assert!(moisture.bloom_threshold < 1.0 && moisture.bloom_threshold > 0.9);
        // …and reaches the rain's entry within a few seconds.
        for _ in 0..120 {
            frame(&mut moisture, &under(RAIN, 1.0));
        }
        assert_eq!(
            (moisture.bloom_threshold, moisture.bloom_intensity),
            (0.5, 1.25)
        );
        // After the rain it holds for a game hour (a game minute a second)…
        for _ in 0..(55 * 30) {
            frame(&mut moisture, &under(CLEAR, 1.0));
        }
        assert_eq!(moisture.bloom_threshold, 0.5);
        // …then heads back to 1.
        for _ in 0..(10 * 30) {
            frame(&mut moisture, &under(CLEAR, 1.0));
        }
        assert_eq!(
            (moisture.bloom_threshold, moisture.bloom_intensity),
            (1.0, 1.0)
        );
        // A scene that starts in snow starts with its bloom; snow holds it
        // for two game minutes only.
        let mut snow = Moisture::default();
        frame(&mut snow, &under(SNOW, 1.0));
        assert_eq!(snow.bloom_threshold, 0.5);
        for _ in 0..(3 * 30) {
            frame(&mut snow, &under(CLEAR, 1.0));
        }
        assert!(snow.bloom_threshold > 0.5);
        // Where it settles, without the chase.
        assert_eq!(
            Moisture::settled(&under(HEAVY_RAIN, 1.0), &entries).bloom_threshold,
            0.25
        );
        assert_eq!(
            Moisture::settled(&under(CLEAR, 1.0), &entries).bloom_threshold,
            1.0
        );
    }

    fn world_frame(camera: Vec3, climate: &dyn Fn(Vec3) -> Option<usize>) -> WorldFrame<'_> {
        WorldFrame {
            camera,
            player: None,
            climate_at: climate,
        }
    }

    /// Frames until the timer runs out, the managers reading it before the
    /// world's frame as in the viewer's frame.
    fn frames_running(stage: &mut StageTimer, world: &WorldFrame, climate: &mut Climate) -> u32 {
        let mut frames = 0;
        while stage.running() {
            stage.step(world, 1.0, climate);
            frames += 1;
        }
        frames
    }

    #[test]
    fn a_new_stage_takes_targets_at_once_for_a_second() {
        let mut stage = StageTimer::default();
        let mut climate = Climate::default();
        // The first frame sees the camera come down 200 m and sets 30 again.
        let still = world_frame(Vec3::new(10.0, 50.0, -3.0), &|_| Some(4));
        assert_eq!(frames_running(&mut stage, &still, &mut climate), 31);
        // The stage begins in the camera's climate, no change under way.
        assert_eq!(climate.shares(), [(4, 0.0), (4, 1.0)]);
    }

    #[test]
    fn a_jump_of_the_camera_or_the_player_takes_targets_at_once() {
        let mut stage = StageTimer::default();
        let mut climate = Climate::default();
        let none = |_: Vec3| None;
        let mut world = world_frame(Vec3::ZERO, &none);
        frames_running(&mut stage, &world, &mut climate);
        // 19 m in a frame is not enough; 20 m is, seen a frame later (the
        // check runs before the positions move on).
        world.camera.x += 19.0;
        stage.step(&world, 1.0, &mut climate);
        stage.step(&world, 1.0, &mut climate);
        assert!(!stage.running());
        world.camera.x += 20.0;
        stage.step(&world, 1.0, &mut climate);
        assert!(!stage.running());
        stage.step(&world, 1.0, &mut climate);
        assert_eq!(stage.timer, 30.0);
        // The player's jump counts too.
        frames_running(&mut stage, &world, &mut climate);
        world.player = Some(Vec3::ZERO);
        stage.step(&world, 1.0, &mut climate);
        world.player = Some(Vec3::new(0.0, 0.0, 25.0));
        stage.step(&world, 1.0, &mut climate);
        stage.step(&world, 1.0, &mut climate);
        assert!(stage.running());
    }

    #[test]
    fn a_new_climate_eases_in_over_six_seconds_and_the_woods_at_once() {
        let mut stage = StageTimer::default();
        let mut climate = Climate::default();
        // West of x = 0 climate 0, east of it the Dark Woods; climate 1
        // north of z = 0.
        let map = |at: Vec3| {
            Some(if at.x > 0.0 {
                DARK_WOODS
            } else if at.z < 0.0 {
                1
            } else {
                0
            })
        };
        // Steps of a metre or two: no jumps.
        let mut world = world_frame(Vec3::new(-1.0, 0.0, 1.0), &map);
        frames_running(&mut stage, &world, &mut climate);
        // A change starts the climate's transition from the former one…
        world.camera.z = -1.0;
        stage.step(&world, 1.0, &mut climate);
        assert_eq!(climate.shares(), [(0, 1.0), (1, 0.0)]);
        // …and none is looked for until it is done: 1% of the way a frame
        // until half is left, then 0.005, some 170 frames.
        world.camera.x = 1.0;
        let mut frames = 0;
        while climate.transition < 1.0 {
            stage.step(&world, 1.0, &mut climate);
            assert_eq!(climate.current, 1);
            frames += 1;
        }
        assert!((165..=172).contains(&frames), "{frames}");
        assert!(!stage.running());
        // Then the Dark Woods, whose darkness has not come in: at once,
        // and the timer runs for three seconds.
        stage.step(&world, 1.0, &mut climate);
        assert_eq!((climate.current, climate.transition), (DARK_WOODS, 1.0));
        assert_eq!(stage.timer, 90.0);
        // Once the darkness is all there, the woods are eased into.
        let mut dark = Climate::settled(0, DARKNESS_ROW);
        let mut stage = StageTimer::default();
        world.camera = Vec3::new(-1.0, 0.0, 1.0);
        frames_running(&mut stage, &world, &mut dark);
        world.camera.x = 1.0;
        stage.step(&world, 1.0, &mut dark);
        assert_eq!(
            (dark.current, dark.transition, stage.timer),
            (DARK_WOODS, 0.0, 0.0)
        );
    }

    #[test]
    fn the_rows_of_palette_sets_follow_the_climate() {
        let mut rows = PaletteRows::default();
        // Into the Lost Woods' row: from 0, exactly 0.005 a frame.
        rows.step(1, 1.0, false);
        assert_eq!((rows.previous, rows.active), (0, 1));
        assert!((rows.transition - 0.005).abs() < 1e-6);
        let mut frames = 1;
        while rows.transition < 1.0 {
            rows.step(1, 1.0, false);
            rows.count_down(1.0);
            frames += 1;
        }
        assert!((199..=201).contains(&frames), "{frames}");
        // Back in the field the row is held four more updates, then eased
        // out.
        for _ in 0..4 {
            rows.step(0, 1.0, false);
            rows.count_down(1.0);
            assert_eq!(rows.active, 1);
        }
        rows.step(0, 1.0, false);
        assert_eq!((rows.previous, rows.active), (1, 0));
        // Out of the darkness's row it takes twice as long.
        let mut dark = PaletteRows::settled(DARKNESS_ROW);
        let mut frames = 0;
        loop {
            dark.step(0, 1.0, false);
            dark.count_down(1.0);
            frames += 1;
            if dark.active == 0 && dark.transition >= 1.0 {
                break;
            }
        }
        assert!((400..=410).contains(&frames), "{frames}");
        // While the timer runs a change is there at once, and only the
        // darkness's row counts as the darkness.
        let mut snap = PaletteRows::default();
        snap.step(DARKNESS_ROW, 1.0, true);
        assert_eq!((snap.active, snap.darkness()), (DARKNESS_ROW, 1.0));
        assert_eq!(PaletteRows::settled(1).darkness(), 0.0);
    }

    #[test]
    fn the_timer_brings_the_weather_and_its_clouds_at_once() {
        let mut weather = Weather::default();
        weather.step(CLEAR, 1.0, false);
        // A weather taken under the timer starts from nothing for a frame,
        // then is there, its clouds and its sky with it.
        weather.step(RAIN, 1.0, true);
        assert_eq!((weather.transition, weather.cloudiness), (0.0, 1.0));
        assert_eq!(weather.sky.to, SKY_OVERCAST);
        weather.step(RAIN, 1.0, true);
        assert_eq!((weather.transition, weather.sky.transition), (1.0, 1.0));
        assert_eq!(weather.overcast_sky(), 1.0);
    }

    #[test]
    fn viewer_climate_override_applies_without_an_ecosystem_map() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(ClimateMap {
                loading: None,
                map: None,
                located: false,
            })
            .insert_resource(Climate::default())
            .insert_resource(StageTimer::default())
            .insert_resource(ClimateOverride(Some(3)))
            .add_systems(Update, super::world_frame);
        app.world_mut().spawn((
            Camera3d::default(),
            crate::camera::MainView,
            GlobalTransform::IDENTITY,
        ));
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().resource::<Climate>().current, 3);
        app.world_mut().resource_mut::<ClimateOverride>().0 = Some(5);
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().resource::<Climate>().current, 5);
    }
}
