//! Rain, snow and thunder as the game's weather manager keeps them
//! (`WEATHER_Calc` `0x03667d20`, Wii U v208; docs/research/weather.md
//! §2.2, §6.1): the concentrations the effect links read as global
//! properties (`濃度:雨` rain or snow, 0–2; `濃度:雷` thunder, 0–1) and the
//! wetness of the ground the deferred shading reads (`rain_ratio`,
//! `rainfall`).
//!
//! The rain concentration heads for 1 (rain, blue-sky rain, snow) or 2
//! (heavy rain, thunder rain, heavy snow) while a four-frame hold timer is
//! re-armed, which it is every frame while the weather taken is rainy and
//! has blown in to 0.65. Once the rain is dense (above 0.8) the ground
//! gets wet towards the weather's targets in about five seconds; it dries
//! very slowly afterwards (snow at once).

use bevy::prelude::*;

use crate::climate::{StageTimer, Weather, chase};

pub struct PrecipitationPlugin;

impl Plugin for PrecipitationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Precipitation>()
            .add_systems(
                Update,
                update
                    .after(crate::climate::forecast)
                    .before(crate::daynight::update_sky_state),
            )
            .add_systems(
                PostUpdate,
                wet_the_look.before(crate::look::LookSystems::Upload),
            );
    }
}

// The weathers by their index in `asset_format::env::WEATHERS`.
const BLUESKY: usize = 0;
const RAIN: usize = 2;
const HEAVY_RAIN: usize = 3;
const SNOW: usize = 4;
const HEAVY_SNOW: usize = 5;
const THUNDER_RAIN: usize = 7;
const BLUESKY_RAIN: usize = 8;

/// Frames the hold timers are armed for (`+0x31c`, `+0x320`).
const HOLD: f32 = 4.0;
/// The weather must have blown in this far to hold the rain (`+0x14`).
const RAIN_TAKEN: f32 = 0.65;
/// … and this far to hold the thunder.
const THUNDER_TAKEN: f32 = 0.3;
/// The rain concentration above which the ground gets wet.
const DENSE: f32 = 0.8;

/// `WeatherMgr`'s precipitation values.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Precipitation {
    /// `濃度:雨` (`+0x2cc`): 0 dry, 1 rain or snow, 2 heavy.
    pub rain: f32,
    /// `濃度:雷` (`+0x2d0`), 0–1.
    pub thunder: f32,
    /// `uking_dynamic_rain_ratio` before its suppression factor (`+0x2c0`).
    pub rain_ratio: f32,
    /// `uking_dynamic_rainfall` (`+0x2c4`).
    pub rainfall: f32,
    /// `+0x2c8` (its reader is not known).
    pub wet_extra: f32,
    /// Hold timers (frames): rain `+0x31c`, thunder `+0x320`.
    rain_hold: f32,
    thunder_hold: f32,
}

impl Precipitation {
    /// One frame of the weather calc's precipitation part: `taken` is the
    /// weather taken (`+0x18`), `transition` how far it has blown in
    /// (`+0x14`), `wanted` the climate's weather (`0x03672890`). `snap`:
    /// a set weather or a new stage, every value jumps to its target.
    pub fn step(&mut self, taken: usize, transition: f32, wanted: usize, frames: f32, snap: bool) {
        let rainy = matches!(taken, RAIN | BLUESKY_RAIN | SNOW);
        let heavy = matches!(taken, HEAVY_RAIN | THUNDER_RAIN | HEAVY_SNOW);
        // The timers run down, then are re-armed for this frame.
        let held = self.rain_hold > 0.0;
        self.rain_hold = (self.rain_hold - frames).max(0.0);
        self.thunder_hold = (self.thunder_hold - frames).max(0.0);
        if (rainy || heavy) && (transition >= RAIN_TAKEN || held) {
            self.rain_hold = HOLD;
        }
        if taken == THUNDER_RAIN && transition >= THUNDER_TAKEN {
            self.thunder_hold = HOLD;
        }

        // `0x036698e8`: the step 0.002 doubles on the heavy and the dry
        // branches, and stays doubled for the thunder after.
        let (target, most) = if self.rain_hold > 0.0 && rainy {
            (1.0, 0.002)
        } else if self.rain_hold > 0.0 && heavy {
            (2.0, 0.004)
        } else {
            (0.0, 0.004)
        };
        let step = |value: f32, target: f32, most: f32, least: f32| {
            if snap {
                target
            } else {
                chase(value, target, 0.9, frames, least * frames, most * frames)
            }
        };
        self.rain = step(self.rain, target, most, 0.001);
        let thunder = if self.thunder_hold > 0.0 { 1.0 } else { 0.0 };
        self.thunder = step(self.thunder, thunder, most, 0.001);

        // Wetness (`+0x2c0`, `+0x2c4`, `+0x2c8`).
        let wet_targets = match wanted {
            RAIN | BLUESKY_RAIN => Some((0.725, 1.0, 0.8)),
            HEAVY_RAIN | THUNDER_RAIN => Some((0.8, 1.2, 1.0)),
            _ => None,
        };
        match wet_targets {
            Some((ratio, rainfall, extra)) if (rainy || heavy) && self.rain_hold > 0.0 => {
                if self.rain > DENSE || snap {
                    self.rain_ratio = step(self.rain_ratio, ratio, 0.005, 0.005);
                    self.rainfall = step(self.rainfall, rainfall, 0.005, 0.005);
                    self.wet_extra = step(self.wet_extra, extra, 0.00125, 0.00125);
                }
            }
            _ => {
                let (most, least) = if matches!(taken, SNOW | HEAVY_SNOW) {
                    (0.025, 0.025)
                } else {
                    (0.0005, 0.00005)
                };
                self.rain_ratio = step(self.rain_ratio, 0.0, most, least);
                self.rainfall = step(self.rainfall, 0.0, most, least);
                let (most, least) = if taken == BLUESKY {
                    (0.001, 0.001)
                } else {
                    (0.0005, 0.00005)
                };
                self.wet_extra = step(self.wet_extra, 0.0, most, least);
            }
        }
    }
}

