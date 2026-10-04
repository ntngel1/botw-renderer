//! Wii U v208 kind-30 texture SRT conversion. The table is read from the
//! executable during baking; render only sees the resulting UV matrices.

use asset_format::model::TextureSrt;
use botw_formats::content::ContentRoots;

use crate::effects::eft_tables::{Rpl, executables};

#[derive(Clone, Debug)]
pub struct TextureSrtTable {
    /// Native layout: sin, cos, deltaSin, deltaCos (not the sead EFT layout).
    rows: Vec<[f32; 4]>,
}

impl TextureSrtTable {
    pub fn load(roots: &ContentRoots) -> Result<Self, String> {
        let mut errors = Vec::new();
        for path in executables(roots) {
            let result = std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| Self::from_rpl(&Rpl::parse(&bytes)?));
            match result {
                Ok(table) => return Ok(table),
                Err(error) => errors.push(format!("{}: {error}", path.display())),
            }
        }
        Err(format!(
            "texture SRT needs U-King.rpx v208: {}",
            errors.join("; ")
        ))
    }

    fn from_rpl(rpl: &Rpl) -> Result<Self, String> {
        // Built-in shader-param kind 30 -> mode dispatch -> three callbacks.
        for (address, expected) in [
            (0x103d_326c, 0x03c0_7eb4),
            (0x103d_3274, 0x03c0_81b0),
            (0x103d_3278, 0x03c0_82bc),
            (0x103d_327c, 0x03c0_83b8),
        ] {
            if rpl.u32(address)? != expected {
                return Err("kind-30 callbacks do not match v208".into());
            }
        }
        let rows = (0..256)
            .map(|i| {
                let at = 0x103d_2140 + i * 16;
                Ok([
                    rpl.f32(at)?,
                    rpl.f32(at + 4)?,
                    rpl.f32(at + 8)?,
                    rpl.f32(at + 12)?,
                ])
            })
            .collect::<Result<Vec<_>, String>>()?;
        if rows[0][0] != 0.0
            || rows[0][1] != 1.0
            || (rows[64][0] - 1.0).abs() > 1e-6
            || rows.iter().flatten().any(|v| !v.is_finite())
        {
            return Err("invalid texture SRT trig table".into());
        }
        Ok(Self { rows })
    }

    fn sin_cos(&self, rotation: f32) -> (f32, f32) {
        // The native fmadd is f64, followed by stfd/lwz: its low word is
        // the wrapped angle index. Rounding here differs from EFT's f32 cast.
        let index = f64::from(rotation)
            .mul_add(std::f64::consts::FRAC_1_PI, 3_145_728.0)
            .to_bits() as u32;
        let [sin, cos, dsin, dcos] = self.rows[(index >> 24) as usize];
        let fraction = (index & 0x00ff_ffff) as f32 * 5.960_464_5e-8;
        (dsin.mul_add(fraction, sin), dcos.mul_add(fraction, cos))
    }

    pub fn matrix(&self, srt: TextureSrt) -> Result<[f32; 6], String> {
        if !srt.rotation.is_finite()
            || srt
                .scale
                .iter()
                .chain(&srt.translation)
                .any(|v| !v.is_finite())
        {
            return Err("non-finite texture SRT".into());
        }
        srt.matrix_from_sin_cos(self.sin_cos(srt.rotation))
            .ok_or_else(|| format!("unsupported texture SRT mode {}", srt.mode))
    }
}
