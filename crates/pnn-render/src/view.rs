//! Zoom and scroll state for one document's canvas. Pure maths, no GPU.
//!
//! Zoom presets, stepping and fit-to-window are ported from paint.net 3.36
//! `Core/ScaleFactor.cs` and `Data/DocumentView.cs` (MIT, see NOTICE). All coordinates here
//! are physical screen pixels, so 100% zoom means one image pixel per device pixel.

/// paint.net's zoom steps (`ScaleFactor.scales`): 1% to 3200%.
pub const ZOOM_PRESETS: [f64; 25] = [
    0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.12, 0.16, 0.25, 0.33, 0.50, 0.66, 1.0, 2.0, 3.0,
    4.0, 5.0, 6.0, 7.0, 8.0, 12.0, 16.0, 24.0, 32.0,
];
pub const MIN_ZOOM: f64 = 0.01;
pub const MAX_ZOOM: f64 = 32.0;

/// `ScaleFactor.GetNextLarger`.
pub fn next_zoom_in(zoom: f64) -> f64 {
    let ratio = zoom + 0.005;
    *ZOOM_PRESETS
        .iter()
        .find(|&&s| ratio <= s)
        .unwrap_or(&MAX_ZOOM)
}

/// `ScaleFactor.GetNextSmaller`.
pub fn next_zoom_out(zoom: f64) -> f64 {
    let ratio = zoom - 0.005;
    match ZOOM_PRESETS.iter().position(|&s| ratio <= s) {
        Some(0) => ZOOM_PRESETS[0],
        Some(i) => ZOOM_PRESETS[i - 1],
        None => ZOOM_PRESETS[ZOOM_PRESETS.len() - 1],
    }
}

