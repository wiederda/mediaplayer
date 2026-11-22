//! Selbst gezeichnete Symbole und Schaltflächen.
//!
//! Die Symbole sind Vektorgrafiken und damit unabhängig von der installierten
//! Schriftart. Früher verwendete Sonderzeichen (⏮, ⛶, ⤨ …) fehlen in vielen
//! Schriften und wurden als leere Kästchen dargestellt – das passiert hier
//! nicht mehr.

use eframe::egui::{
    self, Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    #[allow(dead_code)]
    Pause,
    Previous,
    Next,
    Shuffle,
    Repeat,
    Volume,
    Fullscreen,
    Playlist,
    Plus,
    Folder,
    Music,
}

/// Runde Schaltfläche mit gezeichnetem Symbol und Tooltip.
pub struct IconButton {
    icon: Icon,
    size: Vec2,
    primary: bool,
    enabled: bool,
    tooltip: &'static str,
}

impl IconButton {
    pub fn new(icon: Icon, tooltip: &'static str) -> Self {
        Self {
            icon,
            size: Vec2::splat(46.0),
            primary: false,
            enabled: true,
            tooltip,
        }
    }

    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    /// Hervorgehobene Schaltfläche in der Akzentfarbe (z. B. Wiedergabe).
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl egui::Widget for IconButton {
    fn ui(self, ui: &mut Ui) -> Response {
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(self.size, sense);

        if ui.is_rect_visible(rect) {
            let hovered = self.enabled && response.hovered();
            let pressed = self.enabled && response.is_pointer_button_down_on();
            let t = ui
                .ctx()
                .animate_bool_with_time(response.id, hovered || pressed, 0.10);

            let corner = CornerRadius::same((rect.width().min(rect.height()) * 0.30) as u8);

            let (base_fill, hover_fill, border, foreground) = if !self.enabled && self.primary {
                (
                    theme::ACCENT.gamma_multiply(0.40),
                    theme::ACCENT.gamma_multiply(0.40),
                    Stroke::NONE,
                    Color32::WHITE.gamma_multiply(0.75),
                )
            } else if !self.enabled {
                (
                    theme::BG_CARD.gamma_multiply(0.55),
                    theme::BG_CARD.gamma_multiply(0.55),
                    Stroke::new(1.0_f32, theme::BORDER.gamma_multiply(0.55)),
                    theme::TEXT_MUTED,
                )
            } else if self.primary {
                (
                    theme::ACCENT,
                    theme::ACCENT_HOVER,
                    Stroke::NONE,
                    Color32::WHITE,
                )
            } else {
                (
                    theme::BG_CARD,
                    theme::BG_HOVER,
                    Stroke::new(1.0_f32, theme::BORDER),
                    theme::TEXT,
                )
            };

            let fill = if pressed && self.enabled {
                hover_fill
            } else {
                base_fill.lerp_to_gamma(hover_fill, t)
            };

            ui.painter()
                .rect(rect, corner, fill, border, StrokeKind::Inside);
            let icon_rect = rect.shrink(rect.width().min(rect.height()) * 0.22);
            paint(ui.painter(), self.icon, icon_rect, foreground);
        }

        if self.enabled {
            response.on_hover_text(self.tooltip)
        } else {
            response.on_hover_text(format!("{} · noch nicht verfügbar", self.tooltip))
        }
    }
}

/// Abgerundete Schaltfläche mit Symbol und Beschriftung (z. B. „Playlist“).
pub fn pill(ui: &mut Ui, icon: Icon, label: &str, active: bool) -> Response {
    let (base, hover, border, foreground) = if active {
        (
            theme::ACCENT,
            theme::ACCENT_HOVER,
            Stroke::NONE,
            Color32::WHITE,
        )
    } else {
        (
            theme::BG_CARD,
            theme::BG_HOVER,
            Stroke::new(1.0_f32, theme::BORDER),
            theme::TEXT,
        )
    };

    let font = egui::FontId::new(15.0, egui::FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, foreground);

    let icon_size = 17.0;
    let padding = 14.0;
    let gap = 8.0;
    let height = 36.0;
    let width = padding * 2.0 + icon_size + gap + galley.size().x;

    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());

