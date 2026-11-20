use std::cell::RefCell;
use std::iter::zip;
use std::mem;

use niri_config::{CornerRadius, Gradient, GradientRelativeTo, TabIndicatorPosition};
use smithay::backend::renderer::element::Kind;
use smithay::utils::{Logical, Point, Rectangle, Size};

use super::tile::Tile;
use super::LayoutElement;
use crate::animation::{Animation, Clock};
use crate::niri_render_elements;
use crate::render_helpers::border::BorderRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::texture::TextureRenderElement;
use crate::render_helpers::title_texture::TitleTexture;
use crate::render_helpers::RenderTarget;
use crate::utils::{
    floor_logical_in_physical_max1, round_logical_in_physical, round_logical_in_physical_max1,
};

#[derive(Debug)]
pub struct TabIndicator {
    labels: Vec<TitleRow>,
    working_height: f64,
    shader_locs: Vec<Point<f64, Logical>>,
    shaders: Vec<BorderRenderElement>,
    open_anim: Option<Animation>,
    config: niri_config::TabIndicator,
}

#[derive(Debug, Default)]
struct TitleRow {
    text: String,
    rect: Rectangle<f64, Logical>,
    scale: f64,
    blocked: Option<niri_config::BlockOutFrom>,
    texture: RefCell<TitleTexture>,
}

#[derive(Debug)]
pub struct TabInfo {
    label: Option<(String, Option<niri_config::BlockOutFrom>)>,
    /// Gradient for the tab indicator.
    pub gradient: Gradient,
    /// Tab geometry in the same coordinate system as the area.
    pub geometry: Rectangle<f64, Logical>,
}

niri_render_elements! {
    TabIndicatorRenderElement => {
        Label = PrimaryGpuTextureRenderElement,
        Gradient = BorderRenderElement,
    }
}

impl TabIndicator {
    pub fn new(config: niri_config::TabIndicator, working_height: f64) -> Self {
        Self {
            shader_locs: Vec::new(),
            shaders: Vec::new(),
            labels: Vec::new(),
            working_height,
            open_anim: None,
            config,
        }
    }

    pub fn update_config(&mut self, config: niri_config::TabIndicator, working_height: f64) {
        self.working_height = working_height;
        self.config = config;
    }

    pub fn update_shaders(&mut self) {
        for elem in &mut self.shaders {
            elem.damage_all();
        }
    }

    pub fn advance_animations(&mut self) {
        if let Some(anim) = &mut self.open_anim {
            if anim.is_done() {
                self.open_anim = None;
            }
        }
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.open_anim.is_some()
    }

    pub fn start_open_animation(&mut self, clock: Clock, config: niri_config::Animation) {
        self.open_anim = Some(Animation::new(clock, 0., 1., 0., config));
    }

    fn stack_height(&self, count: usize, scale: f64) -> f64 {
        // Leave at least half the work area for the active window, even in large stacks.
        round_logical_in_physical(scale, (24. * count as f64).min(self.working_height / 2.))
    }

    fn tab_rects(
        &self,
        area: Rectangle<f64, Logical>,
        count: usize,
        scale: f64,
    ) -> impl Iterator<Item = Rectangle<f64, Logical>> {
        let stacked = self.config.stacked_titles;
        let height = self.stack_height(count, scale);
        let mut markers = self.marker_rects(area, count, scale);
        (0..count).map(move |i| {
            if !stacked {
                return markers.next().unwrap();
            }
            let edge =
                |i: usize| round_logical_in_physical(scale, height * i as f64 / count as f64);
            let top = edge(i);
            Rectangle::new(
                Point::new(area.loc.x, (area.loc.y - height) + top),
                Size::from((area.size.w, edge(i + 1) - top)),
            )
        })
    }

