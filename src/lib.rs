//! Raster Tools — an external Open CAD Studio add-on for raster images
//! (GIF, JPG, PNG, BMP, TIFF, WebP) referenced by a drawing.
//!
//! * rectangular clip: keep a window, cut a hole, or split a piece off into
//!   its own image entity;
//! * resize: by dragging a corner, or by typed width / height / factor / DPI;
//! * relink: change the file on disk an image points to, or find moved
//!   files in a folder;
//! * list every image with its path, pixel size and whether the file exists.
//!
//! The crate depends only on `ocs_plugin_api` (with the `host` feature), whose
//! re-exported `codec` is the host's entity model, so plugin and host share
//! one set of types. Geometry ([`geom`]) and image-header parsing
//! ([`imgsize`]) are host-free and unit-tested on their own; [`commands`] and
//! [`tools`] are the glue to the host.

use ocs_plugin_api::host::{BuiltinPlugin, HostApi};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::CadModule;

#[macro_use]
pub mod i18n;
pub mod commands;
pub mod geom;
pub mod imgsize;
pub mod pixels;
mod ribbon;
pub mod tools;

/// Reverse-DNS plugin id; also the folder name under `plugins/`.
pub const PLUGIN_ID: &str = "opencad.rastertools";

// Keep in sync with plugin.toml (the version comes from Cargo.toml).
static MANIFEST: PluginManifest = PluginManifest {
    id: PLUGIN_ID,
    name: "Raster Tools",
    version: env!("CARGO_PKG_VERSION"),
    description: "Raster images (GIF, JPG, PNG, BMP, TIFF, WebP): insert, clip with a rectangle, polygon \
                  or polyline, cut holes and split pieces, resize on screen or by size/DPI, change the \
                  file path and find moved files. English and Italian.",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 60,
    xdata_apps: &[],
    command_prefixes: &["RT_"],
};

struct RasterToolsPlugin;

impl BuiltinPlugin for RasterToolsPlugin {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }

    fn ribbon(&self) -> Box<dyn CadModule> {
        Box::new(ribbon::RasterModule)
    }

    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        commands::handle(host, cmd)
    }
}

ocs_plugin_api::export_plugin!(RasterToolsPlugin);
