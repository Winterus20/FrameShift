use crate::controller::WorkspaceUiController;
use frameshift_media::{AlphaMode, AudioBuffer, Rgba8Frame};
use frameshift_schema::{
    AssetKind, AudioClipDef, GraphNodeDef, HwAccelPolicy, NodeKind, PreferenceScope, Project,
    RenderProfileKind, TemplateSlotKind,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, ticks_to_audio_samples_nearest};
use std::collections::HashMap;

const BG_CANVAS: [u8; 4] = [12, 14, 20, 255];
const BG_PANEL: [u8; 4] = [20, 24, 34, 255];
const BG_HEADER: [u8; 4] = [28, 34, 48, 255];
const BG_SUBSURFACE: [u8; 4] = [15, 18, 26, 255];
const BORDER_SUBTLE: [u8; 4] = [44, 52, 70, 255];
const BORDER_ACCENT: [u8; 4] = [56, 189, 248, 255];
const TEXT_PRIMARY: [u8; 4] = [232, 238, 248, 255];
const TEXT_MUTED: [u8; 4] = [142, 156, 180, 255];
const ACCENT_CYAN: [u8; 4] = [56, 189, 248, 255];
const ACCENT_BLUE: [u8; 4] = [37, 99, 235, 255];
const ACCENT_PURPLE: [u8; 4] = [124, 58, 237, 255];
const ACCENT_GREEN: [u8; 4] = [16, 185, 129, 255];
const ACCENT_YELLOW: [u8; 4] = [250, 204, 21, 255];
const ACCENT_RED: [u8; 4] = [239, 68, 68, 255];
const ACCENT_PINK: [u8; 4] = [244, 114, 182, 255];

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Rect {
    fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    fn right(self) -> i32 {
        self.x + self.w
    }

    fn bottom(self) -> i32 {
        self.y + self.h
    }
}

struct Canvas {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32, fill: [u8; 4]) -> Self {
        let count = (width as usize) * (height as usize);
        let mut data = Vec::with_capacity(count * 4);
        for _ in 0..count {
            data.extend_from_slice(&fill);
        }
        Self {
            width,
            height,
            data,
        }
    }

    #[inline]
    fn blend_pixel(&mut self, x: i32, y: i32, rgba: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let a = rgba[3] as u32;
        if a == 0 {
            return;
        }
        if a == 255 {
            self.data[idx..idx + 4].copy_from_slice(&rgba);
            return;
        }
        let inv = 255 - a;
        for (c, &src_ch) in rgba.iter().enumerate().take(3) {
            let dst = self.data[idx + c] as u32;
            let src = src_ch as u32;
            self.data[idx + c] = ((src * a + dst * inv + 127) / 255) as u8;
        }
        self.data[idx + 3] = 255;
    }

    fn fill_rect(&mut self, r: Rect, rgba: [u8; 4]) {
        let x0 = r.x.max(0);
        let y0 = r.y.max(0);
        let x1 = r.right().min(self.width as i32);
        let y1 = r.bottom().min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                self.blend_pixel(x, y, rgba);
            }
        }
    }

    fn stroke_rect(&mut self, r: Rect, thickness: i32, rgba: [u8; 4]) {
        if r.w <= 0 || r.h <= 0 || thickness <= 0 {
            return;
        }
        self.fill_rect(Rect::new(r.x, r.y, r.w, thickness), rgba);
        self.fill_rect(Rect::new(r.x, r.bottom() - thickness, r.w, thickness), rgba);
        self.fill_rect(Rect::new(r.x, r.y, thickness, r.h), rgba);
        self.fill_rect(Rect::new(r.right() - thickness, r.y, thickness, r.h), rgba);
    }

    fn draw_panel(&mut self, r: Rect, title: &str, scale: i32) {
        self.fill_rect(r, BG_PANEL);
        self.stroke_rect(r, 1, BORDER_SUBTLE);
        let header_h = (28 * scale).max(20).min(r.h);
        let hdr = Rect::new(r.x + 1, r.y + 1, (r.w - 2).max(0), header_h);
        self.fill_rect(hdr, BG_HEADER);
        self.fill_rect(
            Rect::new(r.x + 1, r.y + header_h, (r.w - 2).max(0), 1),
            BORDER_SUBTLE,
        );
        self.draw_text(
            r.x + 10,
            r.y + (header_h - 7 * scale) / 2,
            title,
            scale,
            TEXT_PRIMARY,
        );
    }

    fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, thickness: i32, rgba: [u8; 4]) {
        let dx = (x1 - x0) as f32;
        let dy = (y1 - y0) as f32;
        let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
        let rad = (thickness - 1).max(0);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let px = (x0 as f32 + dx * t).round() as i32;
            let py = (y0 as f32 + dy * t).round() as i32;
            for oy in -rad..=rad {
                for ox in -rad..=rad {
                    self.blend_pixel(px + ox, py + oy, rgba);
                }
            }
        }
    }

    fn draw_dashed_vline(&mut self, x: i32, y0: i32, y1: i32, rgba: [u8; 4]) {
        let mut y = y0.min(y1);
        let end = y0.max(y1);
        while y < end {
            for dy in 0..4 {
                if y + dy < end {
                    self.blend_pixel(x, y + dy, rgba);
                }
            }
            y += 8;
        }
    }

    fn draw_dashed_hline(&mut self, x0: i32, x1: i32, y: i32, rgba: [u8; 4]) {
        let mut x = x0.min(x1);
        let end = x0.max(x1);
        while x < end {
            for dx in 0..4 {
                if x + dx < end {
                    self.blend_pixel(x + dx, y, rgba);
                }
            }
            x += 8;
        }
    }

    fn draw_diamond(&mut self, cx: i32, cy: i32, radius: i32, rgba: [u8; 4]) {
        for dy in -radius..=radius {
            let span = radius - dy.abs();
            for dx in -span..=span {
                self.blend_pixel(cx + dx, cy + dy, rgba);
            }
        }
    }

    fn draw_down_triangle(&mut self, cx: i32, top_y: i32, half_w: i32, h: i32, rgba: [u8; 4]) {
        let h = h.max(1);
        for row in 0..h {
            let span = ((h - 1 - row) * half_w) / h;
            for dx in -span..=span {
                self.blend_pixel(cx + dx, top_y + row, rgba);
            }
        }
    }

    fn blit_frame_scaled(&mut self, dst: Rect, frame: &Rgba8Frame) {
        if dst.w <= 0 || dst.h <= 0 || frame.width == 0 || frame.height == 0 {
            return;
        }
        let scale_x = dst.w as f64 / frame.width as f64;
        let scale_y = dst.h as f64 / frame.height as f64;
        let fit_scale = scale_x.min(scale_y);
        let fit_w = ((frame.width as f64 * fit_scale).round() as i32).max(1);
        let fit_h = ((frame.height as f64 * fit_scale).round() as i32).max(1);
        let ox = dst.x + (dst.w - fit_w) / 2;
        let oy = dst.y + (dst.h - fit_h) / 2;

        // Subtle dark letterbox + accent frame border around embedded viewport.
        self.fill_rect(dst, [8, 10, 14, 255]);
        let fit_rect = Rect::new(ox, oy, fit_w, fit_h);

        for py in 0..fit_h {
            let sy = (((py as f64 + 0.5) / fit_h as f64) * frame.height as f64)
                .floor()
                .clamp(0.0, (frame.height - 1) as f64) as u32;
            for px in 0..fit_w {
                let sx = (((px as f64 + 0.5) / fit_w as f64) * frame.width as f64)
                    .floor()
                    .clamp(0.0, (frame.width - 1) as f64) as u32;
                if let Some([r, g, b, a]) = frame.get_pixel(sx, sy) {
                    let rgba = match frame.alpha_mode {
                        AlphaMode::Straight => [r, g, b, a],
                        AlphaMode::Premultiplied => {
                            if a == 0 {
                                [0, 0, 0, 0]
                            } else if a == 255 {
                                [r, g, b, 255]
                            } else {
                                let unpremul = |c: u8| -> u8 {
                                    (((c as u32) * 255) / (a as u32)).min(255) as u8
                                };
                                [unpremul(r), unpremul(g), unpremul(b), a]
                            }
                        }
                    };
                    self.blend_pixel(ox + px, oy + py, rgba);
                }
            }
        }
        self.stroke_rect(fit_rect, 2, BORDER_ACCENT);
    }

    fn draw_text(&mut self, x: i32, y: i32, text: &str, scale: i32, rgba: [u8; 4]) {
        let scale = scale.max(1);
        let advance = 6 * scale;
        let mut cursor_x = x;
        for ch in text.chars() {
            let glyph = glyph_5x7(ch);
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..5 {
                    if (bits >> (4 - col)) & 1 == 1 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.blend_pixel(
                                    cursor_x + col * scale + sx,
                                    y + (row as i32) * scale + sy,
                                    rgba,
                                );
                            }
                        }
                    }
                }
            }
            cursor_x += advance;
        }
    }

    fn into_frame(self, pts: Flicks) -> Rgba8Frame {
        Rgba8Frame::new(self.width, self.height, pts, AlphaMode::Straight, self.data)
            .expect("canvas dimensions and byte buffer length are always synchronized")
    }
}

/// Visual coloring style for real PCM audio waveforms drawn in desktop snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SnapshotWaveformStyle {
    #[default]
    DualEnvelope,
    SpectralRgb,
}

/// Pre-summarized per-column waveform bucket for [`SnapshotAudioClipWaveform`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SnapshotWaveformColumn {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
    pub band_energy: [f32; 3],
}

/// Real PCM audio waveform payload associated with a single timeline [`AudioClipDef`].
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotAudioClipWaveform {
    pub clip_id: String,
    pub effective_start: Flicks,
    pub effective_end: Flicks,
    /// Optional pre-sampled columns across the clip width (e.g. from `WaveformPyramid`).
    pub columns: Vec<SnapshotWaveformColumn>,
    /// Optional decoded or synthetic PCM buffer sampled on-demand when `columns` is empty.
    pub buffer: Option<AudioBuffer>,
    /// Optional per-column linear gain envelope (`fade_in`, `fade_out`, `gain_db`).
    pub gain_envelope: Vec<f32>,
    pub style: SnapshotWaveformStyle,
}

impl SnapshotAudioClipWaveform {
    /// Creates a clip waveform payload from pre-sampled columns.
    pub fn from_columns(
        clip_id: impl Into<String>,
        effective_start: Flicks,
        effective_end: Flicks,
        columns: Vec<SnapshotWaveformColumn>,
    ) -> Self {
        Self {
            clip_id: clip_id.into(),
            effective_start,
            effective_end,
            columns,
            buffer: None,
            gain_envelope: Vec::new(),
            style: SnapshotWaveformStyle::DualEnvelope,
        }
    }

    /// Creates a clip waveform payload backed directly by an [`AudioBuffer`].
    pub fn from_audio_buffer(
        clip_id: impl Into<String>,
        effective_start: Flicks,
        effective_end: Flicks,
        buffer: AudioBuffer,
    ) -> Self {
        Self {
            clip_id: clip_id.into(),
            effective_start,
            effective_end,
            columns: Vec::new(),
            buffer: Some(buffer),
            gain_envelope: Vec::new(),
            style: SnapshotWaveformStyle::DualEnvelope,
        }
    }
}

