//! The game's sky table: the colour of the sky in every direction, baked
//! from the atmosphere's precomputed inscatter (`sky.skybin`, read by
//! `asset_format::sky`) like the game bakes it every frame
//! (`sky_bake_inscatter`, PS 413 of `uking_pass_shader`, called from
//! `SKY_BakeInscatterLut` `0x033f5a98`, Wii U v208; formula and uniform
//! sources in docs/research/wiiu-sky-resources.md). The scattering fog takes
//! its colour from it (`apply_haze` in `look.wgsl`, the game's PS 140) and
//! the sky dome draws it (`sky_haze.wgsl`, the game's `sky_postfx_sky`).
//!
//! The bake is the game's formula with the palette's parameters; where they
//! come from is the game's too (`ENV_UpdateWeatherPalettes`): the sun's
//! colour in the sky (`SkySunColor`, rgb × a) tinted by the climate and the
//! weather (`FeatureColor`), the Rayleigh and Mie amplifiers and the Mie
//! asymmetry times the climate's and weather's factors, the camera's height
//! and the sun's, and the night fade by the time of day. Not reproduced:
//! the world manager's pull of the Mie amplifier towards 1 (`+0x2178`, not
//! understood). How bright the table is in the viewer's units is a fit
//! (`fog.rs`). It runs on the CPU in the background (the game bakes every
//! frame): a new bake starts when the bake's parameters have changed, at
//! most every [`BAKE_INTERVAL`] seconds; each bake is the game's table
//! for its parameters.
//!
//! Without the game's table (no dump, or it cannot be read) nothing is
//! baked: the haze and the sky keep their fitted colours, and a warning
//! says so.

use std::path::PathBuf;
use std::sync::Arc;

use asset_format::sky::{Inscatter, RES_MU, RES_MU_S, RES_NU, RES_R};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::climate::{Climate, Weather};
use crate::daynight::{Environment, Sky, TimeOfDay};
use crate::look::{LOOK_SKY, LookSkyLut};

/// Radius of the ground and of the top of the atmosphere, km
/// (`SKY_CalcAltitudeTextureParams` `0x033f099c` and the shaders).
const RG: f32 = 6360.0;
const RT: f32 = 6420.0;
/// Wavelengths the Rayleigh coefficients are computed for, µm (`0x102bfb08`).
const WAVELENGTHS: [f32; 3] = [0.7, 0.546, 0.436];

/// Whether bad weather keeps the brightness of the climate's clear palettes
/// (the table's colours still the game's for the current sky): the game's
/// overcast table is 5–15 times darker than the clear one, yet its rainy
/// distance is a light turquoise (R/712, R/717); what brightens it in the
/// game is not traced. Off: the game's table as it comes. Disputed:
/// docs/CHOICES.md, SKY-LUT-001.
// SI-SKY-10: refraction +1.0 assumed; night mu_s and brightness probe are ours.
const HOLD_CLEAR_BRIGHTNESS: bool = false;

/// Least real time between the starts of two bakes (s): the parameters
/// (the time of day, the weather's blend, the camera's height) drift
/// slowly, and a bake takes a worker thread for a while.
const BAKE_INTERVAL: f32 = 0.25;

/// Where the brightness of the table is compared (HOLD_CLEAR_BRIGHTNESS):
/// a little above the horizon, away from the sun (μ, u).
const BRIGHTNESS_PROBE: (f32, f32) = (0.05, 0.25);

pub struct SkyLutPlugin {
    /// The `assets/` folder (`sky/inscatter.bin`).
    pub assets: PathBuf,
}

impl Plugin for SkyLutPlugin {
    fn build(&self, app: &mut App) {
        let path = self.assets.join(asset_format::paths::INSCATTER);
        let loading = path.exists().then(|| {
            AsyncComputeTaskPool::get().spawn(async move {
                match Inscatter::read(&path) {
                    Ok(table) => Some(Arc::new(table)),
                    Err(error) => {
                        warn!("sky table unreadable ({error}): the haze keeps its fitted colour");
                        None
                    }
                }
            })
        });
        if loading.is_none() {
            info!(
                "no baked {}: the haze keeps its fitted colour instead of the game's sky table",
                asset_format::paths::INSCATTER
            );
        }
        app.insert_resource(SkyLutState {
            loading,
            table: None,
            baking: None,
            baked: None,
        })
        .add_systems(
            Update,
            bake_sky_lut.after(crate::daynight::update_sky_state),
        );
    }
}

