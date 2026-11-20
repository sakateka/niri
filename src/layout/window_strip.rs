//! An output-fixed attention indicator, in workspace, column and tab order.
use smithay::backend::renderer::element::Kind;

use super::workspace::Workspace;
use super::LayoutElement;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};

pub(super) const HEIGHT: f64 = 2.;

#[derive(Debug, Default)]
pub(super) struct WindowStrip {
    background: SolidColorBuffer,
    segments: Vec<(f64, SolidColorBuffer)>,
}

impl WindowStrip {
    pub(super) fn update<W: LayoutElement>(
        &mut self,
        workspaces: &[Workspace<W>],
        active_workspace: usize,
        focused: bool,
        width: f64,
        scale: f64,
    ) {
        let count = workspaces
            .iter()
            .map(|ws| ws.windows().count())
            .sum::<usize>();
        self.background
            .update((width, HEIGHT), [0.09, 0.10, 0.12, 1.]);
        self.segments.resize_with(count, Default::default);
        let edge = |i: usize| (i as f64 * width * scale / count as f64).round();
        let mut index = 0;
        for (ws_idx, ws) in workspaces.iter().enumerate() {
            let active = ws_idx == active_workspace;
            let selected = ws.active_window().map(|w| w.id());
            for (within_ws, window) in ws.windows().enumerate() {
                let color = if window.is_urgent() {
                    [1., 0.18, 0.20, 1.]
                } else if active && selected == Some(window.id()) {
                    if focused {
                        [0.50, 0.78, 1., 1.]
                    } else {
                        [0.30, 0.46, 0.58, 1.]
                    }
                } else if active {
                    [0.36, 0.38, 0.41, 1.]
                } else {
                    [0.22, 0.24, 0.26, 1.]
                };
                let gap = if index == 0 {
                    0.
                } else if within_ws == 0 {
                    4.
                } else {
                    1.
                };
                let right = edge(index + 1);
                let left = edge(index);
                let left = left + (gap * scale).round().min((right - left - 1.).max(0.));
                let (x, buffer) = &mut self.segments[index];
                *x = left / scale;
                buffer.update(((right - left) / scale, HEIGHT), color);
                index += 1;
            }
        }
    }

    pub(super) fn render(&self, push: &mut dyn FnMut(SolidColorRenderElement)) {
        for (x, buffer) in &self.segments {
            push(SolidColorRenderElement::from_buffer(
                buffer,
                (*x, 0.),
                1.,
                Kind::Unspecified,
            ));
        }
        push(SolidColorRenderElement::from_buffer(
            &self.background,
            (0., 0.),
            1.,
            Kind::Unspecified,
        ));
    }
}