/// Optional decoded audio context passed to [`render_desktop_ui_snapshot_with_audio`] so the
/// A1 audio lane renders real PCM waveforms and beat/downbeat markers without running FFmpeg
/// inside `frameshift-app`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapshotAudioContext {
    pub clip_waveforms: HashMap<String, SnapshotAudioClipWaveform>,
    pub detected_beats: Vec<Flicks>,
    pub downbeat_ticks: Vec<Flicks>,
}

/// Renders a deterministic, dark-mode desktop editor UI snapshot (`1920x1080` default)
/// displaying the project's media pool, creative decisions & preferences, scaled live
/// `wgpu` viewport embed, render DAG inspector with `SafeExpr` bindings, multi-track
/// timeline (`video` + `audio` + beat markers + `event_id` anchor + red playhead), and
/// inlaid speed-curve velocity graph.
pub fn render_desktop_ui_snapshot(
    controller: &WorkspaceUiController,
    viewport_frame: &Rgba8Frame,
    width: u32,
    height: u32,
) -> Rgba8Frame {
    render_desktop_ui_snapshot_with_audio(controller, viewport_frame, width, height, None)
}

/// Renders the deterministic desktop editor UI snapshot with an optional [`SnapshotAudioContext`]
/// for real PCM waveform & beat-grid visualization in the `A1 AUDIO` lane.
pub fn render_desktop_ui_snapshot_with_audio(
    controller: &WorkspaceUiController,
    viewport_frame: &Rgba8Frame,
    width: u32,
    height: u32,
    audio: Option<&SnapshotAudioContext>,
) -> Rgba8Frame {
    let w = width.max(640) as i32;
    let h = height.max(360) as i32;
    let project = controller.project();
    let mut canvas = Canvas::new(w as u32, h as u32, BG_CANVAS);

    let text_scale = if w >= 1600 { 2 } else { 1 };
    let small_scale = 1;

    // 1. Top Header Bar
    let top_h = (h * 52 / 1080).max(38);
    let top_bar = Rect::new(0, 0, w, top_h);
    canvas.fill_rect(top_bar, BG_HEADER);
    canvas.fill_rect(Rect::new(0, top_h - 1, w, 1), BORDER_SUBTLE);

    let title = format!("FRAMESHIFT STUDIO // REV #{}", project.revision_id);
    let title_x = 16;
    let title_y = (top_h - 7 * text_scale) / 2;
    let char_w = 6 * text_scale;
    let title_right = title_x + (title.chars().count() as i32) * char_w;
    canvas.draw_text(title_x, title_y, &title, text_scale, ACCENT_CYAN);

    let hw_str = match project.hw_accel_policy {
        HwAccelPolicy::Auto => "AUTO",
        HwAccelPolicy::D3d11va => "D3D11VA",
        HwAccelPolicy::SoftwareFallback => "SW",
    };
    let profile_str = match controller.active_profile {
        RenderProfileKind::Draft => "DRAFT",
        RenderProfileKind::Final => "FINAL",
    };
    let proxy_str = if project.proxy_config.enabled {
        format!(
            "PROXY {}/{}",
            project.proxy_config.scale_num, project.proxy_config.scale_den
        )
    } else {
        "PROXY OFF".to_owned()
    };
    let badge_text = format!("{profile_str} | HW:{hw_str} | {proxy_str}");
    let badge_w = (badge_text.len() as i32) * 6 * small_scale + 20;
    let badge_rect = Rect::new(w - badge_w - 14, (top_h - 24) / 2, badge_w, 24);
    canvas.fill_rect(badge_rect, BG_PANEL);
    canvas.stroke_rect(badge_rect, 1, BORDER_ACCENT);
    canvas.draw_text(
        badge_rect.x + 10,
        badge_rect.y + 8,
        &badge_text,
        small_scale,
        ACCENT_CYAN,
    );

    let tc = format_timecode(controller.playhead_tick, project.timeline.fps);
    let raw_tc_label = format!("TC {}  ({} FLICKS)", tc, controller.playhead_tick.0);
    let raw_tc_w = (raw_tc_label.chars().count() as i32) * char_w;
    let tc_x = (badge_rect.x - 18 - raw_tc_w).max(title_right + 16);
    let max_tc_chars = ((badge_rect.x - 12 - tc_x).max(0) / char_w) as usize;
    let tc_label = truncate_str(&raw_tc_label, max_tc_chars);
    canvas.draw_text(tc_x, title_y, &tc_label, text_scale, ACCENT_YELLOW);

    let proj_x = title_right + 20;
    let max_proj_total_chars = ((tc_x - 16 - proj_x).max(0) / char_w) as usize;
    let fps_suffix = format!("  |  {:.2} FPS", project.timeline.fps.as_f64());
    let suffix_chars = fps_suffix.chars().count();
    let proj_label = if max_proj_total_chars > suffix_chars + 3 {
        let max_name_chars = max_proj_total_chars - suffix_chars;
        format!(
            "{}{}",
            truncate_str(&project.name, max_name_chars),
            fps_suffix
        )
    } else if max_proj_total_chars >= 4 {
        truncate_str(&project.name, max_proj_total_chars)
    } else {
        String::new()
    };
    if !proj_label.is_empty() {
        canvas.draw_text(proj_x, title_y, &proj_label, text_scale, TEXT_PRIMARY);
    }

    // Compute main row and bottom timeline geometry
    let pad = 8;
    let mid_top = top_h + pad;
    let mid_h = (h * 588 / 1080).max(160);
    let bottom_top = mid_top + mid_h + pad;
    let bottom_h = (h - bottom_top - pad).max(120);

    let left_w = (w * 384 / 1920).max(200);
    let right_w = (w * 480 / 1920).max(240);
    let center_x = pad + left_w + pad;
    let center_w = (w - center_x - right_w - pad * 2).max(200);
    let right_x = center_x + center_w + pad;

    // 2. Left Panel: Media Pool + Creative Decisions & Preferences + Activity Feed
    let left_rect = Rect::new(pad, mid_top, left_w, mid_h);
    canvas.draw_panel(left_rect, "MEDIA POOL & DECISIONS", text_scale);
    let mut ly = left_rect.y + 38;
    canvas.draw_text(left_rect.x + 12, ly, "ASSETS", small_scale, ACCENT_CYAN);
    ly += 14;
    for asset in project.assets.iter().take(6) {
        let tag = match asset.kind {
            AssetKind::Video => "[VID]",
            AssetKind::Audio => "[AUD]",
            AssetKind::Image => "[IMG]",
        };
        let meta = if let Some(res) = asset.resolution {
            format!("{}x{}", res.width, res.height)
        } else if let Some(hz) = asset.sample_rate_hz {
            format!("{hz}Hz")
        } else {
            "MEDIA".to_owned()
        };
        let line = format!("{tag} {} ({meta})", truncate_str(&asset.id, 24));
        let row_r = Rect::new(left_rect.x + 10, ly, left_rect.w - 20, 18);
        canvas.fill_rect(row_r, BG_SUBSURFACE);
        canvas.draw_text(row_r.x + 6, row_r.y + 5, &line, small_scale, TEXT_PRIMARY);
        ly += 22;
    }

    ly += 8;
    canvas.draw_text(
        left_rect.x + 12,
        ly,
        "CREATIVE DECISIONS",
        small_scale,
        ACCENT_YELLOW,
    );
    ly += 14;
    if project.creative_decisions.is_empty() {
        canvas.draw_text(
            left_rect.x + 12,
            ly,
            "(none recorded)",
            small_scale,
            TEXT_MUTED,
        );
        ly += 16;
    } else {
        for dec in project.creative_decisions.iter().take(3) {
            let hdr = format!("* {}", truncate_str(&dec.decision_id, 42));
            canvas.draw_text(left_rect.x + 12, ly, &hdr, small_scale, TEXT_PRIMARY);
            ly += 12;
            let app = format!("  {}", truncate_str(&dec.approach, 50));
            canvas.draw_text(left_rect.x + 12, ly, &app, small_scale, TEXT_MUTED);
            ly += 14;
        }
    }

    ly += 8;
    canvas.draw_text(
        left_rect.x + 12,
        ly,
        "PREFERENCES & HISTORY",
        small_scale,
        ACCENT_GREEN,
    );
    ly += 14;
    for pref in project.preferences.iter().take(3) {
        let scope = match pref.scope {
            PreferenceScope::Global => "G",
            PreferenceScope::Project => "P",
            PreferenceScope::Scene => "S",
        };
        let line = format!(
            "[{scope}] {} = {}",
            truncate_str(&pref.key, 20),
            truncate_str(&pref.value, 24)
        );
        canvas.draw_text(left_rect.x + 12, ly, &line, small_scale, TEXT_PRIMARY);
        ly += 13;
    }
    let undo_line = format!(
        "UNDO: {}  |  REDO: {}  |  LOG: {}",
        controller.session().undo_len(),
        controller.session().redo_len(),
        controller.activity_log.len()
    );
    if ly + 18 < left_rect.bottom() {
        canvas.draw_text(
            left_rect.x + 12,
            ly + 4,
            &undo_line,
            small_scale,
            ACCENT_CYAN,
        );
        ly += 20;
    }
    for entry in controller.activity_log.iter().rev().take(4) {
        if ly + 14 >= left_rect.bottom() {
            break;
        }
        let log_line = format!(
            "#{} {}",
            entry.revision_id,
            truncate_str(&entry.summary, 46)
        );
        canvas.draw_text(left_rect.x + 12, ly, &log_line, small_scale, TEXT_MUTED);
        ly += 13;
    }

    // 3. Center Panel: Scaled Live Viewport Embed
    let center_rect = Rect::new(center_x, mid_top, center_w, mid_h);
    let vp_title = format!(
        "LIVE WGPU VIEWPORT ({}x{})",
        viewport_frame.width, viewport_frame.height
    );
    canvas.draw_panel(center_rect, &vp_title, text_scale);
    let vp_area = Rect::new(
        center_rect.x + 14,
        center_rect.y + 42,
        center_rect.w - 28,
        center_rect.h - 56,
    );
    canvas.blit_frame_scaled(vp_area, viewport_frame);
    let overlay_txt = format!("PLAYHEAD {}", tc);
    let overlay_box = Rect::new(
        vp_area.x + 10,
        vp_area.y + 10,
        (overlay_txt.len() as i32) * 6 + 16,
        20,
    );
    canvas.fill_rect(overlay_box, [12, 14, 20, 210]);
    canvas.stroke_rect(overlay_box, 1, ACCENT_RED);
    canvas.draw_text(
        overlay_box.x + 8,
        overlay_box.y + 6,
        &overlay_txt,
        1,
        TEXT_PRIMARY,
    );

    // 4. Right Panel: Node DAG Inspector & ParamBindingDef / SafeExpr list
    let right_rect = Rect::new(right_x, mid_top, right_w, mid_h);
    canvas.draw_panel(right_rect, "NODE DAG INSPECTOR", text_scale);
    let right_header_h = (28 * text_scale).max(20).min(right_rect.h);
    let mut ry = right_rect.y + right_header_h + 8;

    let total_nodes = project.graph.nodes.len();
    let max_visible_nodes = 30usize;
    let (node_start, node_end) = if total_nodes <= max_visible_nodes {
        (0, total_nodes)
    } else {
        let sel_idx = controller
            .selected_node
            .as_ref()
            .and_then(|id| project.graph.nodes.iter().position(|n| &n.id == id))
            .unwrap_or(0);
        let start = sel_idx
            .saturating_sub(max_visible_nodes / 2)
            .min(total_nodes - max_visible_nodes);
        (start, start + max_visible_nodes)
    };
    let shown_nodes = node_end - node_start;
    let nodes_header = if shown_nodes < total_nodes {
        format!("NODES ({shown_nodes}/{total_nodes})")
    } else {
        format!("RENDER GRAPH NODES ({total_nodes})")
    };
    canvas.draw_text(
        right_rect.x + 12,
        ry,
        &nodes_header,
        small_scale,
        ACCENT_CYAN,
    );
    ry += 14;

    let two_col = total_nodes > 12;
    let cols = if two_col { 2 } else { 1 };
    let col_gap = 6;
    let inner_w = (right_rect.w - 20).max(40);
    let col_w = if two_col {
        (inner_w - col_gap) / 2
    } else {
        inner_w
    };
    let row_stride = 18;
    let box_h = 16;
    let max_node_chars = ((col_w - 10) / 6).max(8) as usize;
    let min_footer_reserve = 76;
    let max_rows_fit =
        ((right_rect.bottom() - min_footer_reserve - ry).max(0) / row_stride) as usize;
    let total_rows = shown_nodes.div_ceil(cols);
    let visible_rows = total_rows.min(max_rows_fit);

    let mut node_boxes: HashMap<&str, (Rect, usize)> = HashMap::with_capacity(shown_nodes);
    let visible_slice = &project.graph.nodes[node_start..node_end];
    for (local_idx, node) in visible_slice.iter().enumerate() {
        let row = local_idx / cols;
        if row >= visible_rows {
            break;
        }
        let col = local_idx % cols;
        let cell_x = right_rect.x + 10 + (col as i32) * (col_w + col_gap);
        let cell_y = ry + (row as i32) * row_stride;
        let is_selected = controller.selected_node.as_ref() == Some(&node.id);
        let is_output = project.graph.output_node == node.id;
        let row_r = Rect::new(cell_x, cell_y, col_w, box_h);
        node_boxes.insert(node.id.0.as_str(), (row_r, col));
        canvas.fill_rect(
            row_r,
            if is_selected {
                [30, 58, 95, 255]
            } else {
                BG_SUBSURFACE
            },
        );
        canvas.fill_rect(
            Rect::new(row_r.x + 1, row_r.y + 1, 2, (row_r.h - 2).max(1)),
            node_kind_accent(&node.kind),
        );
        if is_selected {
            canvas.stroke_rect(row_r, 1, ACCENT_CYAN);
        }
        let out_tag = if is_output { " [OUT]" } else { "" };
        let has_dp = matches!(
            &node.kind,
            NodeKind::Scene3D {
                depth_plate: Some(_),
                ..
            }
        );
        let in_count = node.inputs.len() + usize::from(has_dp);
        let line = if two_col {
            truncate_str(
                &format!("{}:{}{}", node.id.0, node_kind_label(&node.kind), out_tag),
                max_node_chars,
            )
        } else {
            let dp_tag = if has_dp { "+dp" } else { "" };
            truncate_str(
                &format!(
                    "{} : {}{} (in:{}{})",
                    node.id.0,
                    node_kind_label(&node.kind),
                    out_tag,
                    in_count,
                    dp_tag
                ),
                max_node_chars,
            )
        };
        canvas.draw_text(row_r.x + 5, row_r.y + 4, &line, small_scale, TEXT_PRIMARY);
    }
    draw_node_dag_edges(
        &mut canvas,
        right_rect,
        col_w,
        two_col,
        visible_slice,
        &node_boxes,
    );
    ry += (visible_rows as i32) * row_stride;

    ry += 6;
    canvas.draw_text(
        right_rect.x + 12,
        ry,
        "SELECTED NODE PARAMS",
        small_scale,
        ACCENT_YELLOW,
    );
    ry += 14;
    let max_panel_chars = ((right_rect.w - 24) / 6).max(12) as usize;
    if let Some(sel) = controller
        .selected_node
        .as_ref()
        .and_then(|id| project.graph.node(id))
    {
        let summary = describe_node_params(sel);
        canvas.draw_text(
            right_rect.x + 12,
            ry,
            &truncate_str(&summary, max_panel_chars),
            small_scale,
            TEXT_PRIMARY,
        );
        ry += 16;
    } else {
        canvas.draw_text(
            right_rect.x + 12,
            ry,
            "(no node selected)",
            small_scale,
            TEXT_MUTED,
        );
        ry += 16;
    }

    ry += 6;
    canvas.draw_text(
        right_rect.x + 12,
        ry,
        "SAFEEXPR PARAM BINDINGS",
        small_scale,
        ACCENT_PINK,
    );
    ry += 14;
    let bindings_bottom = right_rect.bottom() - 16;
    if project.param_bindings.is_empty() {
        if ry + 10 <= bindings_bottom {
            canvas.draw_text(right_rect.x + 12, ry, "(none)", small_scale, TEXT_MUTED);
        }
    } else {
        let total_bindings = project.param_bindings.len();
        for (idx, binding) in project.param_bindings.iter().enumerate() {
            let remaining = total_bindings - idx;
            let reserve_overflow = if remaining > 1 { 14 } else { 0 };
            if ry + 14 + reserve_overflow > bindings_bottom {
                if ry + 10 <= right_rect.bottom() - 4 {
                    canvas.draw_text(
                        right_rect.x + 12,
                        ry,
                        &format!("... +{remaining} more"),
                        small_scale,
                        TEXT_MUTED,
                    );
                }
                break;
            }
            let txt = truncate_str(
                &format!(
                    "{}.{} = {}",
                    binding.node_id.0, binding.param_name, binding.expression
                ),
                max_panel_chars,
            );
            canvas.draw_text(right_rect.x + 12, ry, &txt, small_scale, TEXT_PRIMARY);
            ry += 14;
        }
    }

    // 5. Bottom Panel: Multi-track Timeline + Speed-Curve Graph
    let bottom_rect = Rect::new(pad, bottom_top, w - pad * 2, bottom_h);
    canvas.draw_panel(
        bottom_rect,
        "MULTI-TRACK TIMELINE & SPEED CURVE",
        text_scale,
    );

    let bottom_header_h = (28 * text_scale).max(20).min(bottom_rect.h);
    let label_col_w = 110;
    let track_x0 = bottom_rect.x + label_col_w;
    let track_w = (bottom_rect.w - label_col_w - 16).max(100);
    let ruler_y = bottom_rect.y + bottom_header_h + 6;
    let ruler_h = 36;
    let total_ticks = project.timeline.duration_ticks.0.max(1) as f64;

    let tick_to_x = |tick: Flicks| -> i32 {
        let ratio = (tick.0 as f64 / total_ticks).clamp(0.0, 1.0);
        track_x0 + (ratio * track_w as f64).round() as i32
    };

    // Ruler ticks (adaptive step so labels are >= 44px apart in top row)
    canvas.fill_rect(
        Rect::new(track_x0, ruler_y, track_w, ruler_h),
        BG_SUBSURFACE,
    );
    let total_secs =
        ((project.timeline.duration_ticks.0 + FLICKS_PER_SECOND - 1) / FLICKS_PER_SECOND).max(1);
    let duration_secs = (total_ticks / FLICKS_PER_SECOND as f64).max(1e-6);
    let px_per_sec = (track_w as f64) / duration_secs;
    let sec_step: i64 = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600]
        .into_iter()
        .find(|&step| (step as f64) * px_per_sec >= 44.0)
        .unwrap_or_else(|| ((44.0 / px_per_sec.max(1e-6)).ceil() as i64).max(1));

    let mut last_tick_right = track_x0 - 8;
    let mut s = 0_i64;
    while s <= total_secs {
        let tick = Flicks((s * FLICKS_PER_SECOND).min(project.timeline.duration_ticks.0));
        let tx = tick_to_x(tick);
        canvas.draw_line(tx, ruler_y + 12, tx, ruler_y + ruler_h, 1, BORDER_SUBTLE);
        let label = format!("{s}s");
        let lw = (label.len() as i32) * 6;
        let lx = (tx + 3).min(track_x0 + track_w - lw - 2);
        if lx >= last_tick_right + 8 && lx >= track_x0 {
            canvas.draw_text(lx, ruler_y + 3, &label, 1, TEXT_MUTED);
            last_tick_right = lx + lw;
        }
        s += sec_step;
    }

    // Track lane layout
    let tracks_y = ruler_y + ruler_h + 6;
    let tracks_avail = (bottom_rect.bottom() - tracks_y - 10).max(80);
    let v_lane_h = (tracks_avail * 22 / 100).max(22);
    let a_lane_h = (tracks_avail * 24 / 100).max(24);
    let speed_lane_h = (tracks_avail - v_lane_h * 2 - a_lane_h - 16).max(36);

    let v1_rect = Rect::new(track_x0, tracks_y, track_w, v_lane_h);
    let v2_rect = Rect::new(track_x0, tracks_y + v_lane_h + 4, track_w, v_lane_h);
    let a1_rect = Rect::new(track_x0, v2_rect.bottom() + 4, track_w, a_lane_h);
    let curve_rect = Rect::new(track_x0, a1_rect.bottom() + 6, track_w, speed_lane_h);

    canvas.fill_rect(v1_rect, BG_SUBSURFACE);
    canvas.fill_rect(v2_rect, BG_SUBSURFACE);
    canvas.fill_rect(a1_rect, BG_SUBSURFACE);
    canvas.fill_rect(curve_rect, [11, 14, 21, 255]);
    canvas.stroke_rect(curve_rect, 1, BORDER_SUBTLE);

    canvas.draw_text(
        bottom_rect.x + 12,
        v1_rect.y + 6,
        "V1 VIDEO",
        1,
        ACCENT_CYAN,
    );
    canvas.draw_text(
        bottom_rect.x + 12,
        v2_rect.y + 6,
        "V2 VIDEO",
        1,
        ACCENT_PURPLE,
    );
    canvas.draw_text(
        bottom_rect.x + 12,
        a1_rect.y + 8,
        "A1 AUDIO",
        1,
        ACCENT_GREEN,
    );
    canvas.draw_text(
        bottom_rect.x + 12,
        curve_rect.y + 8,
        "SPEED (x)",
        1,
        ACCENT_PINK,
    );

    // Draw video clips across V1/V2 staggered lanes so overlapping clips are both visible
    for (idx, clip) in project.timeline.clips.iter().enumerate() {
        let lane = if idx % 2 == 0 { v1_rect } else { v2_rect };
        let cx0 = tick_to_x(clip.timeline_start);
        let cx1 = tick_to_x(clip.timeline_end).max(cx0 + 4);
        let clip_r = Rect::new(cx0, lane.y + 2, cx1 - cx0, lane.h - 4);
        let fill = if idx % 2 == 0 {
            ACCENT_BLUE
        } else {
            ACCENT_PURPLE
        };
        canvas.fill_rect(clip_r, fill);
        let is_sel = controller.selected_clip.as_deref() == Some(clip.id.as_str());
        canvas.stroke_rect(
            clip_r,
            if is_sel { 2 } else { 1 },
            if is_sel { ACCENT_CYAN } else { BORDER_SUBTLE },
        );
        let label = format!(
            "{} [{:.1}s]",
            clip.id,
            (clip.timeline_end - clip.timeline_start).to_seconds_f64()
        );
        canvas.draw_text(
            clip_r.x + 6,
            clip_r.y + (clip_r.h - 7) / 2,
            &label,
            1,
            TEXT_PRIMARY,
        );
    }

    // Draw audio clips: real PCM waveform when `SnapshotAudioContext` is supplied,
    // falling back to deterministic procedural bars when media context is absent.
    for ac in &project.timeline.audio_clips {
        let clip_wf = audio.and_then(|ctx| ctx.clip_waveforms.get(&ac.id));
        let (eff_start, eff_end) = if let Some(wf) = clip_wf {
            (wf.effective_start, wf.effective_end)
        } else {
            resolve_snapshot_audio_clip_bounds(project, ac)
        };
        let ax0 = tick_to_x(eff_start);
        let ax1 = tick_to_x(eff_end).max(ax0 + 4);
        let ar = Rect::new(ax0, a1_rect.y + 2, ax1 - ax0, a1_rect.h - 4);
        canvas.fill_rect(ar, [14, 58, 48, 255]);
        canvas.stroke_rect(ar, 1, ACCENT_GREEN);

        if let Some(wf) = clip_wf {
            let wf_rect = Rect::new(ar.x + 1, ar.y + 1, (ar.w - 2).max(1), (ar.h - 2).max(1));
            let cols = resolve_snapshot_clip_columns(ac, wf, wf_rect.w as usize);
            if !cols.is_empty() {
                paint_snapshot_waveform_columns(&mut canvas, wf_rect, &cols, wf.style);
                let label_w = ((ac.id.chars().count() as i32) * 6 + 8).min((ar.w - 4).max(0));
                if label_w > 6 {
                    canvas.fill_rect(Rect::new(ar.x + 3, ar.y + 2, label_w, 11), [12, 16, 24, 195]);
                }
            } else {
                draw_procedural_waveform_bars(&mut canvas, ar);
            }
        } else {
            draw_procedural_waveform_bars(&mut canvas, ar);
        }
        canvas.draw_text(ar.x + 6, ar.y + 4, &ac.id, 1, TEXT_PRIMARY);
    }

    // Beat & downbeat grid markers across A1 (real detected beats when present, 0.5s fallback otherwise)
    if let Some(ctx) = audio
        && (!ctx.detected_beats.is_empty() || !ctx.downbeat_ticks.is_empty())
    {
        for &bt in &ctx.detected_beats {
            if bt >= Flicks::ZERO && bt <= project.timeline.duration_ticks {
                let bx = tick_to_x(bt);
                canvas.draw_line(bx, a1_rect.y, bx, a1_rect.bottom(), 1, [110, 231, 183, 150]);
            }
        }
        for &dbt in &ctx.downbeat_ticks {
            if dbt >= Flicks::ZERO && dbt <= project.timeline.duration_ticks {
                let bx = tick_to_x(dbt);
                canvas.draw_line(bx, a1_rect.y, bx, a1_rect.bottom(), 2, [56, 189, 248, 210]);
            }
        }
    } else {
        let half_sec = FLICKS_PER_SECOND / 2;
        let mut bt = half_sec;
        while bt < project.timeline.duration_ticks.0 {
            let bx = tick_to_x(Flicks(bt));
            canvas.draw_line(bx, a1_rect.y, bx, a1_rect.bottom(), 1, [110, 231, 183, 140]);
            bt += half_sec;
        }
    }

    // Draw inlaid SpeedCurve plot for selected clip (or first clip)
    if let Some(curve_clip) = controller
        .selected_clip
        .as_deref()
        .and_then(|id| project.clip(id))
        .or_else(|| project.timeline.clips.first())
    {
        let max_speed = 3.0_f64;
        let speed_to_y = |spd: f64| -> i32 {
            let norm = (spd / max_speed).clamp(0.0, 1.0);
            curve_rect.bottom() - 8 - ((norm * (curve_rect.h - 16) as f64).round() as i32)
        };
        let one_x_y = speed_to_y(1.0);
        canvas.draw_dashed_hline(
            curve_rect.x + 2,
            curve_rect.right() - 2,
            one_x_y,
            BORDER_SUBTLE,
        );
        canvas.draw_text(curve_rect.x + 6, one_x_y - 9, "1.0x", 1, TEXT_MUTED);

        let gx0 = tick_to_x(curve_clip.timeline_start);
        let gx1 = tick_to_x(curve_clip.timeline_end).max(gx0 + 2);
        let clip_span = (curve_clip.timeline_end - curve_clip.timeline_start)
            .0
            .max(1);
        let samples = ((gx1 - gx0) / 3).max(24);
        let mut prev_pt: Option<(i32, i32)> = None;
        for i in 0..=samples {
            let t = i as f64 / samples as f64;
            let offset = Flicks((clip_span as f64 * t).round() as i64);
            let spd = curve_clip.speed_curve.instant_speed_at(offset);
            let px = gx0 + (((gx1 - gx0) as f64) * t).round() as i32;
            let py = speed_to_y(spd);
            if let Some((lx, ly)) = prev_pt {
                canvas.draw_line(lx, ly, px, py, 2, ACCENT_PINK);
            }
            prev_pt = Some((px, py));
        }
        canvas.draw_text(
            curve_rect.x + 48,
            curve_rect.y + 6,
            &format!("CURVE: {}", curve_clip.id),
            1,
            ACCENT_PINK,
        );
    }

    // Event anchor markers: yellow diamond + guide line + 2-lane staggered event_id labels
    let mut sorted_events: Vec<(i32, &str)> = project
        .events
        .iter()
        .map(|ev| (tick_to_x(ev.target_output_tick), ev.event_id.as_str()))
        .collect();
    sorted_events.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));

    let mut lane_right_x = [track_x0 - 4; 2];
    let lane_y = [ruler_y + 18, ruler_y + 27];
    let right_limit = track_x0 + track_w - 2;

    for (ex, event_id) in sorted_events {
        canvas.draw_dashed_vline(ex, ruler_y + 14, curve_rect.bottom(), [250, 204, 21, 160]);
        canvas.draw_diamond(ex, ruler_y + 26, 5, ACCENT_YELLOW);

        let full_w = (event_id.chars().count() as i32) * 6;
        let (lx, label_text) = if ex + 8 + full_w <= right_limit {
            (ex + 8, event_id.to_owned())
        } else if ex - 8 - full_w >= track_x0 {
            (ex - 8 - full_w, event_id.to_owned())
        } else {
            let avail_right = (right_limit - (ex + 8)).max(0);
            let avail_left = (ex - 8 - track_x0).max(0);
            let txt = truncate_str(event_id, (avail_right.max(avail_left) / 6) as usize);
            let w_px = (txt.chars().count() as i32) * 6;
            if avail_right >= avail_left {
                (ex + 8, txt)
            } else {
                ((ex - 8 - w_px).max(track_x0), txt)
            }
        };
        let lw = (label_text.chars().count() as i32) * 6;
        if lw <= 0 {
            continue;
        }
        if lx >= lane_right_x[0] + 6 {
            canvas.draw_text(lx, lane_y[0], &label_text, 1, ACCENT_YELLOW);
            lane_right_x[0] = lx + lw;
        } else if lx >= lane_right_x[1] + 6 {
            canvas.draw_text(lx, lane_y[1], &label_text, 1, ACCENT_YELLOW);
            lane_right_x[1] = lx + lw;
        } else {
            let lane = usize::from(lane_right_x[1] < lane_right_x[0]);
            let shifted_lx = lane_right_x[lane] + 6;
            if shifted_lx + 18 <= right_limit && shifted_lx - ex <= 48 {
                let max_c = ((right_limit - shifted_lx) / 6) as usize;
                let txt = truncate_str(&label_text, max_c);
                let tw = (txt.chars().count() as i32) * 6;
                if tw > 0 {
                    canvas.draw_text(shifted_lx, lane_y[lane], &txt, 1, ACCENT_YELLOW);
                    lane_right_x[lane] = shifted_lx + tw;
                }
            }
        }
    }

    // Red vertical playhead line + top downward triangle handle
    let ph_x = tick_to_x(controller.playhead_tick);
    canvas.draw_line(ph_x, ruler_y, ph_x, curve_rect.bottom(), 2, ACCENT_RED);
    canvas.draw_down_triangle(ph_x, ruler_y, 7, 10, ACCENT_RED);

    canvas.into_frame(controller.playhead_tick)
}

