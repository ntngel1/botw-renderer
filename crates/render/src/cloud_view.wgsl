// The view the clouds' reduced buffer is drawn for (`botw::clouds::
// reduced`): the camera and the time as Bevy's mesh view bindings have
// them, and the frame's depth, which the pass reads instead of testing
// against it. The cloud shaders import `view` and `globals` from here
// when REDUCED_BUFFER is set, from `bevy_pbr::mesh_view_bindings`
// otherwise.

#define_import_path botw::cloud_view

#import bevy_render::{view::View, globals::Globals}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> globals: Globals;
#ifdef MULTISAMPLED
@group(0) @binding(2) var scene_depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(2) var scene_depth: texture_depth_2d;
#endif
