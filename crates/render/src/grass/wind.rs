//! The game's grass wind (docs/research/wiiu-field-shading.md, "Grass wind"):
//! the world's wind manager (`world::Manager` job 5, update `0x0366ccbc`)
//! smooths the world's wind into a strength, a direction and a blend from
//! the previous direction, and the environment writer (`0x033ff8cc`) hands
//! them to the grass shaders as `gsys_environment` 32, 33 and 57–60. The
//! swells that roll across the field are the texture `grass_wind_swell`,
//! which the game computes once at start (`0x039407b8`).
//!
//! The world's wind itself (`world::Manager`): the camera's climate's
//! `WindPower` times a multiplier, blowing along one of seven directions;
//! both are rerolled every game hour (`0x036662ec` from `0x03667d20`), the
//! multiplier also while the grass wind is faded out, and the direction
//! turns slowly towards the rolled one (`0x036783e0`).
//!
//! The managers step once per game frame (30 a second); the viewer steps
//! them as many whole frames as have passed.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Game frames per second the manager's per-frame steps assume.
pub const FRAME_RATE: f32 = 30.0;

/// `grass_wind_swell`: 320 × 64 half floats (`0x03940a78`).
pub const SWELL_WIDTH: usize = 320;
pub const SWELL_HEIGHT: usize = 64;

/// The swell texture's clock runs over `2·4800` frames (`0x03940300`,
/// `0x03940c04`); the shaders get it divided by 4800.
const SWELL_PERIOD: f32 = 4800.0;
/// Wind speed at which the grass wind is full (`DAT_1047be4c`).
const FULL_SPEED: f32 = 15.0;
/// The strength's divisor in the shaders' `e32.x` (`DAT_1047be5c`).
const STRENGTH_DIVISOR: f32 = 12.0;
/// Frames over which a new direction blends in (`DAT_1047be50`).
const TURN_FRAMES: f32 = 120.0;
/// Strength steps per frame towards its target (`0x0366ccbc`).
const STRENGTH_STEP: f32 = 1.0 / 300.0;
/// Gust phase per frame at full strength (`DAT_1047be60`).
const PHASE_STEP: f32 = 0.0001;
/// The slow wave in the wind's speed, per frame (`DAT_1047be48`).
const WAVE_STEP: f32 = 0.001;
/// The gust spots' directions are the wind's turned by ±0.2 rad
/// (`0x033ff8cc`), their pattern 27 m across; the swell's min and max are
/// divided by 0.62 (`0x03940ccc`, `0x03940d70`).
const SPOT_TURN: f32 = 0.2;
const SPOT_SIZE: f32 = 27.0;
const SWELL_NORM: f32 = 0.619_999_77;

/// `WindPower`'s default (`world::Manager`'s climate parameters), for
/// when there is no world info.
pub const DEFAULT_POWER: f32 = 5.0;

/// The swell of `TeraGrass`'s `Blade1` and `Cross1` (`uking_grass_wind_
/// swell_*`, docs/research/wiiu-field-shading.md) for when there is no dump,
/// as [`swell_params`] packs them.
pub const BLADE_SWELL: Vec4 = Vec4::new(9.0 / 0.022295, 0.005 / 0.022295, 2.0, 0.022295);
pub const TUFT_SWELL: Vec4 = Vec4::new(9.0 / 0.022295, 0.0, 1.0, 0.022295);

/// A material's swell for the shaders: the swell's frequency and its
/// per-blade dispersion over the world coefficient (the blade shader's
/// 403.68 and 0.2243), the swell's scale and the world coefficient.
pub fn swell_params(frequency: f32, dispersion: f32, scale: f32, coefficient: f32) -> Vec4 {
    Vec4::new(
        frequency / coefficient,
        dispersion / coefficient,
        scale,
        coefficient,
    )
}

/// A baked `TeraGrass` swell (`asset_format::grass::Swell`) for the
/// shaders; `None` without a world coefficient.
pub fn swell_of(swell: &asset_format::grass::Swell) -> Option<Vec4> {
    let swell = swell_params(
        swell.freq_scale,
        swell.dispersion_scale,
        swell.scale,
        swell.world_transform_coef,
    );
    Some(swell).filter(|s| s.w > 0.0)
}

/// The turn towards a rolled direction per frame: the share `1 − 0.9^dt`
/// of what is left, between 0.001 and 0.005 rad (`0x036783e0`).
const DIRECTION_EASE: f32 = 0.9;
const DIRECTION_STEP: (f32, f32) = (0.001, 0.005);