fn format_timecode(tick: Flicks, fps: frameshift_time::Rational32) -> String {
    let clamped = tick.0.max(0);
    let total_secs = clamped / FLICKS_PER_SECOND;
    let rem_ticks = clamped % FLICKS_PER_SECOND;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let fps_val = fps.as_f64().max(1.0);
    let frame = (((rem_ticks as f64) / (FLICKS_PER_SECOND as f64)) * fps_val).floor() as u32;
    format!("{mins:02}:{secs:02}:{frame:02}")
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_owned()
    } else if max_chars <= 3 {
        s.chars().take(max_chars).collect()
    } else {
        let mut out: String = s.chars().take(max_chars - 3).collect();
        out.push_str("...");
        out
    }
}

/// Returns a concise display label for a [`NodeKind`] in UI inspectors and snapshots.
pub fn node_kind_label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::SourceVideo { .. } => "SourceVideo",
        NodeKind::SourceImage { .. } => "SourceImage",
        NodeKind::Solid { .. } => "Solid",
        NodeKind::Transform2D { .. } => "Transform2D",
        NodeKind::MaskMatte { .. } => "MaskMatte",
        NodeKind::Blend { .. } => "Blend",
        NodeKind::ColorBasic { .. } => "ColorBasic",
        NodeKind::ShaderTransition { .. } => "ShaderTransition",
        NodeKind::MotionBlurShutter { .. } => "MotionBlurShutter",
        NodeKind::VectorText { .. } => "VectorText",
        NodeKind::VectorPathHud { .. } => "VectorPathHud",
        NodeKind::Lut3D { .. } => "Lut3D",
        NodeKind::MotionTileShake { .. } => "MotionTileShake",
        NodeKind::AnimeImpactFrame { .. } => "AnimeImpactFrame",
        NodeKind::RadialShockwave { .. } => "RadialShockwave",
        NodeKind::HalftoneGlow { .. } => "HalftoneGlow",
        NodeKind::DeepGlow { .. } => "DeepGlow",
        NodeKind::AnimeSharpen { .. } => "AnimeSharpen",
        NodeKind::FilmOptics { .. } => "FilmOptics",
        NodeKind::CustomWgsl { .. } => "CustomWgsl",
        NodeKind::Transform3D { .. } => "Transform3D",
        NodeKind::Text3D { .. } => "Text3D",
        NodeKind::ParticleField3D { .. } => "ParticleField3D",
        NodeKind::RibbonTrail3D { .. } => "RibbonTrail3D",
        NodeKind::Shape3D { .. } => "Shape3D",
        NodeKind::Scene3D { .. } => "Scene3D",
        NodeKind::CornerPin { .. } => "CornerPin",
        NodeKind::GridWarp { .. } => "GridWarp",
        NodeKind::BackdropScrim { .. } => "BackdropScrim",
        NodeKind::Precomp { .. } => "Precomp",
        NodeKind::PrecompInput { .. } => "PrecompInput",
        NodeKind::AdjustmentLayer { .. } => "AdjustmentLayer",
        NodeKind::BlurMoCurves { .. } => "BlurMoCurves",
        NodeKind::Null3D { .. } => "Null3D",
        NodeKind::DepthMatte { .. } => "DepthMatte",
        NodeKind::OpticalFlowBlur { .. } => "OpticalFlowBlur",
        NodeKind::Anime4K { .. } => "Anime4K",
        NodeKind::DisplacementMap { .. } => "DisplacementMap",
        NodeKind::FrameEcho { .. } => "FrameEcho",
        NodeKind::ShapeMask { .. } => "ShapeMask",
    }
}