    if ui.is_rect_visible(rect) {
        let t = ui
            .ctx()
            .animate_bool_with_time(response.id, response.hovered(), 0.10);
        let fill = base.lerp_to_gamma(hover, t);

        ui.painter().rect(
            rect,
            CornerRadius::same((height * 0.5) as u8),
            fill,
            border,
            StrokeKind::Inside,
        );

        let icon_rect = Rect::from_center_size(
            pos2(rect.left() + padding + icon_size * 0.5, rect.center().y),
            Vec2::splat(icon_size),
        );
        paint(ui.painter(), icon, icon_rect, foreground);

        let text_pos = pos2(
            rect.left() + padding + icon_size + gap,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(text_pos, galley, foreground);
    }

    response
}

/// Breite Schaltfläche mit Symbol und Beschriftung (z. B. für die Playlist).
pub fn wide_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let foreground = if enabled {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    let font = egui::FontId::new(15.5, egui::FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, foreground);

    let height = 44.0;
    let width = ui.available_width();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);

    if ui.is_rect_visible(rect) {
        let t = ui
            .ctx()
            .animate_bool_with_time(response.id, enabled && response.hovered(), 0.10);
        let (base, hover, border) = if enabled {
            (
                theme::BG_CARD,
                theme::BG_HOVER,
                Stroke::new(1.0_f32, theme::BORDER),
            )
        } else {
            (
                theme::BG_CARD.gamma_multiply(0.55),
                theme::BG_CARD.gamma_multiply(0.55),
                Stroke::new(1.0_f32, theme::BORDER.gamma_multiply(0.55)),
            )
        };

        ui.painter().rect(
            rect,
            CornerRadius::same(10),
            base.lerp_to_gamma(hover, t),
            border,
            StrokeKind::Inside,
        );

        let icon_size = 18.0;
        let gap = 10.0;
        let content_width = icon_size + gap + galley.size().x;
        let left = rect.center().x - content_width * 0.5;

        let icon_rect = Rect::from_center_size(
            pos2(left + icon_size * 0.5, rect.center().y),
            Vec2::splat(icon_size),
        );
        paint(ui.painter(), icon, icon_rect, foreground);

        ui.painter().galley(
            pos2(
                left + icon_size + gap,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            foreground,
        );
    }

    response
}

/// Kleines, nicht interaktives Etikett (z. B. „AUDIO · VIDEO“).
pub fn badge(ui: &mut Ui, text: &str) {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        1.0,
        egui::TextFormat {
            font_id: egui::FontId::new(11.5, egui::FontFamily::Proportional),
            color: theme::TEXT_MUTED,
            extra_letter_spacing: 1.0,
            ..Default::default()
        },
    );
    let galley = ui.painter().layout_job(job);

    let size = vec2(galley.size().x + 16.0, galley.size().y + 7.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(6), theme::BG_CARD);
    ui.painter().galley(
        pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        theme::TEXT_MUTED,
    );
}

