//! Pattern (b), compiled only with `--features pattern-b`.
//!
//! Slint creates the wgpu device (`WGPUConfiguration::Automatic`) and Bevy is
//! started with `RenderCreation::manual` on that same device, rendering the
//! lit cube into a texture Slint displays. This is the shape of Slint's
//! `slint-hosts-bevy` example at the pinned revision, cut down to a cube.
//!
//! It is not the Android entry. Stock GameActivity loads this cdylib and
//! Bevy's `android_main` owns the loop. Slint's winit backend is not built
//! for Android, and its android-activity backend would be a second activity.

#![allow(dead_code, unused_imports)]

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::renderer::WgpuWrapper;
use bevy::render::settings::RenderCreation;
use bevy::render::{RenderApp, RenderPlugin};
use slint::wgpu_29::wgpu;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

pub fn shared_device_from_slint(
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
) -> Result<(), String> {
    let adapter = spin_on::spin_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .map_err(|err| format!("adapter: {err:?}"))?;

    let render_device: bevy::render::renderer::RenderDevice = device.clone().into();
    let render_queue =
        bevy::render::renderer::RenderQueue(std::sync::Arc::new(WgpuWrapper::new(queue.clone())));
    let render_adapter_info =
        bevy::render::renderer::RenderAdapterInfo(WgpuWrapper::new(adapter.get_info()));
    let render_adapter =
        bevy::render::renderer::RenderAdapter(std::sync::Arc::new(WgpuWrapper::new(adapter)));
    let render_instance =
        bevy::render::renderer::RenderInstance(std::sync::Arc::new(WgpuWrapper::new(instance)));

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pattern-b-scene"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    let mut app = App::new();
    app.insert_resource(PatternBTexture(Some(texture)))
        .add_plugins(
            DefaultPlugins
                .build()
                .disable::<bevy::winit::WinitPlugin>()
                .set(RenderPlugin {
                    render_creation: RenderCreation::manual(
                        render_device,
                        render_queue,
                        render_adapter_info,
                        render_adapter,
                        render_instance,
                    ),
                    ..default()
                }),
        )
        .add_plugins(ExtractResourcePlugin::<PatternBTexture>::default());
    // Construction is the check. Running would need Slint's event loop, which
    // the GameActivity cdylib does not hand over.
    let _ = app;
    Ok(())
}

#[derive(Resource, Clone, ExtractResource)]
struct PatternBTexture(Option<wgpu::Texture>);

#[allow(dead_code)]
fn point_camera_at_texture(camera: &mut RenderTarget) {
    *camera = RenderTarget::TextureView(bevy::camera::ManualTextureViewHandle(1));
}