#[derive(Resource)]
pub(crate) struct SkyLutState {
    loading: Option<Task<Option<Arc<Inscatter>>>>,
    table: Option<Arc<Inscatter>>,
    baking: Option<Task<Vec<[f32; 4]>>>,
    /// The parameters of the last bake started, and when (real seconds).
    baked: Option<((BakeParams, Option<BakeParams>), f32)>,
}

impl SkyLutState {
    pub fn is_loading(&self) -> bool {
        self.loading.is_some() || self.baking.is_some()
    }
}

/// Takes a finished bake into the look texture and starts the next one
/// when the parameters have changed.
#[allow(clippy::too_many_arguments)]
fn bake_sky_lut(
    mut state: ResMut<SkyLutState>,
    real: Res<Time<Real>>,
    sky: Res<Sky>,
    time: Res<TimeOfDay>,
    environment: Res<Environment>,
    climate: Option<Res<Climate>>,
    cameras: Query<&GlobalTransform, crate::camera::MainCamera>,
    mut lut: ResMut<LookSkyLut>,
) {
    let state = &mut *state;
    if let Some(task) = &mut state.loading
        && let Some(table) = block_on(poll_once(task))
    {
        state.loading = None;
        state.table = table;
    }
    let Some(table) = state.table.clone() else {
        return;
    };
    if let Some(task) = &mut state.baking {
        let Some(texels) = block_on(poll_once(task)) else {
            return;
        };
        state.baking = None;
        lut.0 = Some(Arc::new(texels));
    }
    let camera_y = cameras.iter().next().map_or(0.0, |c| c.translation().y);
    let params = BakeParams::for_sky(&sky, time.hours, camera_y);
    let clear = HOLD_CLEAR_BRIGHTNESS.then(|| {
        let climate = climate.as_deref().cloned().unwrap_or_default();
        let clear_sky = environment.sky(&time, &climate, &Weather::default());
        BakeParams::for_sky(&clear_sky, time.hours, camera_y)
    });
    let now = real.elapsed_secs();
    let due = match state.baked {
        None => true,
        Some((baked, at)) => baked != (params, clear) && (now - at >= BAKE_INTERVAL),
    };
    if !due {
        return;
    }
    state.baked = Some(((params, clear), now));
    state.baking = Some(AsyncComputeTaskPool::get().spawn(async move {
        let mut texels = bake(&table, &params);
        if let Some(clear) = clear {
            let gain = clear_gain(&table, &params, &clear);
            for t in &mut texels {
                for c in &mut t[..3] {
                    *c *= gain;
                }
            }
        }
        texels
    }));
}

/// The uniforms of the bake (`SKY_BakeInscatterLut`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakeParams {
    /// `cAltitude`: the camera's height over 60 km.
    pub altitude: f32,
    /// `cSunZenithAngle`: the height (sine of the elevation) of the light
    /// the sky is lit by: the sky object's light direction (`+0x72c`),
    /// written by `0x03656be0` (`daynight::game_main_light`): the sun by
    /// day, the night light's arc at night (the night palettes'
    /// `SkySunColor` is a moonlit cyan). docs/CHOICES.md, SKY-LUT-001.
    pub mu_s: f32,
    /// `cAmplifierReyleigh`, `cAmplifierMie`, `cSymmetricalPropertyMie`.
    pub rayleigh: f32,
    pub mie: f32,
    pub g: f32,
    /// `cSunColor`: rgb × brightness.
    pub sun: Vec3,
    /// `cFade`: how far the sky is evened out towards its colour at right
    /// angles to the sun ([`night_fade`]).
    pub fade: f32,
}

impl BakeParams {
    /// The game's sources for the current sky and a camera `camera_y`
    /// metres up (`ENV_UpdateWeatherPalettes`, `SKY_SetDynamicScatteringParams`
    /// clamps: amplifiers at least 0, g within 0–1).
    pub fn for_sky(sky: &Sky, hours: f32, camera_y: f32) -> Self {
        let palette = &sky.palette;
        let influence = &sky.scalars;
        Self {
            altitude: camera_y / 60_000.0,
            // SI-SKY-10: refraction +1.0 assumed; night mu_s and brightness probe are ours.
            mu_s: sky.towards_light.y,
            rayleigh: (palette.rayleigh_amplifier * influence.rayleigh).max(0.0),
            mie: (palette.mie_amplifier * influence.mie).max(0.0),
            g: (palette.mie_asymmetry * influence.mie_symmetrical).clamp(0.0, 1.0),
            // `SkySunColor` in `F` like the main light (the sky's
            // `dynamic_color`, @`0x036460f4`): only on the field's row.
            sun: Vec3::from(palette.sky_sun_color) * palette.sky_sun_intensity * sky.light_feature,
            // The lightning's flash evens the sky out (`0x036461e4`).
            fade: if sky.flash_peak {
                1.0
            } else {
                let night = night_fade(hours);
                night + (1.0 - night) * sky.flash
            },
        }
    }
}