/// Zeichnet ein Symbol zentriert in den angegebenen Bereich.
pub fn paint(painter: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let size = rect.width().min(rect.height());
    let center = rect.center();
    let stroke = Stroke::new((size * 0.11).clamp(1.4, 4.0), color);
    let round = |radius: f32| CornerRadius::same(radius.max(1.0).round() as u8);

    let triangle = |a: Pos2, b: Pos2, c: Pos2| {
        painter.add(Shape::convex_polygon(vec![a, b, c], color, Stroke::NONE));
    };

    match icon {
        Icon::Play => triangle(
            pos2(center.x - 0.30 * size, center.y - 0.40 * size),
            pos2(center.x - 0.30 * size, center.y + 0.40 * size),
            pos2(center.x + 0.40 * size, center.y),
        ),
        Icon::Pause => {
            let bar = vec2(0.20 * size, 0.76 * size);
            painter.rect_filled(
                Rect::from_center_size(pos2(center.x - 0.21 * size, center.y), bar),
                round(0.05 * size),
                color,
            );
            painter.rect_filled(
                Rect::from_center_size(pos2(center.x + 0.21 * size, center.y), bar),
                round(0.05 * size),
                color,
            );
        }
        Icon::Previous => {
            painter.rect_filled(
                Rect::from_center_size(
                    pos2(center.x - 0.38 * size, center.y),
                    vec2(0.11 * size, 0.70 * size),
                ),
                round(0.04 * size),
                color,
            );
            triangle(
                pos2(center.x + 0.36 * size, center.y - 0.36 * size),
                pos2(center.x + 0.36 * size, center.y + 0.36 * size),
                pos2(center.x - 0.12 * size, center.y),
            );
        }
        Icon::Next => {
            painter.rect_filled(
                Rect::from_center_size(
                    pos2(center.x + 0.38 * size, center.y),
                    vec2(0.11 * size, 0.70 * size),
                ),
                round(0.04 * size),
                color,
            );
            triangle(
                pos2(center.x - 0.36 * size, center.y - 0.36 * size),
                pos2(center.x - 0.36 * size, center.y + 0.36 * size),
                pos2(center.x + 0.12 * size, center.y),
            );
        }
        Icon::Shuffle => {
            let start_top = pos2(center.x - 0.40 * size, center.y - 0.24 * size);
            let start_bottom = pos2(center.x - 0.40 * size, center.y + 0.24 * size);
            let end_bottom = pos2(center.x + 0.24 * size, center.y + 0.24 * size);
            let end_top = pos2(center.x + 0.24 * size, center.y - 0.24 * size);

            let first = cubic(
                start_top,
                pos2(center.x - 0.10 * size, center.y - 0.24 * size),
                pos2(center.x + 0.02 * size, center.y + 0.24 * size),
                end_bottom,
            );
            let second = cubic(
                start_bottom,
                pos2(center.x - 0.10 * size, center.y + 0.24 * size),
                pos2(center.x + 0.02 * size, center.y - 0.24 * size),
                end_top,
            );
            painter.add(Shape::line(first, stroke));
            painter.add(Shape::line(second, stroke));

            arrow_head(
                painter,
                pos2(center.x + 0.42 * size, center.y + 0.24 * size),
                vec2(1.0, 0.0),
                0.22 * size,
                color,
            );
            arrow_head(
                painter,
                pos2(center.x + 0.42 * size, center.y - 0.24 * size),
                vec2(1.0, 0.0),
                0.22 * size,
                color,
            );
        }
        Icon::Repeat => {
            let radius = 0.31 * size;
            let start = -1.0_f32;
            let end = 4.0_f32;

            let points: Vec<Pos2> = (0..=24)
                .map(|step| {
                    let angle = start + (end - start) * (step as f32 / 24.0);
                    pos2(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    )
                })
                .collect();
            painter.add(Shape::line(points, stroke));

            let tangent = vec2(-end.sin(), end.cos());
            let tip = pos2(center.x + radius * end.cos(), center.y + radius * end.sin())
                + tangent * 0.05 * size;
            arrow_head(painter, tip, tangent, 0.28 * size, color);
        }
        Icon::Volume => {
            let body = Rect::from_min_max(
                pos2(center.x - 0.34 * size, center.y - 0.10 * size),
                pos2(center.x - 0.14 * size, center.y + 0.10 * size),
            );
            painter.rect_filled(body, round(0.03 * size), color);

            painter.add(Shape::convex_polygon(
                vec![
                    pos2(center.x - 0.14 * size, center.y - 0.10 * size),
                    pos2(center.x + 0.02 * size, center.y - 0.28 * size),
                    pos2(center.x + 0.02 * size, center.y + 0.28 * size),
                    pos2(center.x - 0.14 * size, center.y + 0.10 * size),
                ],
                color,
                Stroke::NONE,
            ));

            let arc_center = pos2(center.x + 0.02 * size, center.y);
            for radius in [0.20_f32, 0.34] {
                let points: Vec<Pos2> = (0..=16)
                    .map(|step| {
                        let angle = -0.78 + 1.56 * (step as f32 / 16.0);
                        pos2(
                            arc_center.x + radius * size * angle.cos(),
                            arc_center.y + radius * size * angle.sin(),
                        )
                    })
                    .collect();
                painter.add(Shape::line(points, Stroke::new(stroke.width * 0.85, color)));
            }
        }
        Icon::Fullscreen => {
            let rect = Rect::from_center_size(center, Vec2::splat(0.72 * size));
            let arm = 0.24 * size;
            let (left, top, right, bottom) = (rect.left(), rect.top(), rect.right(), rect.bottom());

            for (from, to) in [
                (pos2(left, top + arm), pos2(left, top)),
                (pos2(left, top), pos2(left + arm, top)),
                (pos2(right - arm, top), pos2(right, top)),
                (pos2(right, top), pos2(right, top + arm)),
                (pos2(right, bottom - arm), pos2(right, bottom)),
                (pos2(right, bottom), pos2(right - arm, bottom)),
                (pos2(left + arm, bottom), pos2(left, bottom)),
                (pos2(left, bottom), pos2(left, bottom - arm)),
            ] {
                painter.line_segment([from, to], stroke);
            }
        }
        Icon::Playlist => {
            for row in 0..3 {
                let y = center.y + (row as f32 - 1.0) * 0.26 * size;
                painter.circle_filled(pos2(center.x - 0.36 * size, y), 0.05 * size, color);
                painter.line_segment(
                    [
                        pos2(center.x - 0.20 * size, y),
                        pos2(center.x + 0.38 * size, y),
                    ],
                    Stroke::new(stroke.width * 0.9, color),
                );
            }
        }
        Icon::Plus => {
            painter.line_segment(
                [
                    pos2(center.x - 0.30 * size, center.y),
                    pos2(center.x + 0.30 * size, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(center.x, center.y - 0.30 * size),
                    pos2(center.x, center.y + 0.30 * size),
                ],
                stroke,
            );
        }
        Icon::Folder => {
            let rect = Rect::from_center_size(
                pos2(center.x, center.y + 0.05 * size),
                vec2(0.74 * size, 0.52 * size),
            );
            painter.rect_stroke(rect, round(0.08 * size), stroke, StrokeKind::Inside);

            let tab = 0.12 * size;
            painter.line_segment(
                [
                    pos2(rect.left() + 0.06 * size, rect.top()),
                    pos2(rect.left() + 0.06 * size, rect.top() - tab),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(rect.left() + 0.06 * size, rect.top() - tab),
                    pos2(rect.left() + 0.32 * size, rect.top() - tab),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(rect.left() + 0.32 * size, rect.top() - tab),
                    pos2(rect.left() + 0.32 * size, rect.top()),
                ],
                stroke,
            );
        }
        Icon::Music => {
            painter.circle_filled(
                pos2(center.x - 0.16 * size, center.y + 0.28 * size),
                0.13 * size,
                color,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(center.x - 0.05 * size, center.y - 0.40 * size),
                    pos2(center.x + 0.03 * size, center.y + 0.28 * size),
                ),
                round(0.03 * size),
                color,
            );

            let flag = cubic(
                pos2(center.x + 0.03 * size, center.y - 0.40 * size),
                pos2(center.x + 0.24 * size, center.y - 0.34 * size),
                pos2(center.x + 0.26 * size, center.y - 0.10 * size),
                pos2(center.x + 0.12 * size, center.y + 0.02 * size),
            );
            painter.add(Shape::line(flag, stroke));
        }
    }
}

fn arrow_head(painter: &Painter, tip: Pos2, direction: Vec2, size: f32, color: Color32) {
    let direction = direction.normalized();
    let normal = vec2(-direction.y, direction.x) * size * 0.5;
    painter.add(Shape::convex_polygon(
        vec![
            tip,
            tip - direction * size + normal,
            tip - direction * size - normal,
        ],
        color,
        Stroke::NONE,
    ));
}

fn cubic(start: Pos2, control_a: Pos2, control_b: Pos2, end: Pos2) -> Vec<Pos2> {
    const STEPS: usize = 20;
    (0..=STEPS)
        .map(|step| {
            let t = step as f32 / STEPS as f32;
            let u = 1.0 - t;
            let w0 = u * u * u;
            let w1 = 3.0 * u * u * t;
            let w2 = 3.0 * u * t * t;
            let w3 = t * t * t;
            pos2(
                w0 * start.x + w1 * control_a.x + w2 * control_b.x + w3 * end.x,
                w0 * start.y + w1 * control_a.y + w2 * control_b.y + w3 * end.y,
            )
        })
        .collect()
}