// SI-GRS-07: steady wind, dungeon branch and xorshift are ours.
/// The world's wind-power multiplier for a roll `r` in 0-1: 0.2 to 1
/// (`0x036661d0`; 0.2 to 0.67 until `FindDungeon_Activated`, which the
/// viewer takes as set).
pub fn wind_multiplier(r: f32) -> f32 {
    0.2 + 0.8 * r
}

/// The angle of a direction type 0–6 (`0x03666158` rolls 0–6; type `k`
/// points along (0, 1) turned by −k·45°): x = sin θ, z = cos θ is where the
/// wind blows, type 0 to +Z (south), 2 to −X (west).
pub fn direction_angle(kind: u32) -> f32 {
    -(kind as f32) * std::f32::consts::FRAC_PI_4
}

/// One texel of `grass_wind_swell` (`0x03940408`): `u` along the wind, `v`
/// the wind's strength. Weak winds ripple (the cosines), strong ones swell
/// (the sines).
pub fn swell(u: f32, v: f32) -> f32 {
    let ripple = (2.0 * v.min(1.0 - v)).clamp(0.0, 1.0);
    let surge = (v - 0.3).clamp(0.0, 1.0) / 0.7;
    let x = u * std::f32::consts::TAU;
    let cosines = (2.0 * x).cos() * (3.0 * x).cos() * (5.0 * x).cos();
    let sines = (3.0 * x).sin() * (5.0 * x).sin() * (7.0 * x).sin();
    0.3 * (ripple * (1.0 - ripple * cosines) * 0.233_333_33 + surge * (1.0 + 0.6 * surge * sines))
}

/// A float as the game stores it in the texture (`0x039406d4`): the
/// mantissa cut, not rounded, and values below the half range flushed to 0.
fn half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    if bits & 0x7f80_0000 == 0x7f80_0000 {
        return sign
            | if bits & 0x7f_ffff == 0 {
                0x7c00
            } else if bits & 0x7f_ffff > 0x3f_ffff {
                0x7fff
            } else {
                0x7dff
            };
    }
    let exponent = ((bits & 0x7f80_0000) >> 23) as i32 - 0x70;
    if exponent > 0x1e {
        sign | 0x7c00
    } else if exponent > 0 {
        sign | ((exponent as u32 * 0x400) & 0x7c00) as u16 | ((bits >> 13) & 0x3ff) as u16
    } else {
        sign
    }
}

#[cfg(test)]
fn half_value(bits: u16) -> f32 {
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let mantissa = (bits & 0x3ff) as f32;
    let magnitude = if exponent == 0 {
        mantissa * 2f32.powi(-24)
    } else {
        (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15)
    };
    if bits & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

/// `grass_wind_swell` as half-float bits, row by row, with each row's least
/// and greatest value (the game's tables `0x10595454`, `0x10595554`, kept as
/// the floats it computed).
pub struct SwellTexture {
    pub texels: Vec<u16>,
    pub row_min: [f32; SWELL_HEIGHT],
    pub row_max: [f32; SWELL_HEIGHT],
}

impl SwellTexture {
    pub fn new() -> Self {
        let mut texels = Vec::with_capacity(SWELL_WIDTH * SWELL_HEIGHT);
        let (mut row_min, mut row_max) = ([0.0; SWELL_HEIGHT], [0.0; SWELL_HEIGHT]);
        for j in 0..SWELL_HEIGHT {
            let v = j as f32 / SWELL_HEIGHT as f32;
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for i in 0..SWELL_WIDTH {
                let value = swell(i as f32 / SWELL_WIDTH as f32, v);
                texels.push(half_bits(value));
                lo = lo.min(value);
                hi = hi.max(value);
            }
            row_min[j] = lo;
            row_max[j] = hi;
        }
        Self {
            texels,
            row_min,
            row_max,
        }
    }

    /// A texel's value as a float.
    #[cfg(test)]
    pub fn value(&self, i: usize, j: usize) -> f32 {
        half_value(self.texels[j * SWELL_WIDTH + i])
    }

    /// The row the strength picks, as the game looks it up (`s·63`).
    fn row(strength: f32) -> usize {
        (strength.clamp(0.0, 1.0) * 63.0) as usize
    }

    pub fn image(&self) -> Image {
        let data = self.texels.iter().flat_map(|t| t.to_le_bytes()).collect();
        let mut image = Image::new(
            Extent3d {
                width: SWELL_WIDTH as u32,
                height: SWELL_HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::R16Float,
            RenderAssetUsages::RENDER_WORLD,
        );
        // The swell scrolls along u; v is the strength, 0-1.
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        });
        image
    }
}