/// `SKY_CalcNightFadeByTime` (`0x03642468`): 1 around 22:00 and 03:00,
/// rising and falling over the hours around them, else 0. The game counts
/// the time in degrees (15 per hour).
pub fn night_fade(hours: f32) -> f32 {
    let d = hours.rem_euclid(24.0) * 15.0;
    let f = if d > 315.0 && d < 330.0 {
        1.0 - (330.0 - d) / 15.0
    } else if (330.0..345.0).contains(&d) {
        (345.0 - d) / 15.0
    } else if d > 30.0 && d <= 45.0 {
        1.0 - (45.0 - d) / 15.0
    } else if d > 45.0 && d < 52.5 {
        (52.5 - d) / 7.5
    } else {
        0.0
    };
    (2.0 * f).min(1.0)
}

// SI-SKY-10: refraction +1.0 assumed; night mu_s and brightness probe are ours.
/// Rayleigh scattering coefficients per km (`SKY_CalcRayleighCoefficients`
/// `0x033f12f8`): `8π³(n² − 1)² / (76.5 λ⁴) · 1000`, with the refractive
/// index from its dispersion formula; the constant 1 added to it lives in
/// uninitialised data and is assumed.
pub fn rayleigh_coefficients() -> Vec3 {
    let beta = |lambda: f32| {
        let x = lambda.powi(-2);
        let n = 0.057_921_05 / (238.0185 - x) + 0.001_679_17 / (57.362 - x) + 1.0;
        std::f32::consts::PI.powi(3) * 8.0 * (n * n - 1.0).powi(2) / (lambda.powi(4) * 76.5)
            * 1000.0
    };
    Vec3::new(
        beta(WAVELENGTHS[0]),
        beta(WAVELENGTHS[1]),
        beta(WAVELENGTHS[2]),
    )
}

/// The table [`LOOK_SKY`]² texels, row by row from the bottom: u (x) the
/// view's angle to the sun, `ν = 2 sin(u π/2) − 1`; v (y) the view's height,
/// `μ = 2v − 1`. Alpha is 0 below the horizon. PS 413 with its vertex
/// shader: `BAKED_SUNVIEW_NON_LINEAR`, `ADHOC_PROC`, below the horizon
/// sampled (`cEffectiveBelowHorizon` 1).
pub fn bake(table: &Inscatter, p: &BakeParams) -> Vec<[f32; 4]> {
    let size = LOOK_SKY as usize;
    let beta = rayleigh_coefficients();
    let r = RG + 60.0 * p.altitude.clamp(1e-4, 0.9999);
    let mu_horizon = -(1.0 - (RG / r).powi(2)).clamp(0.0, 1.0).sqrt();
    let lookup = Lookup::new(table, r, p.mu_s);
    let mut out = Vec::with_capacity(size * size);
    for j in 0..size {
        let mu_view = 2.0 * (j as f32 + 0.5) / size as f32 - 1.0;
        let alpha = if mu_view < mu_horizon { 0.0 } else { 1.0 };
        for i in 0..size {
            let u = (i as f32 + 0.5) / size as f32;
            // Kept finite for the half-float texture.
            let rgb = texel(&lookup, p, beta, mu_view, u).min(Vec3::splat(60_000.0));
            out.push([rgb.x, rgb.y, rgb.z, alpha]);
        }
    }
    out
}

/// How much darker the table for `params` is than for the clear sky's
/// `clear` at [`BRIGHTNESS_PROBE`] (at least 1: never darkened).
pub fn clear_gain(table: &Inscatter, params: &BakeParams, clear: &BakeParams) -> f32 {
    let luminance = |p: &BakeParams| {
        let r = RG + 60.0 * p.altitude.clamp(1e-4, 0.9999);
        let lookup = Lookup::new(table, r, p.mu_s);
        let (mu, u) = BRIGHTNESS_PROBE;
        texel(&lookup, p, rayleigh_coefficients(), mu, u).dot(Vec3::new(0.2126, 0.7152, 0.0722))
    };
    (luminance(clear) / luminance(params).max(1e-6)).max(1.0)
}

