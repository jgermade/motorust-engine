//! Bevy draws the sun's shadows out to the end of its last cascade and no
//! further: a shadow comes in all at once as what casts it crosses that
//! distance, a pop-in plain to see on an open road. Its shadow shader is
//! patched as it loads, so that shadows fade out over the last fifth of the
//! last cascade instead and come in softly. If the code it is patched in is
//! not found (another version of Bevy), the shader is left as it is, and a
//! warning says so.

use bevy::prelude::*;
use bevy::shader::{Shader, ShaderImport, Source};

/// The shader patched, by its import path.
const MODULE: &str = "bevy_pbr::shadows";
/// Where the fade goes: at the end of `fetch_directional_shadow`, after the
/// blend between cascades, before it returns.
const ANCHOR: &str = "    return shadow;\n}\n\nfn cascade_debug_visualization";
/// The fade, marked so it is put in once.
const FADE: &str = "    // motorust: fade out over the last fifth of the last cascade, so a
    // shadow comes in softly rather than all at once.
    let last_far_bound = (*light).cascades[(*light).num_cascades - 1u].far_bound;
    shadow = mix(shadow, 1.0, smoothstep(0.8 * last_far_bound, last_far_bound, -view_z));
";

/// `source` (Bevy's shadow shader) with the fade, if it has the place for
/// it and has not got it yet.
pub fn fade_far_shadows(source: &str) -> Option<String> {
    (source.contains(ANCHOR) && !source.contains("motorust:"))
        .then(|| source.replacen(ANCHOR, &format!("{FADE}{ANCHOR}"), 1))
}

pub struct ShadowFadePlugin;

impl Plugin for ShadowFadePlugin {
    fn build(&self, app: &mut App) {
        // Only where there are shaders (not in the tests of the world).
        if app.is_plugin_added::<bevy::pbr::PbrPlugin>() {
            app.add_systems(PostUpdate, patch_shadow_shader);
        }
    }
}

/// Patches Bevy's shadow shader as soon as it loads (and again if it is
/// loaded again).
fn patch_shadow_shader(
    mut events: MessageReader<AssetEvent<Shader>>,
    shaders: Option<ResMut<Assets<Shader>>>,
    mut warned: Local<bool>,
) {
    let Some(mut shaders) = shaders else {
        return;
    };
    let module = ShaderImport::Custom(MODULE.into());
    for event in events.read() {
        let (AssetEvent::Added { id }
        | AssetEvent::Modified { id }
        | AssetEvent::LoadedWithDependencies { id }) = event
        else {
            continue;
        };
        let Some(shader) = shaders.get(*id) else {
            continue;
        };
        if shader.import_path != module {
            continue;
        }
        let Source::Wgsl(source) = &shader.source else {
            continue;
        };
        if source.contains("motorust:") {
            continue;
        }
        match fade_far_shadows(source) {
            Some(patched) => {
                if let Some(mut shader) = shaders.get_mut(*id) {
                    shader.source = Source::Wgsl(patched.into());
                    info!("The far shadows fade out");
                }
            }
            None if !*warned => {
                *warned = true;
                warn!("The far shadows do not fade: Bevy's shadow shader is not as expected");
            }
            None => {}
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn the_far_shadows_fade_once() {
        let shader = "fn fetch_directional_shadow() -> f32 {\n    var shadow = 0.5;\n    return shadow;\n}\n\nfn cascade_debug_visualization() {}\n";
        let patched = fade_far_shadows(shader).expect("the place is there");
        assert!(patched.contains("smoothstep(0.8 * last_far_bound, last_far_bound, -view_z)"));
        // Before it returns, and only there.
        let fade = patched.find("shadow = mix").unwrap();
        assert!(fade < patched.find("return shadow;").unwrap());
        assert_eq!(patched.matches("shadow = mix").count(), 1);
        // Not twice, and not where the code is not as expected.
        assert_eq!(fade_far_shadows(&patched), None);
        assert_eq!(fade_far_shadows("fn other() {}"), None);
    }
}
