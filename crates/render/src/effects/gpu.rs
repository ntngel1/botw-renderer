//! What the simulation hands the particle shaders: per particle the vertex
//! attributes the effect library writes at emission (instanced buffers 8
//! and 9, `eft_Shader_BindAttributes` `0x03b639f4`), per emitter its
//! dynamic uniform block (`eft_Emitter_UpdateDynamicUbo` `0x03b6b724`).
//! docs/research/eft-runtime.md §4.3, §7.

use bevy::math::Vec4;
use bevy::render::render_resource::ShaderType;
use bytemuck::{Pod, Zeroable};

/// One particle's attributes, in the library's order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable, ShaderType)]
pub struct ParticleAttr {
    /// `sysLocalPosAttr`: position (emitter space, or as the follow type
    /// says), life (frames).
    pub local_pos: Vec4,
    /// `sysLocalVecAttr`: velocity (per frame), birth frame (emitter frame).
    pub local_vec: Vec4,
    /// `sysLocalDiffAttr`: the last position step (CPU calc).
    pub local_diff: Vec4,
    /// `sysScaleAttr`: scale xyz, speed multiplier.
    pub scale: Vec4,
    /// `sysRandomAttr`: four randoms in [0, 1).
    pub random: Vec4,
    /// `sysInitRotateAttr`: the resource's initial rotation (0x700..0x708).
    pub init_rotate: Vec4,
    /// `sysColor0Attr`, `sysColor1Attr`: 1, or inherited from a parent.
    pub color0: Vec4,
    pub color1: Vec4,
    /// `sysEmtMat0..2`: the emitter's world SRT rows at birth.
    pub emt_mat: [Vec4; 3],
    /// `sysEmtRTMat0..2`: the emitter's world RT rows at birth.
    pub emt_rt_mat: [Vec4; 3],
}

/// `sysEmitterDynamicUniformBlock` (0xC0 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable, ShaderType)]
pub struct EmitterDynamic {
    /// rgb, alpha (0x00).
    pub color0: Vec4,
    /// (0x10).
    pub color1: Vec4,
    /// Emitter frame, 1, 1, frame step (0x20).
    pub frame: Vec4,
    /// Set alpha × alpha fade, particle scale xyz × scale fade (0x30).
    pub alpha_scale: Vec4,
    /// Emitter world SRT, three rows and (0, 0, 0, 1) (0x40).
    pub srt: [Vec4; 4],
    /// Emitter world RT (0x80).
    pub rt: [Vec4; 4],
}
