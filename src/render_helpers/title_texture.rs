use std::cmp::min;

use anyhow::ensure;
use pango::FontDescription;
use pangocairo::cairo::{self, ImageSurface};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::utils::Transform;

use super::texture::TextureBuffer;
use crate::utils::to_physical_precise_round;

pub(crate) const FONT: &str = "sans 14px";

/// Cached title texture.
#[derive(Debug, Default)]
pub(crate) struct TitleTexture {
    title: String,
    scale: f64,
    texture: Option<Option<TextureBuffer<GlesTexture>>>,
}

impl TitleTexture {
    pub(crate) fn get(
        &mut self,
        renderer: &mut GlesRenderer,
        title: &str,
        scale: f64,
    ) -> Option<TextureBuffer<GlesTexture>> {
        if self.title != title || self.scale != scale {
            self.texture = None;
            self.title = title.to_owned();
            self.scale = scale;
        }

        self.texture
            .get_or_insert_with(|| generate_title_texture(renderer, title, scale).ok())
            .clone()
    }

    pub(crate) fn get_stale(&self) -> Option<&TextureBuffer<GlesTexture>> {
        if let Some(Some(texture)) = &self.texture {
            Some(texture)
        } else {
            None
        }
    }
}

fn generate_title_texture(
    renderer: &mut GlesRenderer,
    title: &str,
    scale: f64,
) -> anyhow::Result<TextureBuffer<GlesTexture>> {
    let _span = tracy_client::span!("generate_title_texture");

    let mut font = FontDescription::from_string(FONT);
    font.set_absolute_size(to_physical_precise_round(scale, font.size()));

    let surface = ImageSurface::create(cairo::Format::ARgb32, 0, 0)?;
    let cr = cairo::Context::new(&surface)?;
    let layout = pangocairo::functions::create_layout(&cr);
    layout.context().set_round_glyph_positions(false);
    // On Window CSD, line breaks are either stripped or replaced with the linebreak symbol anyway.
    // No use rendering it as multiple lines.
    layout.set_single_paragraph_mode(true);
    layout.set_font_description(Some(&font));
    layout.set_text(title);

    let (width, height) = layout.pixel_size();
    ensure!(width > 0 && height > 0);

    // Guard against overly long window titles.
    let width = min(width, 16383);
    let height = min(height, 16383);

    let surface = ImageSurface::create(cairo::Format::ARgb32, width, height)?;
    let cr = cairo::Context::new(&surface)?;
    cr.set_source_rgb(1., 1., 1.);
    pangocairo::functions::show_layout(&cr, &layout);

    drop(cr);
    let data = surface.take_data().unwrap();
    let buffer = TextureBuffer::from_memory(
        renderer,
        &data,
        Fourcc::Argb8888,
        (width, height),
        false,
        scale,
        Transform::Normal,
        Vec::new(),
    )?;

    Ok(buffer)
}