/// One texel of the bake at view height `mu` and `u` (see [`bake`]).
fn texel(lookup: &Lookup, p: &BakeParams, beta: Vec3, mu: f32, u: f32) -> Vec3 {
    let nu = 2.0 * (u * std::f32::consts::FRAC_PI_2).sin() - 1.0;
    let g = p.g
        * (2.0 * u - 0.25)
            .clamp(0.0, 1.0)
            .max((mu + 0.75).clamp(0.0, 1.0));
    let s = lookup.at(mu, (nu + 1.0) / 2.0 * (RES_NU as f32 - 1.0));
    let mut rgb = radiance(s, nu, g, p, beta);
    if p.fade > 0.0 {
        // The even sky the fade blends to: at right angles to the sun, with
        // the plain asymmetry.
        let even = radiance(
            lookup.at(mu, (RES_NU as f32 - 1.0) / 2.0),
            0.0,
            p.g,
            p,
            beta,
        );
        rgb = rgb.lerp(even, p.fade);
    }
    rgb * p.sun
}

/// Rayleigh and Mie light with the shader's phase functions; `s` is the
/// packed inscatter (rgb Rayleigh, alpha Mie red).
fn radiance(s: Vec4, nu: f32, g: f32, p: &BakeParams, beta: Vec3) -> Vec3 {
    let mie = s.truncate() * (s.w / s.x.max(1e-4)) * (beta.x / beta);
    let phase_r = 3.0 / (16.0 * std::f32::consts::PI) * (1.0 + nu * nu) * p.rayleigh;
    let phase_m = 3.0 / (8.0 * std::f32::consts::PI) * (1.0 - g * g) * (1.0 + nu * nu)
        / ((2.0 + g * g) * (1.0 + g * g - 2.0 * g * nu).max(0.0).powf(1.5))
        * p.mie;
    (s.truncate() * phase_r + mie * phase_m).max(Vec3::ZERO)
}

/// Bruneton's `texture4D` at a fixed radius and sun height, as PS 413 has
/// it: trilinear, clamped reads of the table, ν between two slices.
struct Lookup<'a> {
    table: &'a Inscatter,
    r: f32,
    u_r: f32,
    u_mu_s: f32,
}

impl<'a> Lookup<'a> {
    fn new(table: &'a Inscatter, r: f32, mu_s: f32) -> Self {
        let h = (RT * RT - RG * RG).sqrt();
        let rho = (r * r - RG * RG).max(0.0).sqrt();
        let (res_r, res_mu_s) = (RES_R as f32, RES_MU_S as f32);
        Self {
            table,
            r,
            u_r: 0.5 / res_r + rho / h * (1.0 - 1.0 / res_r),
            u_mu_s: 0.5 / res_mu_s
                + ((1.0 - (-3.0 * mu_s - 0.6).exp()) * 1.0281).max(0.0) * (1.0 - 1.0 / res_mu_s),
        }
    }

    /// At view height `mu` and ν as a fractional slice.
    fn at(&self, mu: f32, nu_slice: f32) -> Vec4 {
        let (r, res_mu) = (self.r, RES_MU as f32);
        let h = (RT * RT - RG * RG).sqrt();
        let rho = (r * r - RG * RG).max(0.0).sqrt();
        let rmu = r * mu;
        let delta = rmu * rmu - r * r + RG * RG;
        let (c0, c1, c2, c3) = if rmu < 0.0 && delta > 0.0 {
            (1.0, 0.0, 0.0, 0.5 - 0.5 / res_mu)
        } else {
            (-1.0, h * h, h, 0.5 + 0.5 / res_mu)
        };
        let u_mu =
            c3 + (rmu * c0 + (delta + c1).max(0.0).sqrt()) / (rho + c2) * (0.5 - 1.0 / res_mu);
        let slice = nu_slice.floor();
        let frac = nu_slice - slice;
        let a = self.sample((slice + self.u_mu_s) / RES_NU as f32, u_mu);
        let b = self.sample((slice + self.u_mu_s + 1.0) / RES_NU as f32, u_mu);
        a.lerp(b, frac).max(Vec4::ZERO)
    }