/// Returns the category accent RGBA color for a [`NodeKind`] in UI node graphs and snapshots.
pub fn node_kind_accent(kind: &NodeKind) -> [u8; 4] {
    match kind {
        NodeKind::SourceVideo { .. }
        | NodeKind::SourceImage { .. }
        | NodeKind::Solid { .. }
        | NodeKind::VectorText { .. }
        | NodeKind::VectorPathHud { .. }
        | NodeKind::ShapeMask { .. }
        | NodeKind::PrecompInput { .. } => ACCENT_GREEN,
        NodeKind::Transform2D { .. }
        | NodeKind::CornerPin { .. }
        | NodeKind::GridWarp { .. }
        | NodeKind::Precomp { .. } => ACCENT_BLUE,
        NodeKind::MaskMatte { .. }
        | NodeKind::Blend { .. }
        | NodeKind::AdjustmentLayer { .. }
        | NodeKind::DepthMatte { .. }
        | NodeKind::BackdropScrim { .. } => ACCENT_YELLOW,
        NodeKind::ShaderTransition { .. } => ACCENT_PINK,
        NodeKind::Transform3D { .. }
        | NodeKind::Null3D { .. }
        | NodeKind::Text3D { .. }
        | NodeKind::ParticleField3D { .. }
        | NodeKind::RibbonTrail3D { .. }
        | NodeKind::Shape3D { .. }
        | NodeKind::Scene3D { .. } => ACCENT_PURPLE,
        NodeKind::ColorBasic { .. }
        | NodeKind::Lut3D { .. }
        | NodeKind::MotionBlurShutter { .. }
        | NodeKind::MotionTileShake { .. }
        | NodeKind::AnimeImpactFrame { .. }
        | NodeKind::RadialShockwave { .. }
        | NodeKind::HalftoneGlow { .. }
        | NodeKind::DeepGlow { .. }
        | NodeKind::AnimeSharpen { .. }
        | NodeKind::FilmOptics { .. }
        | NodeKind::BlurMoCurves { .. }
        | NodeKind::OpticalFlowBlur { .. }
        | NodeKind::Anime4K { .. }
        | NodeKind::DisplacementMap { .. }
        | NodeKind::FrameEcho { .. } => ACCENT_CYAN,
        NodeKind::CustomWgsl { .. } => ACCENT_RED,
    }
}