fn update(
    real: Res<Time>,
    weather: Res<Weather>,
    stage: Res<StageTimer>,
    mut precipitation: ResMut<Precipitation>,
) {
    let mut next = precipitation.clone();
    let snap = weather.forced().is_some() || stage.running();
    next.step(
        weather.current,
        weather.transition,
        weather.wanted,
        real.delta_secs() * 30.0,
        snap,
    );
    if next != *precipitation {
        *precipitation = next;
    }
}

/// Period of the shader clock `gsys_context[20].y` (frames, `0x039a8590`).
const CLOCK_PERIOD: f32 = 120.0;

/// The wet ground's values for the field shaders (`look.wgsl`,
/// `field_wetness`): `KSYS_SetRainUniformsAndVariation` (`0x034087bc`)
/// hands `rain_ratio` and `rainfall` to the deferred shading, which reads
/// them with a clock of frames wrapped at 120.
fn wet_the_look(
    real: Res<Time>,
    precipitation: Res<Precipitation>,
    mut clock: Local<f32>,
    mut look: ResMut<crate::look::Look>,
) {
    *clock = (*clock + real.delta_secs() * 30.0).rem_euclid(CLOCK_PERIOD);
    // SI-WTH-11: the rain ratio's suppression factor `(KSys+0x1f4)+0x1f8`
    // is taken as 0.
    let weather = Vec4::new(
        precipitation.rain_ratio,
        precipitation.rainfall,
        (*clock * 0.125).fract(),
        0.0,
    );
    if look.weather[0] != weather {
        look.weather[0] = weather;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rain_blows_in_over_about_seventeen_seconds() {
        let mut p = Precipitation::default();
        let mut frames = 0;
        while p.rain < 1.0 {
            p.step(RAIN, 1.0, RAIN, 1.0, false);
            frames += 1;
        }
        assert!((480..=520).contains(&frames), "{frames}");
        // Dense rain wets the ground in about five seconds.
        assert!(p.rain_ratio > 0.0);
        for _ in 0..200 {
            p.step(RAIN, 1.0, RAIN, 1.0, false);
        }
        assert_eq!((p.rain_ratio, p.rainfall), (0.725, 1.0));
    }

    #[test]
    fn rain_stops_in_about_eight_seconds_and_the_ground_dries_slowly() {
        let mut p = Precipitation::default();
        p.step(RAIN, 1.0, RAIN, 1.0, true);
        assert_eq!(p.rain, 1.0);
        let mut frames = 0;
        while p.rain > 0.0 {
            p.step(BLUESKY, 1.0, BLUESKY, 1.0, false);
            frames += 1;
        }
        assert!((240..=270).contains(&frames), "{frames}");
        assert!(p.rain_ratio > 0.5, "{}", p.rain_ratio);
    }

    #[test]
    fn heavy_rain_reaches_two() {
        let mut p = Precipitation::default();
        p.step(HEAVY_RAIN, 1.0, HEAVY_RAIN, 1.0, true);
        assert_eq!((p.rain, p.rain_ratio, p.rainfall), (2.0, 0.8, 1.2));
        assert_eq!(p.thunder, 0.0);
        p.step(THUNDER_RAIN, 1.0, THUNDER_RAIN, 1.0, true);
        assert_eq!(p.thunder, 1.0);
    }
}