    /// Trilinear, clamp to edge, normalised coordinates (z at `u_r`).
    fn sample(&self, x: f32, y: f32) -> Vec4 {
        let (w, h, d) = (
            Inscatter::WIDTH as i32,
            Inscatter::HEIGHT as i32,
            Inscatter::DEPTH as i32,
        );
        let fx = x * w as f32 - 0.5;
        let fy = y * h as f32 - 0.5;
        let fz = self.u_r * d as f32 - 0.5;
        let (x0, y0, z0) = (fx.floor(), fy.floor(), fz.floor());
        let (tx, ty, tz) = (fx - x0, fy - y0, fz - z0);
        let mut out = Vec4::ZERO;
        for (dz, wz) in [(0, 1.0 - tz), (1, tz)] {
            let zi = (z0 as i32 + dz).clamp(0, d - 1) as u32;
            for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
                let yi = (y0 as i32 + dy).clamp(0, h - 1) as u32;
                for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
                    let weight = wx * wy * wz;
                    if weight == 0.0 {
                        continue;
                    }
                    let xi = (x0 as i32 + dx).clamp(0, w - 1) as u32;
                    out += Vec4::from(self.table.texel(xi, yi, zi)) * weight;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_table(value: [f32; 4]) -> Inscatter {
        let n = (Inscatter::WIDTH * Inscatter::HEIGHT * Inscatter::DEPTH) as usize;
        Inscatter {
            texels: vec![value; n],
        }
    }

    fn params() -> BakeParams {
        BakeParams {
            altitude: 0.00223,
            mu_s: 0.9,
            rayleigh: 1.0,
            mie: 12.0,
            g: 0.75,
            sun: Vec3::new(1.0, 0.921_569, 0.85) * 18.0,
            fade: 0.0,
        }
    }

    #[test]
    fn night_fade_peaks_at_ten_and_three() {
        assert_eq!(night_fade(12.0), 0.0);
        assert_eq!(night_fade(22.0), 1.0);
        assert_eq!(night_fade(3.0), 1.0);
        assert_eq!(night_fade(21.0), 0.0);
        assert!((night_fade(21.25) - 0.5).abs() < 1e-5);
        assert_eq!(night_fade(23.0), 0.0);
        assert_eq!(night_fade(3.5), 0.0);
        assert_eq!(night_fade(-2.0), 1.0, "22:00 of the day before");
    }

    #[test]
    fn rayleigh_scatters_blue_most() {
        let beta = rayleigh_coefficients();
        assert!(beta.z > beta.y && beta.y > beta.x && beta.x > 0.0);
        // λ⁻⁴ dominates: roughly (0.7 / 0.436)⁴ ≈ 6.6 between red and blue.
        assert!((beta.z / beta.x - 6.6).abs() < 0.5, "{beta}");
    }

    #[test]
    fn mie_glows_towards_the_sun_and_nothing_shows_below_the_horizon() {
        let lut = bake(&flat_table([0.1, 0.2, 0.4, 0.05]), &params());
        let size = LOOK_SKY as usize;
        let row = size * 3 / 4;
        let at = |i: usize, j: usize| lut[j * size + i];
        let towards = at(size - 1, row);
        let away = at(0, row);
        assert!(towards[0] > 5.0 * away[0], "{towards:?} vs {away:?}");
        assert_eq!(at(size / 2, 0)[3], 0.0);
        assert_eq!(at(size / 2, size - 1)[3], 1.0);
    }

    /// The bake against `tools/research/sky_bake_probe.py` on the dump's
    /// table (its clear noon: zenith and the horizon away from the sun, in
    /// docs/research/wiiu-sky-resources.md). Needs the baked table:
    /// `cargo test -p render sky_lut -- --ignored`.
    #[test]
    #[ignore = "needs the baked assets/sky/inscatter.bin"]
    fn bake_matches_the_probe_on_the_dump() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets")
            .join(asset_format::paths::INSCATTER);
        let table = Inscatter::read(&path).unwrap();
        let lut = bake(&table, &params());
        let size = LOOK_SKY as usize;
        let zenith = Vec4::from(lut[(size - 1) * size + 128]).truncate();
        let horizon = Vec4::from(lut[(size / 2 + 2) * size + 32]).truncate();
        for (ours, probe) in [
            (zenith, Vec3::new(0.106_219, 0.263_068, 0.612_267)),
            (horizon, Vec3::new(1.379_107, 1.714_051, 1.667_719)),
        ] {
            assert!(
                ((ours - probe) / probe).abs().max_element() < 0.01,
                "{ours} vs probe {probe}"
            );
        }
    }

    #[test]
    fn clear_gain_brightens_only_a_darker_sky() {
        let table = flat_table([0.1, 0.2, 0.4, 0.05]);
        let clear = params();
        let dim = BakeParams {
            sun: clear.sun * 0.1,
            ..clear
        };
        assert!((clear_gain(&table, &dim, &clear) - 10.0).abs() < 1e-3);
        assert_eq!(clear_gain(&table, &clear, &dim), 1.0);
    }

    #[test]
    fn full_fade_evens_the_sky_round_the_sun() {
        let lut = bake(
            &flat_table([0.1, 0.2, 0.4, 0.05]),
            &BakeParams {
                fade: 1.0,
                ..params()
            },
        );
        let size = LOOK_SKY as usize;
        let row = &lut[size * 3 / 4 * size..][..size];
        assert!(row.iter().all(|t| (t[0] - row[0][0]).abs() < 1e-6));
    }
}