/// Returns a display label for a [`TemplateSlotKind`] in UI/inspector surfaces.
pub fn template_slot_kind_label(kind: TemplateSlotKind) -> &'static str {
    match kind {
        TemplateSlotKind::AssetPath(AssetKind::Video) => "VideoAsset",
        TemplateSlotKind::AssetPath(AssetKind::Audio) => "AudioAsset",
        TemplateSlotKind::AssetPath(AssetKind::Image) => "ImageAsset",
        TemplateSlotKind::Text => "Text",
        TemplateSlotKind::Tick => "Tick",
        TemplateSlotKind::Number => "Number",
        TemplateSlotKind::Boolean => "Boolean",
        TemplateSlotKind::Color => "Color",
        TemplateSlotKind::Choice => "Choice",
        TemplateSlotKind::SpeedCurve => "SpeedCurve",
        TemplateSlotKind::Font => "Font",
    }
}

/// Returns the UI accent color associated with a [`TemplateSlotKind`].
pub fn template_slot_kind_accent(kind: TemplateSlotKind) -> [u8; 4] {
    match kind {
        TemplateSlotKind::AssetPath(_) => ACCENT_GREEN,
        TemplateSlotKind::Text | TemplateSlotKind::Font => ACCENT_CYAN,
        TemplateSlotKind::Tick | TemplateSlotKind::SpeedCurve => ACCENT_PINK,
        TemplateSlotKind::Number | TemplateSlotKind::Boolean => ACCENT_BLUE,
        TemplateSlotKind::Color => ACCENT_YELLOW,
        TemplateSlotKind::Choice => ACCENT_PURPLE,
    }
}

fn describe_node_params(node: &GraphNodeDef) -> String {
    match &node.kind {
        NodeKind::SourceVideo {
            asset_id, clip_id, ..
        } => {
            format!(
                "asset={asset_id} clip={}",
                clip_id.as_deref().unwrap_or("-")
            )
        }
        NodeKind::SourceImage {
            asset_id,
            fit,
            opacity,
            ..
        } => {
            format!("asset={asset_id} fit={fit:?} op={opacity:.2}")
        }
        NodeKind::Solid { color_rgba, .. } => format!(
            "rgba=[{:.2},{:.2},{:.2},{:.2}]",
            color_rgba[0], color_rgba[1], color_rgba[2], color_rgba[3]
        ),
        NodeKind::Transform2D {
            translate,
            scale,
            rotation_rad,
            ..
        } => format!(
            "t=[{:.2},{:.2}] s=[{:.2},{:.2}] rot={:.2}",
            translate[0], translate[1], scale[0], scale[1], rotation_rad
        ),
        NodeKind::MaskMatte {
            mode,
            invert,
            feather,
            ..
        } => {
            format!("mode={mode:?} invert={invert} feather={feather:.2}")
        }
        NodeKind::Blend { mode, opacity } => format!("mode={mode:?} opacity={opacity:.2}"),
        NodeKind::ColorBasic {
            exposure,
            contrast,
            saturation,
            flash_amount,
        } => format!(
            "exp={exposure:.2} con={contrast:.2} sat={saturation:.2} flash={flash_amount:.2}"
        ),
        NodeKind::ShaderTransition {
            shader_name,
            progress,
            whip_strength,
            chromatic_aberration,
            ..
        } => format!(
            "{shader_name} p={progress:.2} whip={whip_strength:.2} ca={chromatic_aberration:.3}"
        ),
        NodeKind::MotionBlurShutter {
            shutter_angle_deg,
            sub_samples,
            phase,
            ..
        } => format!("shutter={shutter_angle_deg:.0}deg samples={sub_samples} {phase:?}"),
        NodeKind::VectorText {
            text,
            font_size_norm,
            ..
        } => {
            format!("\"{}\" size={font_size_norm:.3}", truncate_str(text, 24))
        }
        NodeKind::Lut3D {
            lut_preset,
            intensity,
            ..
        } => {
            format!("lut={lut_preset} intensity={intensity:.2}")
        }
        NodeKind::MotionTileShake {
            tile_scale,
            shake_freq_hz,
            event_delta_sec,
            ..
        } => {
            format!("tile={tile_scale:.2} freq={shake_freq_hz:.1}Hz dt={event_delta_sec:.2}s")
        }
        NodeKind::AnimeImpactFrame {
            luma_threshold,
            invert_amount,
            mix_amount,
            ..
        } => {
            format!("thresh={luma_threshold:.2} inv={invert_amount:.2} mix={mix_amount:.2}")
        }
        NodeKind::RadialShockwave {
            radius,
            refraction_amp,
            zoom_blur_strength,
            ..
        } => {
            format!("r={radius:.2} refr={refraction_amp:.3} zoom={zoom_blur_strength:.2}")
        }
        NodeKind::HalftoneGlow {
            dot_scale,
            halftone_mix,
            glow_intensity,
            ..
        } => {
            format!("dots={dot_scale:.0} ht={halftone_mix:.2} glow={glow_intensity:.2}")
        }
        NodeKind::DeepGlow {
            threshold,
            intensity,
            mip_levels,
            ..
        } => {
            format!("thresh={threshold:.2} int={intensity:.2} mips={mip_levels}")
        }
        NodeKind::AnimeSharpen {
            line_strength,
            sharpen_amount,
            anti_ringing,
        } => {
            format!("line={line_strength:.2} sharp={sharpen_amount:.2} ar={anti_ringing:.2}")
        }
        NodeKind::FilmOptics {
            chromatic_edge,
            grain_amount,
            halation_amount,
            vignette_amount,
            ..
        } => {
            format!("ca={chromatic_edge:.3} grain={grain_amount:.2} hal={halation_amount:.2} vig={vignette_amount:.2}")
        }
        NodeKind::CustomWgsl {
            expected_inputs, ..
        } => {
            format!("inputs={expected_inputs}")
        }
        NodeKind::Transform3D {
            pos,
            rot_deg,
            fov_deg,
            ..
        } => {
            format!(
                "pos=[{:.2},{:.2},{:.2}] rot=[{:.0},{:.0},{:.0}] fov={fov_deg:.0}",
                pos[0], pos[1], pos[2], rot_deg[0], rot_deg[1], rot_deg[2]
            )
        }
        NodeKind::Text3D {
            text,
            font_size,
            extrude_depth,
            ..
        } => {
            format!(
                "\"{}\" size={font_size:.2} depth={extrude_depth:.2}",
                truncate_str(text, 20)
            )
        }
        NodeKind::ParticleField3D {
            count,
            shape,
            speed,
            ..
        } => {
            format!("n={count} shape={shape} speed={speed:.2}")
        }
        NodeKind::RibbonTrail3D {
            ribbon_count,
            trim_start,
            trim_end,
            width,
            core_intensity,
            halo_intensity,
            saturation,
            min_width_px,
            core_whiten,
            normal_blend,
            ..
        } => {
            format!(
                "n={ribbon_count} trim=[{trim_start:.2},{trim_end:.2}] w={width:.3} \
                 core={core_intensity:.2} halo={halo_intensity:.2} sat={saturation:.2} \
                 minw={min_width_px:.1}px whiten={core_whiten:.2} blend={}",
                if *normal_blend { "normal" } else { "additive" }
            )
        }
        NodeKind::Shape3D {
            shape_kind,
            size,
            extrude_depth,
            ..
        } => {
            format!("{shape_kind:?} size={size:.2} depth={extrude_depth:.2}")
        }
        NodeKind::Scene3D {
            camera,
            depth_plate,
            ..
        } => {
            let dp_str = match depth_plate {
                Some(dp) => format!(" dp={}({:.2}..{:.2})", dp.node_id.0, dp.near_z, dp.far_z),
                None => String::new(),
            };
            format!(
                "mode={:?} pos=[{:.2},{:.2},{:.2}] fov={:.0} dof={:.2}{dp_str}",
                camera.mode,
                camera.pos[0],
                camera.pos[1],
                camera.pos[2],
                camera.fov_deg,
                camera.dof_blur_strength
            )
        }
        NodeKind::VectorPathHud {
            points_norm,
            bezier_segments,
            trim_start,
            trim_end,
            data_label,
            data_value,
            ..
        } => {
            format!(
                "pts={} bez={} trim=[{trim_start:.2},{trim_end:.2}] {}{:.1}",
                points_norm.len(),
                bezier_segments.len(),
                data_label,
                data_value
            )
        }
        NodeKind::CornerPin {
            tl,
            tr,
            br,
            bl,
            opacity,
            ..
        } => {
            format!(
                "tl=[{:.2},{:.2}] tr=[{:.2},{:.2}] br=[{:.2},{:.2}] bl=[{:.2},{:.2}] op={opacity:.2}",
                tl[0], tl[1], tr[0], tr[1], br[0], br[1], bl[0], bl[1]
            )
        }
        NodeKind::GridWarp {
            cols,
            rows,
            mix_amount,
            opacity,
            ..
        } => {
            format!("grid={cols}x{rows} mix={mix_amount:.2} op={opacity:.2}")
        }
        NodeKind::BackdropScrim {
            rect_center_norm,
            rect_size_norm,
            blur_radius_norm,
            opacity,
            auto_contrast,
            ..
        } => {
            format!(
                "rect=[{:.2},{:.2}] size=[{:.2},{:.2}] blur={:.3} op={opacity:.2} ac={auto_contrast:.2}",
                rect_center_norm[0],
                rect_center_norm[1],
                rect_size_norm[0],
                rect_size_norm[1],
                blur_radius_norm
            )
        }
        NodeKind::Precomp {
            sub_graph,
            time_offset_ticks,
            time_scale,
            opacity,
        } => {
            format!(
                "nodes={} off={time_offset_ticks} scale={time_scale:.2} op={opacity:.2}",
                sub_graph.nodes.len()
            )
        }
        NodeKind::PrecompInput { slot } => {
            format!("slot={slot}")
        }
        NodeKind::AdjustmentLayer {
            start_tick,
            end_tick,
            opacity,
            blend_mode,
        } => {
            format!(
                "[{}..{}] mode={blend_mode:?} op={opacity:.2}",
                start_tick.0, end_tick.0
            )
        }
        NodeKind::BlurMoCurves {
            shift_x,
            shift_y,
            z_dist,
            rotate_deg,
            blur_strength,
            samples,
            wrap_mode,
        } => {
            format!(
                "s=[{shift_x:.2},{shift_y:.2}] z={z_dist:.2} rot={rotate_deg:.1} str={blur_strength:.2} n={samples} w={wrap_mode}"
            )
        }
        NodeKind::Null3D {
            pos,
            rot_deg,
            scale,
            parent,
            ..
        } => {
            format!(
                "pos=[{:.2},{:.2},{:.2}] rot=[{:.0},{:.0},{:.0}] s=[{:.2},{:.2},{:.2}] p={}",
                pos[0],
                pos[1],
                pos[2],
                rot_deg[0],
                rot_deg[1],
                rot_deg[2],
                scale[0],
                scale[1],
                scale[2],
                parent.as_ref().map_or("-", |id| id.0.as_str())
            )
        }
        NodeKind::DepthMatte {
            near_depth,
            far_depth,
            feather,
            invert,
        } => {
            format!("near={near_depth:.2} far={far_depth:.2} feather={feather:.2} inv={invert}")
        }
        NodeKind::OpticalFlowBlur {
            shutter_amount,
            samples,
            max_blur_px,
            sensitivity,
            visualize_flow,
            ..
        } => {
            format!(
                "shutter={shutter_amount:.2} n={samples} max={max_blur_px:.0}px sens={sensitivity:.2} vis={visualize_flow}"
            )
        }
        NodeKind::Anime4K {
            preset,
            strength,
            clamp_highlights,
        } => {
            format!("preset={preset:?} str={strength:.2} clamp={clamp_highlights}")
        }
        NodeKind::DisplacementMap {
            channel_mode,
            scale_x,
            scale_y,
            opacity,
            ..
        } => {
            format!("mode={channel_mode:?} s=[{scale_x:.2},{scale_y:.2}] op={opacity:.2}")
        }
        NodeKind::FrameEcho {
            echo_count,
            starting_intensity,
            decay,
            operator,
            ..
        } => {
            format!("n={echo_count} int={starting_intensity:.2} decay={decay:.2} op={operator:?}")
        }
        NodeKind::ShapeMask {
            vertices,
            feather_px,
            invert,
            opacity,
            ..
        } => {
            format!("pts={} feather={feather_px:.1} inv={invert} op={opacity:.2}", vertices.len())
        }
    }
}