/// `DocumentView.ZoomToWindow`: largest zoom showing the whole image with a 10 px margin,
/// never above 100% and never below 1%.
pub fn fit_zoom(image: [f64; 2], viewport: [f64; 2]) -> f64 {
    let zx = (viewport[0] - 10.0).max(1.0) / image[0];
    let zy = (viewport[1] - 10.0).max(1.0) / image[1];
    zx.min(zy).clamp(MIN_ZOOM, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub zoom: f64,
    /// Image coordinate shown at the centre of the viewport.
    pub center: [f64; 2],
    /// paint.net's `ZoomBasis.FitToWindow`: keep refitting as the window resizes, until the
    /// user picks a zoom.
    pub fit_to_window: bool,
}

impl View {
    /// New documents start fitted to the window, as in paint.net.
    pub fn fitted(image: [f64; 2], viewport: [f64; 2]) -> Self {
        Self {
            zoom: fit_zoom(image, viewport),
            center: [image[0] / 2.0, image[1] / 2.0],
            fit_to_window: true,
        }
    }

    pub fn image_to_screen(&self, p: [f64; 2], viewport: [f64; 2]) -> [f64; 2] {
        [
            viewport[0] / 2.0 + (p[0] - self.center[0]) * self.zoom,
            viewport[1] / 2.0 + (p[1] - self.center[1]) * self.zoom,
        ]
    }

    pub fn screen_to_image(&self, s: [f64; 2], viewport: [f64; 2]) -> [f64; 2] {
        [
            self.center[0] + (s[0] - viewport[0] / 2.0) / self.zoom,
            self.center[1] + (s[1] - viewport[1] / 2.0) / self.zoom,
        ]
    }

    /// Call every frame with the current viewport: refits in fit-to-window mode, otherwise
    /// keeps the scroll position legal.
    pub fn update(&mut self, image: [f64; 2], viewport: [f64; 2]) {
        if self.fit_to_window {
            self.zoom = fit_zoom(image, viewport);
            self.center = [image[0] / 2.0, image[1] / 2.0];
        } else {
            self.clamp(image, viewport);
        }
    }

    /// The image may be scrolled until its edge reaches the middle of the viewport, so the
    /// view centre stays inside the image (paint.net stops at the edge; the user asked for
    /// room to work near the edges). This holds whether or not the image fits: paint.net
    /// pins a fitting image in the middle, but the user wants to drag it about too
    /// (2026-10-15). Fit to Window still centres it.
    pub fn clamp(&mut self, image: [f64; 2], _viewport: [f64; 2]) {
        for (c, &len) in self.center.iter_mut().zip(&image) {
            *c = c.clamp(0.0, len);
        }
    }

    /// Set zoom keeping the image point under `anchor` (screen px) fixed.
    pub fn zoom_about(&mut self, zoom: f64, anchor: [f64; 2], image: [f64; 2], viewport: [f64; 2]) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let p = self.screen_to_image(anchor, viewport);
        self.zoom = zoom;
        self.fit_to_window = false;
        self.center = [
            p[0] - (anchor[0] - viewport[0] / 2.0) / zoom,
            p[1] - (anchor[1] - viewport[1] / 2.0) / zoom,
        ];
        self.clamp(image, viewport);
    }

    /// Zoom keeping the viewport centre fixed (keyboard zoom, View menu).
    pub fn zoom_centered(&mut self, zoom: f64, image: [f64; 2], viewport: [f64; 2]) {
        self.zoom_about(
            zoom,
            [viewport[0] / 2.0, viewport[1] / 2.0],
            image,
            viewport,
        );
    }

    /// Scroll by a screen-pixel delta (positive = content moves left/up, as with scrollbars).
    pub fn scroll_by(&mut self, delta: [f64; 2], image: [f64; 2], viewport: [f64; 2]) {
        self.fit_to_window = false;
        self.center[0] += delta[0] / self.zoom;
        self.center[1] += delta[1] / self.zoom;
        self.clamp(image, viewport);
    }

    /// Scroll so the image's top-left (`Home ×2`) or bottom-right (`End ×2`) corner meets the
    /// same corner of the viewport.
    pub fn scroll_to_corner(&mut self, bottom_right: bool, image: [f64; 2], viewport: [f64; 2]) {
        self.fit_to_window = false;
        for axis in 0..2 {
            let half = viewport[axis] / 2.0 / self.zoom;
            self.center[axis] = if bottom_right {
                image[axis] - half
            } else {
                half
            };
        }
        self.clamp(image, viewport);
    }

    /// Put an image point at the viewport centre without clamping (`Ctrl+Home`, `Ctrl+End`).
    pub fn center_on(&mut self, p: [f64; 2]) {
        self.fit_to_window = false;
        self.center = p;
    }

    /// Scrollable length along one axis in screen px: the image plus half a viewport of
    /// overscroll at each end. `None` when the image fits on that axis.
    pub fn scroll_length(&self, axis: usize, image: [f64; 2], viewport: [f64; 2]) -> Option<f64> {
        let content = image[axis] * self.zoom;
        (content > viewport[axis]).then_some(content + viewport[axis])
    }

    /// Visible part of the scrollable length along one axis as (start, length) fractions,
    /// for drawing scrollbars. `None` when the image fits on that axis.
    pub fn scrollbar(
        &self,
        axis: usize,
        image: [f64; 2],
        viewport: [f64; 2],
    ) -> Option<(f64, f64)> {
        let total = self.scroll_length(axis, image, viewport)?;
        let visible = viewport[axis] / total;
        let start = self.center[axis] * self.zoom / total;
        // A degenerate viewport (no room left for the canvas) must not panic.
        Some((start.clamp(0.0, (1.0 - visible).max(0.0)), visible))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMG: [f64; 2] = [8000.0, 6000.0];
    const VP: [f64; 2] = [1600.0, 1000.0];

    #[test]
    fn scrollbar_survives_a_viewport_with_no_room() {
        let v = View {
            zoom: 4.0,
            center: [50.0, 40.0],
            fit_to_window: false,
        };
        // The panels can leave the canvas no height at all (a very short window).
        let (start, len) = v.scrollbar(1, [100.0, 80.0], [800.0, -20.0]).unwrap();
        assert!(start.is_finite() && len.is_finite());
    }

    #[test]
    fn preset_stepping_matches_paint_net() {
        assert_eq!(next_zoom_in(1.0), 2.0);
        assert_eq!(next_zoom_out(1.0), 0.66);
        assert_eq!(next_zoom_in(0.66), 1.0);
        // Off-preset values snap to the neighbouring presets.
        assert_eq!(next_zoom_in(0.4), 0.5);
        assert_eq!(next_zoom_out(0.4), 0.33);
        // Within the 0.005 tolerance counts as on the preset.
        assert_eq!(next_zoom_in(0.997), 2.0);
        assert_eq!(next_zoom_in(32.0), 32.0);
        assert_eq!(next_zoom_out(0.01), 0.01);
    }

    #[test]
    fn fit_never_enlarges_and_keeps_margin() {
        assert_eq!(fit_zoom([100.0, 100.0], VP), 1.0);
        let z = fit_zoom(IMG, VP);
        assert!((z - 990.0 / 6000.0).abs() < 1e-12);
        assert_eq!(fit_zoom([1e9, 1e9], VP), MIN_ZOOM);
    }

    #[test]
    fn zoom_about_keeps_anchor_fixed() {
        let mut v = View::fitted(IMG, VP);
        let anchor = [1200.0, 300.0];
        let before = v.screen_to_image(anchor, VP);
        v.zoom_about(2.0, anchor, IMG, VP);
        let after = v.screen_to_image(anchor, VP);
        assert!((before[0] - after[0]).abs() < 1e-9 && (before[1] - after[1]).abs() < 1e-9);
        assert!(!v.fit_to_window);
    }

    #[test]
    fn small_images_pan_until_their_edge_is_mid_viewport() {
        let small = [200.0, 100.0];
        let mut v = View::fitted(small, VP);
        assert_eq!(v.center, [100.0, 50.0], "fitted: centred");
        // A little: it moves.
        v.scroll_by([30.0, -20.0], small, VP);
        assert_eq!(v.center, [130.0, 30.0]);
        // A lot: it stops with its far edge at the middle of the view.
        v.scroll_by([500.0, 500.0], small, VP);
        assert_eq!(v.center, [200.0, 100.0]);
        assert_eq!(v.image_to_screen(small, VP), [VP[0] / 2.0, VP[1] / 2.0]);
        // Fit to Window centres it again.
        assert_eq!(View::fitted(small, VP).center, [100.0, 50.0]);
    }

    #[test]
    fn scrolling_stops_with_the_edge_mid_viewport() {
        let mut v = View::fitted(IMG, VP);
        v.zoom_centered(1.0, IMG, VP);
        v.scroll_by([-1e9, -1e9], IMG, VP);
        assert_eq!(v.image_to_screen([0.0, 0.0], VP), [800.0, 500.0]);
        v.scroll_by([1e9, 1e9], IMG, VP);
        assert_eq!(v.image_to_screen(IMG, VP), [800.0, 500.0]);
        // Home ×2 / End ×2 still line the image corner up with the viewport corner.
        v.scroll_to_corner(false, IMG, VP);
        assert_eq!(v.image_to_screen([0.0, 0.0], VP), [0.0, 0.0]);
        v.scroll_to_corner(true, IMG, VP);
        assert_eq!(v.image_to_screen(IMG, VP), VP);
    }

    #[test]
    fn fit_mode_refits_on_resize_until_user_zooms() {
        let mut v = View::fitted(IMG, VP);
        v.update(IMG, [800.0, 500.0]);
        assert_eq!(v.zoom, fit_zoom(IMG, [800.0, 500.0]));
        v.zoom_centered(1.0, IMG, VP);
        v.update(IMG, [400.0, 300.0]);
        assert_eq!(v.zoom, 1.0);
    }

    #[test]
    fn scrollbar_fractions() {
        let mut v = View::fitted(IMG, VP);
        assert_eq!(v.scrollbar(0, IMG, VP), None);
        v.zoom_centered(1.0, IMG, VP);
        // Total = 8000 + 1600 overscroll; the corner position leaves half a viewport before it.
        v.scroll_to_corner(false, IMG, VP);
        let (start, len) = v.scrollbar(0, IMG, VP).unwrap();
        assert!((start - 800.0 / 9600.0).abs() < 1e-12);
        assert!((len - 1600.0 / 9600.0).abs() < 1e-12);
        v.scroll_by([-1e9, 0.0], IMG, VP);
        assert_eq!(v.scrollbar(0, IMG, VP).unwrap().0, 0.0);
    }
}