    fn marker_rects(
        &self,
        area: Rectangle<f64, Logical>,
        count: usize,
        scale: f64,
    ) -> impl Iterator<Item = Rectangle<f64, Logical>> {
        let round = |logical: f64| round_logical_in_physical(scale, logical);
        let round_max1 = |logical: f64| round_logical_in_physical_max1(scale, logical);

        let progress = self.open_anim.as_ref().map_or(1., |a| a.value().max(0.));

        let width = round_max1(self.config.width);
        let gap = self.config.gap;
        let gap = round_max1(gap.abs()).copysign(gap);
        let gaps_between = round_max1(self.config.gaps_between_tabs);

        let position = self.config.position;
        let side = match position {
            TabIndicatorPosition::Left | TabIndicatorPosition::Right => area.size.h,
            TabIndicatorPosition::Top | TabIndicatorPosition::Bottom => area.size.w,
        };
        let total_prop = self.config.length.total_proportion.unwrap_or(0.5);
        let min_length = round(side * total_prop.clamp(0., 2.));

        // Compute px_per_tab before applying the animation to gaps_between in order to avoid it
        // growing and shrinking over the duration of the animation.
        let pixel = 1. / scale;
        let shortest_length = count as f64 * (pixel + gaps_between) - gaps_between;
        let length = f64::max(min_length, shortest_length);
        let px_per_tab = (length + gaps_between) / count as f64 - gaps_between;

        let px_per_tab = px_per_tab * progress;
        let gaps_between = round(self.config.gaps_between_tabs * progress);

        let length = count as f64 * (px_per_tab + gaps_between) - gaps_between;
        let px_per_tab = floor_logical_in_physical_max1(scale, px_per_tab);
        let floored_length = count as f64 * (px_per_tab + gaps_between) - gaps_between;
        let mut ones_left = ((length - floored_length) / pixel).round() as usize;

        let mut shader_loc = Point::from((-gap - width, round((side - length) / 2.)));
        match position {
            TabIndicatorPosition::Left => (),
            TabIndicatorPosition::Right => shader_loc.x = area.size.w + gap,
            TabIndicatorPosition::Top => mem::swap(&mut shader_loc.x, &mut shader_loc.y),
            TabIndicatorPosition::Bottom => {
                shader_loc.x = shader_loc.y;
                shader_loc.y = area.size.h + gap;
            }
        }
        shader_loc += area.loc;

        (0..count).map(move |_| {
            let mut px_per_tab = px_per_tab;
            if ones_left > 0 {
                ones_left -= 1;
                px_per_tab += pixel;
            }

            let loc = shader_loc;

            match position {
                TabIndicatorPosition::Left | TabIndicatorPosition::Right => {
                    shader_loc.y += px_per_tab + gaps_between
                }
                TabIndicatorPosition::Top | TabIndicatorPosition::Bottom => {
                    shader_loc.x += px_per_tab + gaps_between
                }
            }

            let size = match position {
                TabIndicatorPosition::Left | TabIndicatorPosition::Right => {
                    Size::from((width, px_per_tab))
                }
                TabIndicatorPosition::Top | TabIndicatorPosition::Bottom => {
                    Size::from((px_per_tab, width))
                }
            };

            Rectangle::new(loc, size)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_render_elements(
        &mut self,
        enabled: bool,
        // Geometry of the tabs area.
        area: Rectangle<f64, Logical>,
        // View rect relative to the tabs area.
        area_view_rect: Rectangle<f64, Logical>,
        // Tab count, should match the tabs iterator length.
        tab_count: usize,
        tabs: impl Iterator<Item = TabInfo>,
        is_active: bool,
        scale: f64,
    ) {
        if !enabled || self.config.off {
            self.labels.clear();
            self.shader_locs.clear();
            self.shaders.clear();
            return;
        }

        let count = tab_count;
        if self.config.hide_when_single_tab && count == 1 {
            self.labels.clear();
            self.shader_locs.clear();
            self.shaders.clear();
            return;
        }

        self.labels.resize_with(
            if self.config.stacked_titles { count } else { 0 },
            Default::default,
        );
        self.shaders.resize_with(count, Default::default);
        self.shader_locs.resize_with(count, Default::default);

        let position = self.config.position;
        let radius = self.config.corner_radius as f32;
        let shared_rounded_corners = self.config.gaps_between_tabs == 0.;
        let mut tabs_left = tab_count;

        let rects = self.tab_rects(area, count, scale);
        for ((shader, loc), (tab, rect)) in zip(
            zip(&mut self.shaders, &mut self.shader_locs),
            zip(tabs, rects),
        ) {
            *loc = rect.loc;
            if let Some((text, blocked)) = tab.label {
                let row = &mut self.labels[count - tabs_left];
                row.text = text;
                row.rect = rect;
                row.blocked = blocked;
                row.scale = scale;
            }

            let mut gradient_area = match tab.gradient.relative_to {
                GradientRelativeTo::Window => tab.geometry,
                GradientRelativeTo::WorkspaceView => area_view_rect,
            };
            gradient_area.loc -= *loc;

            let mut color_from = tab.gradient.from;
            let mut color_to = tab.gradient.to;
            if !is_active {
                color_from *= 0.5;
                color_to *= 0.5;
            }

            if self.config.stacked_titles {
                // Keep white text readable over bright focus-ring colors.
                for color in [&mut color_from, &mut color_to] {
                    color.r *= 0.45;
                    color.g *= 0.45;
                    color.b *= 0.45;
                }
            }

            let radius = if shared_rounded_corners && tab_count > 1 {
                if tabs_left == tab_count {
                    // First tab.
                    match position {
                        TabIndicatorPosition::Left | TabIndicatorPosition::Right => CornerRadius {
                            top_left: radius,
                            top_right: radius,
                            bottom_right: 0.,
                            bottom_left: 0.,
                        },
                        TabIndicatorPosition::Top | TabIndicatorPosition::Bottom => CornerRadius {
                            top_left: radius,
                            top_right: 0.,
                            bottom_right: 0.,
                            bottom_left: radius,
                        },
                    }
                } else if tabs_left == 1 {
                    // Last tab.
                    match position {
                        TabIndicatorPosition::Left | TabIndicatorPosition::Right => CornerRadius {
                            top_left: 0.,
                            top_right: 0.,
                            bottom_right: radius,
                            bottom_left: radius,
                        },
                        TabIndicatorPosition::Top | TabIndicatorPosition::Bottom => CornerRadius {
                            top_left: 0.,
                            top_right: radius,
                            bottom_right: radius,
                            bottom_left: 0.,
                        },
                    }
                } else {
                    // Tab in the middle.
                    CornerRadius::default()
                }
            } else {
                // Separate tabs, or the only tab.
                CornerRadius::from(radius)
            };
            let radius = radius.fit_to(rect.size.w as f32, rect.size.h as f32);
            tabs_left -= 1;

            shader.update(
                rect.size,
                gradient_area,
                tab.gradient.in_,
                color_from,
                color_to,
                ((tab.gradient.angle as f32) - 90.).to_radians(),
                Rectangle::from_size(rect.size),
                0.,
                radius,
                scale as f32,
                1.,
            );
        }
    }

    pub fn hit(
        &self,
        area: Rectangle<f64, Logical>,
        tab_count: usize,
        scale: f64,
        point: Point<f64, Logical>,
    ) -> Option<usize> {
        if self.config.off {
            return None;
        }

        let count = tab_count;
        if self.config.hide_when_single_tab && count == 1 {
            return None;
        }

        let rects = self.tab_rects(area, count, scale);
        if self.config.stacked_titles {
            if point.x < area.loc.x || point.x >= area.loc.x + area.size.w || point.y >= area.loc.y {
                return None;
            }
            // Compare against the shared row starts: adding a fractional row height back
            // to its origin can otherwise make two rows claim the same boundary.
            return rects.take_while(|rect| rect.loc.y <= point.y).count().checked_sub(1);
        }

        rects.enumerate()
            .find_map(|(idx, rect)| rect.contains(point).then_some(idx))
    }

    pub fn render(
        &self,
        renderer: &mut impl NiriRenderer,
        target: RenderTarget,
        pos: Point<f64, Logical>,
        push: &mut dyn FnMut(TabIndicatorRenderElement),
    ) {
        let has_border_shader = BorderRenderElement::has_shader(renderer);
        if !has_border_shader {
            return;
        }

        for label in &self.labels {
            if target.should_block_out(label.blocked)
                || label.rect.size.w <= 12.
                || label.rect.size.h <= 0.
            {
                continue;
            }
            let Some(texture) = label.texture.borrow_mut().get(
                renderer.as_gles_renderer(),
                &label.text,
                label.scale,
            ) else {
                continue;
            };
            let mut size = texture.logical_size();
            size.w = size.w.min(label.rect.size.w - 12.);
            size.h = size.h.min(label.rect.size.h);
            let loc = pos + label.rect.loc + Point::new(6., (label.rect.size.h - size.h) / 2.);
            let elem = TextureRenderElement::from_texture_buffer(
                texture,
                loc,
                1.,
                Some(Rectangle::from_size(size)),
                None,
                Kind::Unspecified,
            );
            push(PrimaryGpuTextureRenderElement(elem).into());
        }

        for (shader, loc) in zip(&self.shaders, &self.shader_locs) {
            let elem = shader.clone().with_location(pos + *loc);
            push(TabIndicatorRenderElement::from(elem));
        }
    }

    /// Extra size occupied by the tab indicator.
    pub fn extra_size(&self, tab_count: usize, scale: f64) -> Size<f64, Logical> {
        if self.config.off
            || (!self.config.stacked_titles && !self.config.place_within_column)
            || (self.config.hide_when_single_tab && tab_count == 1)
        {
            return Size::from((0., 0.));
        }

        let round = |logical: f64| round_logical_in_physical(scale, logical);
        if self.config.stacked_titles {
            return Size::from((0., self.stack_height(tab_count, scale)));
        }
        let width = round(self.config.width);
        let gap = round(self.config.gap);

        // No, I am *not* falling into the rabbit hole of "what if the tab indicator is wide enough
        // that it peeks from the other side of the window".
        let size = f64::max(0., width + gap);

        match self.config.position {
            TabIndicatorPosition::Left | TabIndicatorPosition::Right => Size::from((size, 0.)),
            TabIndicatorPosition::Top | TabIndicatorPosition::Bottom => Size::from((0., size)),
        }
    }

    /// Offset of the tabbed content due to space occupied by the tab indicator.
    pub fn content_offset(&self, tab_count: usize, scale: f64) -> Point<f64, Logical> {
        if self.config.stacked_titles {
            return self.extra_size(tab_count, scale).to_point();
        }
        match self.config.position {
            TabIndicatorPosition::Left | TabIndicatorPosition::Top => {
                self.extra_size(tab_count, scale).to_point()
            }
            TabIndicatorPosition::Right | TabIndicatorPosition::Bottom => Point::from((0., 0.)),
        }
    }

    pub fn config(&self) -> niri_config::TabIndicator {
        self.config
    }
}

impl TabInfo {
    pub fn from_tile<W: LayoutElement>(
        tile: &Tile<W>,
        position: Point<f64, Logical>,
        is_active: bool,
        is_urgent: bool,
        config: &niri_config::TabIndicator,
    ) -> Self {
        let rules = tile.window().rules();
        let rule = rules.tab_indicator;

        let gradient_from_rule = || {
            let (color, gradient) = if is_urgent {
                (rule.urgent_color, rule.urgent_gradient)
            } else if is_active {
                (rule.active_color, rule.active_gradient)
            } else {
                (rule.inactive_color, rule.inactive_gradient)
            };
            let color = color.map(Gradient::from);
            gradient.or(color)
        };

        let gradient_from_config = || {
            let (color, gradient) = if is_urgent {
                (config.urgent_color, config.urgent_gradient)
            } else if is_active {
                (config.active_color, config.active_gradient)
            } else {
                (config.inactive_color, config.inactive_gradient)
            };
            let color = color.map(Gradient::from);
            gradient.or(color)
        };

        let gradient_from_border = || {
            // Come up with tab indicator gradient matching the focus ring or the border, whichever
            // one is enabled.
            let focus_ring_config = tile.focus_ring().config();
            let border_config = tile.border().config();
            let config = if focus_ring_config.off {
                border_config
            } else {
                focus_ring_config
            };

            let (color, gradient) = if is_urgent {
                (config.urgent_color, config.urgent_gradient)
            } else if is_active {
                (config.active_color, config.active_gradient)
            } else {
                (config.inactive_color, config.inactive_gradient)
            };
            gradient.unwrap_or_else(|| Gradient::from(color))
        };

        let gradient = gradient_from_rule()
            .or_else(gradient_from_config)
            .unwrap_or_else(gradient_from_border);

        let geometry = Rectangle::new(position, tile.animated_tile_size());

        let label = config
            .stacked_titles
            .then(|| (tile.window().tab_title(), rules.block_out_from));
        TabInfo {
            gradient,
            geometry,
            label,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacked_rows_share_reserved_space_and_hit_boundaries() {
        let mut config = niri_config::TabIndicator::default();
        config.stacked_titles = true;
        config.hide_when_single_tab = true;
        let indicator = TabIndicator::new(config, 100.);
        let scale = 1.5;
        assert_eq!(indicator.extra_size(1, scale).h, 0.);
        let height = indicator.extra_size(3, scale).h;
        assert_eq!(height, 50.);
        let area = Rectangle::new(Point::from((12., height)), Size::from((200., 50.)));
        let mut bottom = 0.;
        for (i, row) in indicator.tab_rects(area, 3, scale).enumerate() {
            assert!((row.loc.y - bottom).abs() < 1e-6);
            assert_eq!(row.size.w, area.size.w);
            assert_eq!(indicator.hit(area, 3, scale, row.loc), Some(i));
            bottom = row.loc.y + row.size.h;
        }
        assert_eq!(bottom, area.loc.y);
        assert_eq!(indicator.hit(area, 3, scale, area.loc), None);
    }
}
