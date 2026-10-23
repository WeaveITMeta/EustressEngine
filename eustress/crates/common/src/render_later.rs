//! Render setup that has to wait for [`RenderApp`].
//!
//! Desktop adds `RenderPlugin` with `DefaultPlugins`, before terrain and the
//! play runtime. The mobile Player does not: [`crate`] callers add those
//! plugins first, and `SurfaceGate::finish` adds the renderer only after the
//! Android window exists. A plugin that returns from `build` when `RenderApp`
//! is missing never gets another chance. `Plugin::cleanup` is too late to
//! `add_plugins` (the app is already finished).
//!
//! Call [`when_render_app`] from `build`. It runs the install immediately when
//! the render app is already there, and otherwise queues it.
//! `SurfaceGate` adds [`InstallDeferredRenderPlugin`] immediately after
//! `RenderPlugin`, while plugin state is still `Adding`, so the queued
//! `MaterialPlugin`s are built and then finished in the same pass.

use bevy::prelude::*;
use bevy::render::RenderApp;
use tracing::debug;

/// Install functions waiting for [`RenderApp`].
#[derive(Resource, Default)]
pub struct DeferredRenderInstalls {
    queue: Vec<fn(&mut App)>,
}

/// Run `install` now if [`RenderApp`] exists, otherwise queue it for
/// [`InstallDeferredRenderPlugin`].
pub fn when_render_app(app: &mut App, install: fn(&mut App)) {
    if app.get_sub_app(RenderApp).is_some() {
        install(app);
        return;
    }
    debug!(
        target: "eustress::render",
        "RenderApp is not up yet; deferring a render install until the renderer is added",
    );
    app.init_resource::<DeferredRenderInstalls>();
    app.world_mut()
        .resource_mut::<DeferredRenderInstalls>()
        .queue
        .push(install);
}

/// Drains [`DeferredRenderInstalls`]. Add this after `RenderPlugin`.
pub struct InstallDeferredRenderPlugin;

impl Plugin for InstallDeferredRenderPlugin {
    fn build(&self, app: &mut App) {
        let queue = app
            .world_mut()
            .get_resource_mut::<DeferredRenderInstalls>()
            .map(|mut installs| std::mem::take(&mut installs.queue))
            .unwrap_or_default();
        for install in queue {
            install(app);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource)]
    struct Installed;

    fn install(app: &mut App) {
        app.insert_resource(Installed);
    }

    #[test]
    fn queues_until_the_render_app_exists() {
        let mut app = App::new();
        when_render_app(&mut app, install);
        assert!(app.world().get_resource::<Installed>().is_none());
        app.insert_sub_app(RenderApp, SubApp::new());
        app.add_plugins(InstallDeferredRenderPlugin);
        assert!(app.world().contains_resource::<Installed>());
    }

    #[test]
    fn runs_immediately_when_the_render_app_exists() {
        let mut app = App::new();
        app.insert_sub_app(RenderApp, SubApp::new());
        when_render_app(&mut app, install);
        assert!(app.world().contains_resource::<Installed>());
    }
}
