//! Lightning as the game's weather manager runs it
//! (`WEATHER_CalcLightningStrikesAndFlash` `0x03666714`, Wii U v208;
//! docs/research/weather.md §5): in a thunderstorm a strike is announced
//! for ten seconds at a point 90 m ahead of the camera, then the bolt
//! falls; now and then a far bolt flashes in the distance. Every bolt
//! flashes the scene ([`Lightning::flash`], the manager's `+0x2f0`): the
//! sky evens out, the main light and the shadows fade.
//!
//! The effects and sounds are requests ([`LightningRequest`]) the effect
//! player turns into the game's emitter sets.

use bevy::prelude::*;

use crate::camera::MainView;
use crate::climate::{Weather, chase};
use crate::clouds::GlobalRandom;
use crate::heights::HeightSampler;

pub struct LightningPlugin {
    pub sampler: HeightSampler,
}

impl Plugin for LightningPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Lightning {
            sampler: Some(self.sampler.clone()),
            ..default()
        })
        .add_message::<LightningRequest>()
        .add_systems(
            Update,
            update
                .after(crate::climate::forecast)
                .before(crate::daynight::update_sky_state),
        );
    }
}

/// What the strike automaton asks for (the `ChemicalMgr` request bits).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum LightningRequest {
    /// Bit 0x10: the warning at the strike point, `progress` 0–1.
    Warning { at: Vec3, progress: f32 },
    /// Bit 0x08: the bolt.
    Strike { at: Vec3 },
    /// Bit 0x40: a far bolt about to fall.
    FarWarning { at: Vec3 },
    /// Bit 0x20: the far bolt.
    FarStrike { at: Vec3 },
    /// Bit 0x80: thunder rolls.
    Rumble,
}

// Weathers by their index in `asset_format::env::WEATHERS`.
const THUNDER_STORM: usize = 6;
const THUNDER_RAIN: usize = 7;

/// The constructor's parameters (`0x03664acc`).
const WARNING_SECONDS: f32 = 10.0; // +0x344
const CALM_SECONDS: f32 = 8.0; // +0x348
const FAR_CHANCE: u32 = 100; // +0x378
const RUMBLE_CHANCE: u32 = 75; // +0x37c
/// Frames a far bolt is announced for (`+0x354`).
const FAR_WARNING: f32 = 90.0;
/// How far ahead of the camera, and how much off, strikes fall (m).
const STRIKE_AHEAD: f32 = 90.0;
const STRIKE_SPREAD: f32 = 30.0;
/// The world's timer must have run this many frames (`WorldMgr+0x538`).
const WORLD_SETTLED: f32 = 30.0;

/// The strike automaton (`+0x310`) and the flash (`+0x34c`, `+0x2f0`).
#[derive(Resource, Default)]
pub struct Lightning {
    state: u8,
    /// `+0x314`, frames.
    timer: f32,
    /// `+0x354`: a far bolt is announced while it runs.
    far_timer: f32,
    strike_at: Option<Vec3>,
    far_at: Vec3,
    /// Frames since the scene started (`WorldMgr+0x538`).
    world_frames: f32,
    flash_state: u8,
    flash_timer: f32,
    /// `+0x2f0`, 0–1.
    pub flash: f32,
    sampler: Option<HeightSampler>,
}

impl Lightning {
    /// What the sky table's `cFade` becomes during a flash, from the night
    /// fade (`0x036461e4`).
    pub fn sky_fade(&self, night_fade: f32) -> f32 {
        match self.flash_state {
            0 => night_fade,
            2 => 1.0,
            _ => night_fade + (1.0 - night_fade) * self.flash,
        }
    }

    /// The flash is at its height (state 2).
    pub fn at_peak(&self) -> bool {
        self.flash_state == 2
    }

    fn trigger_flash(&mut self) {
        // SI-WTH-08: the EnvMgr id gating flashes is taken as the field's.
        self.flash = 0.0;
        self.flash_timer = 30.0;
        self.flash_state = 1;
    }

    /// The flash automaton, `frames` of 30 a second.
    fn step_flash(&mut self, frames: f32) {
        let chase = |value: f32, target: f32, r: f32, most: f32, least: f32| {
            chase(
                value,
                target,
                1.0 - r,
                frames,
                least * frames,
                most * frames,
            )
        };
        match self.flash_state {
            1 => {
                self.flash = chase(self.flash, 0.5, 0.5, 0.5, 0.1);
                self.flash_timer -= frames;
                if self.flash_timer <= 0.0 {
                    self.flash_state = 2;
                }
            }
            2 => {
                self.flash = chase(self.flash, 1.0, 0.1, 0.1, 0.01);
                if self.flash >= 1.0 {
                    self.flash_state = 3;
                }
            }
            3 => {
                self.flash = chase(self.flash, 0.0, 0.1, 0.12, 0.005);
                if self.flash <= 0.0 {
                    self.flash_state = 4;
                }
            }
            4 => self.flash_state = 0,
            _ => {}
        }
    }