impl Default for SwellTexture {
    fn default() -> Self {
        Self::new()
    }
}

/// The fade that silences the grass wind while the weather rerolls the
/// climates' wind powers (`0x0366c80c`): every 60–90 s of wind no faster
/// than 10, it fades out over 300 frames and back in over 60.
#[derive(Clone, Debug, PartialEq)]
struct Fade {
    active: bool,
    state: u32,
    value: f32,
    timer: f32,
}

impl Default for Fade {
    fn default() -> Self {
        Self {
            active: false,
            state: 0,
            value: 1.0,
            timer: 0.0,
        }
    }
}

impl Fade {
    /// One frame of `dt` frames; `random` in 0-1 draws the next wait.
    fn step(&mut self, dt: f32, speed: f32, random: f32) {
        match self.state {
            0 if self.active => self.state = 1,
            0 if self.timer <= 0.0 => self.timer = (random + 2.0) * 30.0 * 60.0,
            0 => {
                self.timer -= dt;
                if self.timer < 0.0 {
                    self.timer = 0.0;
                    self.active = speed <= 10.0;
                }
            }
            1 => {
                self.value -= dt / 300.0;
                if self.value < 0.0 {
                    self.value = 0.0;
                    self.state = 2;
                }
            }
            // The game rerolls the climates' wind powers here.
            2 => self.state = 3,
            3 => {
                self.value += dt / 60.0;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.state = 4;
                }
            }
            _ => {
                self.active = false;
                self.state = 0;
            }
        }
    }
}

/// The grass wind's state and the world's wind it follows.
#[derive(Resource)]
pub struct GrassWind {
    /// The camera's climate's `WindPower` (the game's units; 15 is full).
    pub power: f32,
    /// The rolled multiplier (1 until the first roll, as the manager starts).
    multiplier: f32,
    /// The rolled direction type and the angle turning towards it.
    kind: u32,
    angle: f32,
    /// The game hour the last roll was for.
    hour: Option<i64>,
    /// The world's wind speed, `power × multiplier`.
    speed: f32,
    /// The world's wind direction (x, z), `(sin θ, cos θ)`.
    direction: Vec2,
    /// Game frames not yet stepped.
    pending: f32,
    /// The slow wave in the speed (`+0x1c`).
    wave: f32,
    /// The grass wind's strength, 0-1 (`+0x54`).
    strength: f32,
    /// Current and previous direction (`+0x3c`, `+0x44`) and how much of
    /// the previous one is left, 1 → 0 (`+0x50`).
    current: Vec2,
    previous: Vec2,
    turn: f32,
    /// The gust spots' phase, 0-1 (`+0x58`).
    phase: f32,
    fade: Fade,
    /// The swell texture's clock in frames, 0 to 2·4800.
    clock: f32,
    random: u64,
    table: SwellTexture,
    pub swell: Handle<Image>,
}

impl GrassWind {
    /// Settled on the climate's `power` from the south, as if the game had
    /// been running (at boot the game starts calm and turns from no
    /// direction). The fields start as the game's `WindMgr` constructor
    /// (`0x0366c2d0`) sets them: flag 3, turn, phase and the fade.
    // SI-GRS-07: steady wind, dungeon branch and xorshift are ours.
    pub fn new(power: f32, swell: Handle<Image>, table: SwellTexture) -> Self {
        let direction = Vec2::new(0.0, 1.0);
        let mut wind = Self {
            power,
            multiplier: 1.0,
            kind: 0,
            angle: 0.0,
            hour: None,
            speed: power,
            direction,
            pending: 0.0,
            wave: 0.0,
            strength: 0.0,
            current: direction,
            previous: direction,
            turn: 0.0,
            phase: 0.0,
            fade: Fade::default(),
            clock: 0.0,
            random: 0x2545_f491_4f6c_dd1d,
            table,
            swell,
        };
        wind.strength = wind.target();
        wind
    }