/// Compact 5x7 ASCII bitmap font (uppercase-folded for lowercase letters).
fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '=' => [
            0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '|' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '#' => [
            0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b00000,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        '&' => [
            0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
        '*' => [
            0b00000, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0b00000,
        ],
        '"' => [
            0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '\'' => [
            0b00100, 0b00100, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        ' ' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        _ => [
            0b11111, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
    }
}

fn node_upstream_edges(node: &GraphNodeDef) -> Vec<(&str, bool)> {
    let mut edges: Vec<(&str, bool)> = node
        .inputs
        .iter()
        .map(|id| (id.0.as_str(), false))
        .collect();
    if let NodeKind::Scene3D {
        depth_plate: Some(dp),
        ..
    } = &node.kind
    {
        edges.push((dp.node_id.0.as_str(), true));
    }
    edges
}

fn draw_node_dag_edges(
    canvas: &mut Canvas,
    right_rect: Rect,
    col_w: i32,
    two_col: bool,
    visible_nodes: &[GraphNodeDef],
    node_boxes: &HashMap<&str, (Rect, usize)>,
) {
    let mut std_lane = 0_i32;
    let mut dp_lane = 0_i32;
    for node in visible_nodes {
        let Some(&(dst_rect, dst_col)) = node_boxes.get(node.id.0.as_str()) else {
            continue;
        };
        let dst_y = dst_rect.y + dst_rect.h / 2;

        for (up_id, is_dp) in node_upstream_edges(node) {
            let Some(&(src_rect, src_col)) = node_boxes.get(up_id) else {
                continue;
            };
            let src_y = src_rect.y + src_rect.h / 2;
            let color = if is_dp {
                ACCENT_YELLOW
            } else {
                [56, 189, 248, 125]
            };

            if !two_col {
                if is_dp {
                    let gutter_x = right_rect.right() - 4 - (dp_lane % 2) * 2;
                    dp_lane += 1;
                    canvas.draw_line(src_rect.right(), src_y, gutter_x, src_y, 1, color);
                    canvas.draw_dashed_vline(gutter_x, src_y, dst_y, color);
                    canvas.draw_line(gutter_x, dst_y, dst_rect.right(), dst_y, 1, color);
                    canvas.fill_rect(Rect::new(dst_rect.right() - 1, dst_y - 1, 3, 3), color);
                } else {
                    let gutter_x = right_rect.x + 3 + (std_lane % 3) * 2;
                    std_lane += 1;
                    canvas.draw_line(src_rect.x, src_y, gutter_x, src_y, 1, color);
                    canvas.draw_line(gutter_x, src_y, gutter_x, dst_y, 1, color);
                    canvas.draw_line(gutter_x, dst_y, dst_rect.x, dst_y, 1, color);
                }
            } else if src_col == dst_col {
                let (gutter_x, sx, dx) = if src_col == 0 {
                    let gx = right_rect.x + 3 + (std_lane % 3) * 2;
                    std_lane += 1;
                    (gx, src_rect.x, dst_rect.x)
                } else {
                    let gx = right_rect.right() - 4 - (dp_lane % 3) * 2;
                    dp_lane += 1;
                    (gx, src_rect.right(), dst_rect.right())
                };
                canvas.draw_line(sx, src_y, gutter_x, src_y, 1, color);
                if is_dp {
                    canvas.draw_dashed_vline(gutter_x, src_y, dst_y, color);
                    canvas.fill_rect(Rect::new(dx - 1, dst_y - 1, 3, 3), color);
                } else {
                    canvas.draw_line(gutter_x, src_y, gutter_x, dst_y, 1, color);
                }
                canvas.draw_line(gutter_x, dst_y, dx, dst_y, 1, color);
            } else {
                // Cross-column edge routed strictly inside the 6px inter-column gap.
                let gap_x = right_rect.x + 10 + col_w + 1 + ((std_lane + dp_lane) % 4);
                if is_dp {
                    dp_lane += 1;
                } else {
                    std_lane += 1;
                }
                let sx = if src_col == 0 {
                    src_rect.right()
                } else {
                    src_rect.x
                };
                let dx = if dst_col == 0 {
                    dst_rect.right()
                } else {
                    dst_rect.x
                };
                canvas.draw_line(sx, src_y, gap_x, src_y, 1, color);
                if is_dp {
                    canvas.draw_dashed_vline(gap_x, src_y, dst_y, color);
                    canvas.fill_rect(Rect::new(dx - 1, dst_y - 1, 3, 3), color);
                } else {
                    canvas.draw_line(gap_x, src_y, gap_x, dst_y, 1, color);
                }
                canvas.draw_line(gap_x, dst_y, dx, dst_y, 1, color);
            }
        }
    }
}

fn resolve_snapshot_audio_clip_bounds(project: &Project, ac: &AudioClipDef) -> (Flicks, Flicks) {
    let duration = ac.timeline_end - ac.timeline_start;
    if let Some(ref event_id) = ac.anchor_event_id
        && let Some(ev) = project.event(event_id)
    {
        let apex = ac.apex_offset_ticks.unwrap_or(Flicks::ZERO);
        let start = Flicks(ev.target_output_tick.0.saturating_sub(apex.0));
        return (start, Flicks(start.0.saturating_add(duration.0)));
    }
    (ac.timeline_start, ac.timeline_end)
}

fn draw_procedural_waveform_bars(canvas: &mut Canvas, ar: Rect) {
    let mid_y = ar.y + ar.h / 2;
    let max_half = (ar.h / 2 - 3).max(2);
    let mut bx = ar.x + 3;
    let mut step = 0_u32;
    while bx < ar.right() - 2 {
        let phase = (step as f64 * 0.45).sin().abs();
        let beat_boost = if step.is_multiple_of(8) {
            1.0
        } else {
            0.45 + 0.45 * phase
        };
        let bar_h = ((max_half as f64) * beat_boost).round() as i32;
        canvas.fill_rect(Rect::new(bx, mid_y - bar_h, 2, bar_h * 2 + 1), ACCENT_GREEN);
        bx += 4;
        step += 1;
    }
}

fn resolve_snapshot_clip_columns(
    ac: &AudioClipDef,
    wf: &SnapshotAudioClipWaveform,
    target_cols: usize,
) -> Vec<SnapshotWaveformColumn> {
    if target_cols == 0 {
        return Vec::new();
    }
    let mut cols = if !wf.columns.is_empty() {
        resample_snapshot_columns(&wf.columns, target_cols)
    } else if let Some(ref buf) = wf.buffer {
        sample_audio_buffer_columns(buf, ac, wf, target_cols)
    } else {
        return Vec::new();
    };

    // Apply explicit gain envelope or clip gain/fade envelope when sampling raw buffer.
    if !wf.gain_envelope.is_empty() {
        for (i, col) in cols.iter_mut().enumerate() {
            let g_idx = (i * wf.gain_envelope.len()) / target_cols;
            let g = wf.gain_envelope[g_idx.min(wf.gain_envelope.len() - 1)].max(0.0);
            col.min = (col.min * g).clamp(-1.0, 1.0);
            col.max = (col.max * g).clamp(-1.0, 1.0);
            col.rms = (col.rms * g).clamp(0.0, 1.0);
        }
    } else if wf.columns.is_empty() {
        let base_gain = if ac.gain_db.is_finite() {
            10.0_f32.powf(ac.gain_db / 20.0).clamp(0.0, 4.0)
        } else {
            1.0
        };
        let dur_ticks = (wf.effective_end - wf.effective_start).0.max(1) as f64;
        for (i, col) in cols.iter_mut().enumerate() {
            let elapsed = ((i as f64 + 0.5) / target_cols as f64) * dur_ticks;
            let remaining = dur_ticks - elapsed;
            let mut g = base_gain;
            if let Some(fi) = ac.fade_in {
                g *= fi.gain_at_elapsed(elapsed) as f32;
            }
            if let Some(fo) = ac.fade_out {
                g *= fo.gain_at_elapsed(remaining) as f32;
            }
            col.min = (col.min * g).clamp(-1.0, 1.0);
            col.max = (col.max * g).clamp(-1.0, 1.0);
            col.rms = (col.rms * g).clamp(0.0, 1.0);
        }
    }
    cols
}

fn merge_snapshot_columns(slice: &[SnapshotWaveformColumn]) -> SnapshotWaveformColumn {
    if slice.is_empty() {
        return SnapshotWaveformColumn::default();
    }
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    let mut rms_sq = 0.0f64;
    let mut b_sq = [0.0f64; 3];
    for c in slice {
        min_v = min_v.min(c.min);
        max_v = max_v.max(c.max);
        rms_sq += (c.rms as f64).powi(2);
        for (k, acc) in b_sq.iter_mut().enumerate() {
            *acc += (c.band_energy[k] as f64).powi(2);
        }
    }
    let n = slice.len() as f64;
    SnapshotWaveformColumn {
        min: if min_v.is_finite() { min_v } else { 0.0 },
        max: if max_v.is_finite() { max_v } else { 0.0 },
        rms: (rms_sq / n).sqrt() as f32,
        band_energy: [
            (b_sq[0] / n).sqrt() as f32,
            (b_sq[1] / n).sqrt() as f32,
            (b_sq[2] / n).sqrt() as f32,
        ],
    }
}

fn resample_snapshot_columns(
    src: &[SnapshotWaveformColumn],
    target_cols: usize,
) -> Vec<SnapshotWaveformColumn> {
    if src.len() == target_cols {
        return src.to_vec();
    }
    let mut out = Vec::with_capacity(target_cols);
    for i in 0..target_cols {
        let c0 = (i * src.len()) / target_cols;
        let c1 = (((i + 1) * src.len()) / target_cols)
            .max(c0 + 1)
            .min(src.len());
        out.push(merge_snapshot_columns(&src[c0..c1]));
    }
    out
}

fn sample_audio_buffer_columns(
    buf: &AudioBuffer,
    ac: &AudioClipDef,
    wf: &SnapshotAudioClipWaveform,
    target_cols: usize,
) -> Vec<SnapshotWaveformColumn> {
    let total_frames = buf.frame_count();
    if total_frames == 0 || target_cols == 0 {
        return Vec::new();
    }
    let ch_count = buf.channels as usize;
    let clip_dur = Flicks((wf.effective_end - wf.effective_start).0.max(1));
    let start_tick = Flicks(ac.source_start.0.max(0));
    let end_tick = Flicks(start_tick.0.saturating_add(clip_dur.0));
    let sf = ticks_to_audio_samples_nearest(start_tick, buf.sample_rate)
        .unwrap_or(0)
        .max(0) as usize;
    let ef = ticks_to_audio_samples_nearest(end_tick, buf.sample_rate)
        .unwrap_or(total_frames as i64)
        .max(0) as usize;
    let sf = sf.min(total_frames.saturating_sub(1));
    let ef = ef.clamp(sf + 1, total_frames);
    let span = ef - sf;

    let mut out = Vec::with_capacity(target_cols);
    for i in 0..target_cols {
        let f0 = sf + (i * span) / target_cols;
        let f1 = (sf + ((i + 1) * span) / target_cols).max(f0 + 1).min(ef);
        let mut min_v = f32::INFINITY;
        let mut max_v = f32::NEG_INFINITY;
        let mut sum_sq = 0.0f64;
        for f in f0..f1 {
            let base = f * ch_count;
            let mut mono = 0.0f32;
            for ch in 0..ch_count {
                let s = buf.samples[base + ch];
                if s.is_finite() {
                    mono += s;
                }
            }
            mono /= ch_count as f32;
            min_v = min_v.min(mono);
            max_v = max_v.max(mono);
            sum_sq += (mono as f64) * (mono as f64);
        }
        let n = (f1 - f0).max(1) as f64;
        out.push(SnapshotWaveformColumn {
            min: if min_v.is_finite() { min_v } else { 0.0 },
            max: if max_v.is_finite() { max_v } else { 0.0 },
            rms: (sum_sq / n).sqrt() as f32,
            band_energy: [0.0; 3],
        });
    }
    out
}

fn paint_snapshot_waveform_columns(
    canvas: &mut Canvas,
    rect: Rect,
    columns: &[SnapshotWaveformColumn],
    style: SnapshotWaveformStyle,
) {
    let mid_y = rect.y as f32 + (rect.h as f32) * 0.5;
    let half_h = ((rect.h as f32) * 0.5 - 2.0).max(1.0);
    let mut prev_range: Option<(f32, f32)> = None;

    for (idx, col) in columns.iter().enumerate() {
        let px = rect.x + idx as i32;
        if px >= rect.right() {
            break;
        }
        let s_min = col.min.clamp(-1.0, 1.0);
        let s_max = col.max.clamp(-1.0, 1.0);
        let s_rms = col.rms.abs().clamp(0.0, 1.0);

        let (peak_rgba, rms_rgba) = match style {
            SnapshotWaveformStyle::DualEnvelope => ([16, 185, 129, 130], [110, 231, 183, 235]),
            SnapshotWaveformStyle::SpectralRgb => spectral_snapshot_colors(col.band_energy),
        };

        let mut y_top = mid_y - s_max * half_h;
        let mut y_bot = mid_y - s_min * half_h;
        if y_top > y_bot {
            std::mem::swap(&mut y_top, &mut y_bot);
        }
        if y_bot - y_top < 1.0 {
            let c = 0.5 * (y_top + y_bot);
            y_top = c - 0.5;
            y_bot = c + 0.5;
        }
        if let Some((pt, pb)) = prev_range {
            if y_top > pb {
                y_top = pb;
            } else if y_bot < pt {
                y_bot = pt;
            }
        }
        prev_range = Some((y_top, y_bot));

        y_top = y_top.clamp(rect.y as f32, rect.bottom() as f32);
        y_bot = y_bot.clamp(rect.y as f32, rect.bottom() as f32);
        paint_aa_span(canvas, px, rect, y_top, y_bot, peak_rgba);

        if s_rms > 1e-5 {
            let y_rt = (mid_y - s_rms * half_h).max(y_top).clamp(rect.y as f32, rect.bottom() as f32);
            let y_rb = (mid_y + s_rms * half_h).min(y_bot).clamp(rect.y as f32, rect.bottom() as f32);
            if y_rb > y_rt {
                paint_aa_span(canvas, px, rect, y_rt, y_rb, rms_rgba);
            }
        }
    }
}

fn paint_aa_span(canvas: &mut Canvas, px: i32, rect: Rect, y_top: f32, y_bot: f32, rgba: [u8; 4]) {
    let py0 = (y_top.floor() as i32).max(rect.y);
    let py1 = (y_bot.ceil() as i32).min(rect.bottom() - 1);
    for py in py0..=py1 {
        let cov = (y_bot.min(py as f32 + 1.0) - y_top.max(py as f32)).clamp(0.0, 1.0);
        if cov > 0.001 {
            let a = ((rgba[3] as f32) * cov).round().clamp(0.0, 255.0) as u8;
            if a > 0 {
                canvas.blend_pixel(px, py, [rgba[0], rgba[1], rgba[2], a]);
            }
        }
    }
}

fn spectral_snapshot_colors(band_energy: [f32; 3]) -> ([u8; 4], [u8; 4]) {
    let max_e = band_energy[0].max(band_energy[1]).max(band_energy[2]);
    if max_e <= 1e-7 {
        return ([16, 185, 129, 130], [110, 231, 183, 235]);
    }
    let w_lo = (band_energy[0].max(0.0) / max_e).clamp(0.0, 1.0).powf(0.65);
    let w_mid = (band_energy[1].max(0.0) / max_e).clamp(0.0, 1.0).powf(0.65);
    let w_hi = (band_energy[2].max(0.0) / max_e).clamp(0.0, 1.0).powf(0.65);
    let sum = (w_lo + w_mid + w_hi).max(1e-6);
    let r = ((244.0 * w_lo + 34.0 * w_mid + 56.0 * w_hi) / sum).clamp(0.0, 255.0);
    let g = ((63.0 * w_lo + 197.0 * w_mid + 189.0 * w_hi) / sum).clamp(0.0, 255.0);
    let b = ((94.0 * w_lo + 94.0 * w_mid + 248.0 * w_hi) / sum).clamp(0.0, 255.0);
    (
        [r.round() as u8, g.round() as u8, b.round() as u8, 120],
        [
            (r + (255.0 - r) * 0.25).round() as u8,
            (g + (255.0 - g) * 0.25).round() as u8,
            (b + (255.0 - b) * 0.25).round() as u8,
            235,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_schema::{EventAnchorDef, EventOrigin, NodeId, ParamBindingDef, Project};

    #[test]
    fn snapshot_handles_long_project_25_nodes_12_bindings_and_dense_events() {
        let mut project = Project::golden_scene_preset();
        project.name = "ASTA // ANTI-MAGIC — BLACK CLOVER AMV 60FPS MASTER CUT".to_owned();
        project.timeline.duration_ticks = Flicks(24 * FLICKS_PER_SECOND);

        // Expand graph to 25 nodes
        let base_count = project.graph.nodes.len();
        for i in base_count..25 {
            project.graph.nodes.push(GraphNodeDef {
                id: NodeId(format!("fx_node_{i:02}")),
                kind: NodeKind::ColorBasic {
                    exposure: 0.1,
                    contrast: 1.05,
                    saturation: 1.1,
                    flash_amount: 0.0,
                },
                inputs: vec![project.graph.nodes[i - 1].id.clone()],
                enabled: true,
            });
        }

        // Expand param bindings to 12 (targeting the added ColorBasic nodes)
        let base_bindings = project.param_bindings.len();
        for i in base_bindings..12 {
            let target_idx = base_count + (i - base_bindings);
            project.param_bindings.push(ParamBindingDef {
                node_id: project.graph.nodes[target_idx].id.clone(),
                param_name: "flash_amount".to_owned(),
                expression: "audio_rms * 0.8".to_owned(),
            });
        }

        // Dense adjacent events
        for (idx, name) in ["kick_1", "kick_2", "kick_3", "snare_1", "chorus_end"]
            .into_iter()
            .enumerate()
        {
            project.events.push(EventAnchorDef {
                event_id: name.to_owned(),
                label: name.to_owned(),
                source_clip_id: None,
                source_anchor_tick: None,
                target_output_tick: Flicks((1 + idx as i64) * (FLICKS_PER_SECOND / 2)),
                origin: EventOrigin::Detected,
                timing: frameshift_schema::EventTimingConstraint::Absolute,
            });
        }

        let mut ctrl = WorkspaceUiController::from_project(project).unwrap();
        ctrl.scrub_to(Flicks(2 * FLICKS_PER_SECOND));
        let vp = Rgba8Frame::filled(
            320,
            180,
            ctrl.playhead_tick,
            [20, 40, 80, 255],
            AlphaMode::Straight,
        )
        .unwrap();

        let snap_1080 = render_desktop_ui_snapshot(&ctrl, &vp, 1920, 1080);
        assert_eq!(snap_1080.width, 1920);
        assert_eq!(snap_1080.height, 1080);

        // Verify 2-column node grid renders in both left (x ~ 1445..1655) and right (x ~ 1680..1895) columns
        let mut left_col_subsurface = 0usize;
        let mut right_col_subsurface = 0usize;
        for y in 138..360 {
            if snap_1080.get_pixel(1500, y) == Some(BG_SUBSURFACE) {
                left_col_subsurface += 1;
            }
            if snap_1080.get_pixel(1750, y) == Some(BG_SUBSURFACE) {
                right_col_subsurface += 1;
            }
        }
        assert!(left_col_subsurface > 100, "expected left node column boxes");
        assert!(
            right_col_subsurface > 100,
            "expected right node column boxes"
        );

        // Verify top header bar retains clean BG_HEADER gap between title (ends at 328) and proj_label (starts at 348)
        let header_gap_left = (0..50)
            .filter(|&dy| snap_1080.get_pixel(338, dy) == Some(BG_HEADER))
            .count();
        assert!(header_gap_left > 40, "expected clean gap after title");

        // Narrow window must also render cleanly without panicking
        let snap_640 = render_desktop_ui_snapshot(&ctrl, &vp, 640, 360);
        assert_eq!(snap_640.width, 640);
        assert_eq!(snap_640.height, 360);
    }

    #[test]
    fn node_and_template_slot_kind_metadata_cover_new_variants() {
        use frameshift_schema::{BlendMode, TemplateSlotKind};

        let adj = GraphNodeDef {
            id: NodeId("adj".into()),
            kind: NodeKind::AdjustmentLayer {
                start_tick: Flicks(0),
                end_tick: Flicks(FLICKS_PER_SECOND),
                opacity: 0.75,
                blend_mode: BlendMode::Overlay,
            },
            inputs: vec![NodeId("motion_blur_out".into())],
            enabled: true,
        };
        let blur = GraphNodeDef {
            id: NodeId("mocurves".into()),
            kind: NodeKind::BlurMoCurves {
                shift_x: 0.05,
                shift_y: -0.02,
                z_dist: 0.1,
                rotate_deg: 5.0,
                blur_strength: 1.0,
                samples: 16,
                wrap_mode: 2,
            },
            inputs: vec![NodeId("adj".into())],
            enabled: true,
        };
        let null = GraphNodeDef {
            id: NodeId("cam_rig".into()),
            kind: NodeKind::Null3D {
                pos: [0.0, 0.2, 0.0],
                rot_deg: [0.0, 15.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                pivot: [0.0, 0.0, 0.0],
                parent: None,
            },
            inputs: vec![],
            enabled: true,
        };
        let depth = GraphNodeDef {
            id: NodeId("dmatte".into()),
            kind: NodeKind::DepthMatte {
                near_depth: 0.1,
                far_depth: 0.9,
                feather: 0.05,
                invert: true,
            },
            inputs: vec![NodeId("mocurves".into()), NodeId("mocurves".into())],
            enabled: true,
        };

        assert_eq!(node_kind_label(&adj.kind), "AdjustmentLayer");
        assert_eq!(node_kind_label(&blur.kind), "BlurMoCurves");
        assert_eq!(node_kind_label(&null.kind), "Null3D");
        assert_eq!(node_kind_label(&depth.kind), "DepthMatte");

        assert_eq!(node_kind_accent(&adj.kind), ACCENT_YELLOW);
        assert_eq!(node_kind_accent(&blur.kind), ACCENT_CYAN);
        assert_eq!(node_kind_accent(&null.kind), ACCENT_PURPLE);
        assert_eq!(node_kind_accent(&depth.kind), ACCENT_YELLOW);

        assert!(describe_node_params(&adj).contains("Overlay"));
        assert!(describe_node_params(&blur).contains("n=16"));
        assert!(describe_node_params(&null).contains("rot=[0,15,0]"));
        assert!(describe_node_params(&depth).contains("inv=true"));

        for (kind, expected_label) in [
            (TemplateSlotKind::AssetPath(AssetKind::Video), "VideoAsset"),
            (TemplateSlotKind::AssetPath(AssetKind::Audio), "AudioAsset"),
            (TemplateSlotKind::AssetPath(AssetKind::Image), "ImageAsset"),
            (TemplateSlotKind::Text, "Text"),
            (TemplateSlotKind::Tick, "Tick"),
            (TemplateSlotKind::Number, "Number"),
            (TemplateSlotKind::Boolean, "Boolean"),
            (TemplateSlotKind::Color, "Color"),
            (TemplateSlotKind::Choice, "Choice"),
            (TemplateSlotKind::SpeedCurve, "SpeedCurve"),
            (TemplateSlotKind::Font, "Font"),
        ] {
            assert_eq!(template_slot_kind_label(kind), expected_label);
            assert_eq!(template_slot_kind_accent(kind)[3], 255);
        }

        let mut project = Project::golden_scene_preset();
        project.graph.nodes.extend([adj, blur, null, depth]);
        project.graph.output_node = NodeId("dmatte".into());
        let ctrl = WorkspaceUiController::from_project(project).unwrap();
        let vp = Rgba8Frame::filled(64, 36, Flicks::ZERO, [10, 20, 30, 255], AlphaMode::Straight)
            .unwrap();
        let snap = render_desktop_ui_snapshot(&ctrl, &vp, 1280, 720);
        assert_eq!(snap.width, 1280);
        assert_eq!(snap.height, 720);
    }

    #[test]
    fn snapshot_with_audio_renders_real_pcm_waveform_and_scene3d_depth_plate_edge() {
        use frameshift_media::SyntheticMediaGenerator;
        use frameshift_schema::{Camera3DDef, SceneDepthPlateDef};

        let mut project = Project::golden_scene_preset();
        let depth_src = GraphNodeDef {
            id: NodeId("depth_src".into()),
            kind: NodeKind::Solid {
                color_rgba: [0.5, 0.5, 0.5, 1.0],
                color_space: frameshift_schema::ColorSpace::LinearRec709,
            },
            inputs: vec![],
            enabled: true,
        };
        let scene_node = GraphNodeDef {
            id: NodeId("scene3d_dp".into()),
            kind: NodeKind::Scene3D {
                camera: Camera3DDef::default(),
                background_rgba: [0.0; 4],
                depth_plate: Some(SceneDepthPlateDef {
                    node_id: NodeId("depth_src".into()),
                    near_z: 0.5,
                    far_z: 6.0,
                    invert: false,
                    ..SceneDepthPlateDef::default()
                }),
            },
            inputs: vec![],
            enabled: true,
        };
        let out_node = GraphNodeDef {
            id: NodeId("final_out".into()),
            kind: NodeKind::ColorBasic {
                exposure: 0.0,
                contrast: 1.0,
                saturation: 1.0,
                flash_amount: 0.0,
            },
            inputs: vec![NodeId("scene3d_dp".into())],
            enabled: true,
        };
        project.graph.nodes = vec![depth_src, scene_node, out_node];
        project.graph.output_node = NodeId("final_out".into());
        project.param_bindings.clear();
        for c in &mut project.timeline.clips {
            c.event_animations.clear();
        }

        let mut ctrl = WorkspaceUiController::from_project(project).unwrap();
        ctrl.select_node(Some(NodeId("scene3d_dp".into()))).unwrap();

        let vp = Rgba8Frame::filled(160, 90, Flicks::ZERO, [16, 24, 48, 255], AlphaMode::Straight)
            .unwrap();
        let snap_fallback = render_desktop_ui_snapshot(&ctrl, &vp, 1280, 720);

        let (beat_buf, beat_ticks) = SyntheticMediaGenerator::generate_beat_audio(
            ctrl.project().timeline.duration_ticks,
            120,
            48_000,
        );
        let mut ctx = SnapshotAudioContext {
            detected_beats: beat_ticks[1..].to_vec(),
            downbeat_ticks: vec![beat_ticks[0]],
            ..Default::default()
        };
        let bgm_id = ctrl.project().timeline.audio_clips[0].id.clone();
        let bgm_start = ctrl.project().timeline.audio_clips[0].timeline_start;
        let bgm_end = ctrl.project().timeline.audio_clips[0].timeline_end;
        ctx.clip_waveforms.insert(
            bgm_id.clone(),
            SnapshotAudioClipWaveform::from_audio_buffer(bgm_id, bgm_start, bgm_end, beat_buf),
        );

        let snap_audio_1 = render_desktop_ui_snapshot_with_audio(&ctrl, &vp, 1280, 720, Some(&ctx));
        let snap_audio_2 = render_desktop_ui_snapshot_with_audio(&ctrl, &vp, 1280, 720, Some(&ctx));
        assert_eq!(snap_audio_1.fnv1a_hash64(), snap_audio_2.fnv1a_hash64());
        assert_ne!(
            snap_fallback.fnv1a_hash64(),
            snap_audio_1.fnv1a_hash64(),
            "real PCM audio context must change A1 waveform rendering vs procedural fallback"
        );

        // Verify Scene3D depth_plate yellow connector edge was rendered in the right panel gutter.
        let mut yellow_edge_px = 0usize;
        for y in 75..160 {
            for x in 1255..1272 {
                if snap_audio_1.get_pixel(x, y) == Some(ACCENT_YELLOW) {
                    yellow_edge_px += 1;
                }
            }
        }
        assert!(
            yellow_edge_px >= 6,
            "expected Scene3D.depth_plate.node_id edge drawn in right gutter, got {yellow_edge_px} px"
        );
        let scene_ref = ctrl
            .project()
            .graph
            .node(&NodeId("scene3d_dp".into()))
            .unwrap();
        assert!(describe_node_params(scene_ref).contains("dp=depth_src"));
    }
}