    /// A point ahead of the camera on the ground, like `0x036665e8`.
    fn point_ahead(&self, eye: Vec3, forward: Vec3, random: &mut GlobalRandom) -> Vec3 {
        let ahead = Vec2::new(forward.x, forward.z).normalize_or(Vec2::NEG_Y) * STRIKE_AHEAD;
        let mut off = || (random.random.unit() * 2.0 - 1.0) * STRIKE_SPREAD;
        let (x, z) = (eye.x + ahead.x + off(), eye.z + ahead.y + off());
        let y = self
            .sampler
            .as_ref()
            .and_then(|s| s.height_at(x, z))
            .unwrap_or(eye.y);
        Vec3::new(x, y, z)
    }
}

#[allow(clippy::too_many_arguments)]
fn update(
    real: Res<Time>,
    weather: Res<Weather>,
    camera: Query<&GlobalTransform, With<MainView>>,
    mut random: ResMut<GlobalRandom>,
    mut lightning: ResMut<Lightning>,
    mut requests: MessageWriter<LightningRequest>,
) {
    let frames = real.delta_secs() * 30.0;
    let Ok(camera) = camera.single() else {
        return;
    };
    let (eye, forward) = (camera.translation(), camera.forward().as_vec3());
    let l = &mut *lightning;
    l.world_frames += frames;
    let enabled = matches!(weather.wanted, THUNDER_STORM | THUNDER_RAIN)
        && weather.transition >= 1.0
        && weather.cloudiness >= 0.99
        && l.world_frames > WORLD_SETTLED;

    // Far bolts: announced, then they fall and flash.
    if l.far_timer > 0.0 {
        requests.write(LightningRequest::FarWarning { at: l.far_at });
        l.far_timer -= frames;
        if l.far_timer <= 0.0 {
            requests.write(LightningRequest::FarStrike { at: l.far_at });
            l.trigger_flash();
        }
    }
    let start_far = |l: &mut Lightning, random: &mut GlobalRandom| {
        l.far_timer = FAR_WARNING;
        l.far_at = l.point_ahead(eye, forward, random);
    };
    l.timer -= frames;
    match l.state {
        0 => {
            if enabled {
                l.timer = 150.0;
                start_far(l, &mut random);
                l.state = 1;
            }
        }
        1 | 2 => {
            if l.far_timer <= 0.0 && random.random.below(FAR_CHANCE) == 0 {
                start_far(l, &mut random);
            }
            if l.timer <= 0.0 {
                if l.state == 1 {
                    l.timer = random.random.below(300) as f32;
                    l.state = 2;
                } else {
                    l.timer = WARNING_SECONDS * 30.0;
                    // SI-WTH-08: the EnvMgr id gating strikes is taken as
                    // the field's (strikes allowed).
                    l.state = 3;
                    l.strike_at = None;
                }
            }
            // SI-WTH-08: the automaton stops once the storm has passed.
            if !enabled {
                l.state = 0;
            }
        }
        3 => {
            let at = match l.strike_at {
                Some(at) => at,
                None => {
                    let at = l.point_ahead(eye, forward, &mut random);
                    l.strike_at = Some(at);
                    at
                }
            };
            let total = WARNING_SECONDS * 30.0;
            let progress = (1.0 - l.timer / total).clamp(0.0, 1.0);
            requests.write(LightningRequest::Warning { at, progress });
            if l.timer <= 0.0 {
                l.state = 4;
            }
        }
        4 => {
            if let Some(at) = l.strike_at {
                requests.write(LightningRequest::Strike { at });
            }
            l.trigger_flash();
            l.timer = 30.0;
            l.state = 5;
        }
        5 => {
            if l.timer <= 0.0 {
                l.timer = 90.0;
                l.state = if enabled { 6 } else { 0 };
            }
        }
        6 => {
            if l.timer <= 0.0 {
                l.timer = CALM_SECONDS * 30.0;
                l.state = 1;
            }
        }
        _ => l.state = 0,
    }
    if l.state != 0 && random.random.below(RUMBLE_CHANCE) == 0 {
        requests.write(LightningRequest::Rumble);
    }
    l.step_flash(frames);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flash_rises_peaks_and_dies_away() {
        let mut l = Lightning::default();
        l.trigger_flash();
        let mut peak = 0.0f32;
        let mut frames = 0;
        while l.flash_state != 0 && frames < 1000 {
            l.step_flash(1.0);
            peak = peak.max(l.flash);
            frames += 1;
        }
        assert_eq!(peak, 1.0);
        assert_eq!(l.flash, 0.0);
        assert!(frames > 30 && frames < 200, "{frames}");
    }

    #[test]
    fn the_sky_evens_out_during_a_flash() {
        let mut l = Lightning::default();
        assert_eq!(l.sky_fade(0.25), 0.25);
        l.trigger_flash();
        l.flash = 0.5;
        assert_eq!(l.sky_fade(0.0), 0.5);
        l.flash_state = 2;
        assert_eq!(l.sky_fade(0.0), 1.0);
    }
}