    fn next_random(&mut self) -> f32 {
        self.random ^= self.random >> 12;
        self.random ^= self.random << 25;
        self.random ^= self.random >> 27;
        (self.random.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// The speed the grass wind follows: the world's, waving slowly by up
    /// to 30 % (`0x0366ccbc` with its flag 3 set, as constructed), faded.
    fn wind_speed(&self) -> f32 {
        let c = -((3.0 * self.wave).cos() * (5.0 * self.wave).cos() * (7.0 * self.wave).cos());
        (1.0 - 0.3 * c.clamp(0.0, 1.0)) * self.speed * self.fade.value
    }

    /// The strength the wind heads for: `√sat(speed/15)`.
    fn target(&self) -> f32 {
        (self.wind_speed() / FULL_SPEED).clamp(0.0, 1.0).sqrt()
    }

    /// The world's wind speed and direction (x, z): `world::Manager`'s
    /// `getWindSpeed` and `getWindDirection` (`0x03672fe8`, `0x03672ec8`),
    /// which the sky manager follows too (`clouds::SkyWind`).
    pub fn world(&self) -> (f32, Vec2) {
        (self.speed, self.direction)
    }

    /// Rolls the multiplier and the direction when the game `hour` (hours
    /// since the first midnight) changes (`0x03667d20`); the first hour
    /// seen only starts the count.
    pub fn set_hour(&mut self, hour: i64) {
        if self.hour.is_some_and(|h| h != hour) {
            self.multiplier = wind_multiplier(self.next_random());
            self.kind = ((self.next_random() * 7.0) as u32).min(6);
        }
        self.hour = Some(hour);
    }

    /// Advances by `seconds` of game time in whole game frames.
    pub fn advance(&mut self, seconds: f32) {
        self.pending += seconds * FRAME_RATE;
        // SI-GRS-07: steady wind, dungeon branch and xorshift are ours.
        // Long stalls (loading) do not replay minutes of frames.
        self.pending = self.pending.min(FRAME_RATE);
        while self.pending >= 1.0 {
            self.pending -= 1.0;
            self.step(1.0);
        }
    }

    /// One game frame of `dt` frames: the world's wind (`0x036783e0`,
    /// `0x03672fe8`), then the grass wind (`0x0366ccbc`).
    fn step(&mut self, dt: f32) {
        let target = direction_angle(self.kind);
        let left = target - self.angle;
        let turn = (left.abs() * (1.0 - DIRECTION_EASE.powf(dt)))
            .clamp(DIRECTION_STEP.0 * dt, DIRECTION_STEP.1 * dt);
        self.angle = if left.abs() <= DIRECTION_STEP.0 * dt {
            target
        } else {
            self.angle + turn.copysign(left)
        };
        self.direction = Vec2::new(self.angle.sin(), self.angle.cos());
        self.speed = self.power * self.multiplier;

        let random = self.next_random();
        let state = self.fade.state;
        self.fade.step(dt, self.speed, random);
        // Faded out: the weather rerolls the multiplier (`0x0367a120`).
        if state == 2 {
            self.multiplier = wind_multiplier(self.next_random());
        }
        self.wave += WAVE_STEP;
        self.clock += dt;
        if self.clock >= 2.0 * SWELL_PERIOD {
            self.clock -= 2.0 * SWELL_PERIOD;
        }
        let direction = self.direction.normalize_or_zero();
        if direction != self.current && (self.fade.state == 2 || self.turn == 0.0) {
            self.turn = 1.0;
            self.previous = self.current;
            self.current = direction;
        }
        self.turn = (self.turn - 1.0 / TURN_FRAMES).max(0.0);
        let target = self.target();
        self.strength = if self.strength < target {
            (self.strength + STRENGTH_STEP).min(target)
        } else {
            (self.strength - STRENGTH_STEP).max(target)
        };
        self.phase += PHASE_STEP * self.strength;
        if self.phase > 1.0 {
            self.phase -= 1.0;
        }
    }

    /// `gsys_environment` 32, 33, 57, 58, 59 and 60 as the environment
    /// writer fills them (`0x033ff8cc`).
    pub fn environment(&self) -> [Vec4; 6] {
        let s = (self.strength / STRENGTH_DIVISOR * FULL_SPEED * self.fade.value).clamp(0.0, 1.0);
        let w = self.turn * self.turn;
        let (current, previous) = (self.current, self.previous);
        let turned = |angle: f32, v: Vec2| {
            let (sin, cos) = angle.sin_cos();
            Vec2::new(cos * v.x - sin * v.y, sin * v.x + cos * v.y) / SPOT_SIZE
        };
        let row = SwellTexture::row(s);
        let (a, b) = (turned(SPOT_TURN, current), turned(SPOT_TURN, previous));
        let (c, d) = (turned(-SPOT_TURN, current), turned(-SPOT_TURN, previous));
        [
            Vec4::new(s, current.x, current.y, self.clock / SWELL_PERIOD),
            Vec4::new(w, previous.x, previous.y, 1.0 - w),
            Vec4::new(
                current.x * (1.0 - w),
                current.y * (1.0 - w),
                previous.x * w,
                previous.y * w,
            ),
            Vec4::new(
                self.table.row_max[row] / SWELL_NORM,
                self.table.row_min[row] / SWELL_NORM,
                self.phase * 25.0 * 6.0 * std::f32::consts::PI,
                self.phase * 30.0,
            ),
            Vec4::new(a.x, a.y, b.x, b.y),
            Vec4::new(c.x, c.y, d.x, d.y),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wind(power: f32) -> GrassWind {
        GrassWind::new(power, Handle::default(), SwellTexture::new())
    }

    #[test]
    fn swell_rows_run_from_ripple_to_surge() {
        let table = SwellTexture::new();
        // Calm: nothing.
        assert!(table.row_max[0].abs() < 1e-6);
        // The weakest winds ripple about a small mean, the strongest surge.
        assert!(table.row_max[10] < 0.2 && table.row_min[10] > 0.0);
        assert!(table.row_min[63] > 0.1 && table.row_max[63] > 0.4);
        // Stored as the game's half floats, close to the floats.
        let v = 40.0 / 64.0;
        assert!((table.value(17, 40) - swell(17.0 / 320.0, v)).abs() < 1e-3);
    }

    #[test]
    fn half_floats_are_cut_like_the_game() {
        assert_eq!(half_bits(1.0), 0x3c00);
        assert_eq!(half_bits(-2.0), 0xc000);
        // Cut, not rounded: just below 1 + 1/1024 stays 1.
        assert_eq!(half_bits(1.0 + 0.9 / 1024.0), 0x3c00);
        // Below the normal range: zero.
        assert_eq!(half_bits(1e-6), 0);
        assert_eq!(half_value(0x3c01), 1.0 + 1.0 / 1024.0);
    }

    #[test]
    fn settled_wind_follows_the_speed() {
        // Speed 5 of the full 15: strength √(1/3) at most, a little less on
        // the slow wave's low side; e32.x = 1.25 × that.
        let e = wind(5.0).environment();
        assert!(
            e[0].x > 0.6 && e[0].x <= 1.25 * (1.0f32 / 3.0).sqrt() + 1e-6,
            "{}",
            e[0].x
        );
        // Settled: all of the current direction.
        assert_eq!(e[2], Vec4::new(0.0, 1.0, 0.0, 0.0));
        assert_eq!(wind(0.0).environment()[0].x, 0.0);
    }

    #[test]
    fn a_new_direction_blends_in_over_four_seconds() {
        let mut wind = wind(10.0);
        // The grass wind still blows west; the world's wind is south.
        wind.current = Vec2::new(-1.0, 0.0);
        wind.step(1.0);
        let e = wind.environment();
        // Nearly all of the old direction still.
        assert!(
            e[1].x > 0.95 && (e[1].y, e[1].z) == (-1.0, 0.0),
            "{:?}",
            e[1]
        );
        assert_eq!((e[0].y, e[0].z), (0.0, 1.0));
        for _ in 0..120 {
            wind.step(1.0);
        }
        assert_eq!(wind.environment()[1].x, 0.0);
    }

    #[test]
    fn the_rolled_direction_is_turned_to_slowly() {
        let mut wind = wind(10.0);
        wind.kind = 2;
        // 45° a frame at most 0.005 rad: half a turn takes over 5 s.
        wind.advance(1.0);
        assert!(wind.angle < 0.0 && wind.angle > -0.2, "{}", wind.angle);
        for _ in 0..20 {
            wind.advance(1.0);
        }
        assert_eq!(wind.angle, direction_angle(2));
        let (_, direction) = wind.world();
        assert!((direction - Vec2::new(-1.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn an_hour_rerolls_the_wind() {
        let mut wind = wind(10.0);
        wind.set_hour(5);
        assert_eq!(wind.multiplier, 1.0);
        wind.set_hour(6);
        assert!((0.2..1.0).contains(&wind.multiplier));
        assert!(wind.kind <= 6);
    }

    #[test]
    fn strength_changes_slowly() {
        let mut wind = wind(0.0);
        wind.power = 15.0;
        wind.advance(1.0);
        // 30 frames of 1/300.
        let e = wind.environment();
        assert!((e[0].x - 30.0 / 300.0 * 1.25).abs() < 1e-3, "{}", e[0].x);
    }
}
