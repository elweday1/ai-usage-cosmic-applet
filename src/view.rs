// SPDX-License-Identifier: GPL-3.0-only

use crate::config::{Config, ResetDisplay, Scope, Style, Thresholds};
use crate::indicator::{level_for, Gauge, IndicatorState, Level};
use crate::usage::{format_countdown, UsageSample};
use cosmic::iced::{Alignment, Background, Border, Color, Length};
use cosmic::widget;
use cosmic::Element;

use crate::Message;

const DOT: f32 = 12.0;

fn color(level: Level) -> Color {
    match level {
        Level::Green => Color::from_rgb(0.30, 0.78, 0.36),
        Level::Amber => Color::from_rgb(0.95, 0.70, 0.10),
        Level::Red => Color::from_rgb(0.90, 0.25, 0.25),
    }
}

/// Neutral accent used by the non-color fill/ring styles.
const ACCENT: Color = Color {
    r: 0.45,
    g: 0.65,
    b: 0.95,
    a: 1.0,
};

fn hex(c: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    )
}

/// Linear per-channel blend between two colours (`t` clamped to 0..1).
fn lerp(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// The hue a fill *reaches* at fraction `value`: green at empty, blending
/// through amber at the amber threshold to red at the red threshold. This is
/// the leading-edge colour of the gradient meter — its hue alone says how close
/// the budget is to its ceiling.
fn color_at(value: f32, t: &Thresholds) -> Color {
    let v = value.clamp(0.0, 1.0);
    let (g, a, r) = (color(Level::Green), color(Level::Amber), color(Level::Red));
    if v <= t.amber {
        lerp(g, a, if t.amber > 0.0 { v / t.amber } else { 1.0 })
    } else if v < t.red {
        lerp(a, r, (v - t.amber) / (t.red - t.amber).max(1e-4))
    } else {
        r
    }
}

/// A left→right linear gradient for the *filled* portion of a meter: green at
/// the origin, passing through amber/red where those thresholds fall within the
/// fill, ending on the leading-edge hue. Offsets are relative to the fill width,
/// so the ramp always reads green→current regardless of how full the bar is.
fn gradient_fill(value: f32, t: &Thresholds, dim: bool) -> Background {
    // PI/2 radians = left→right (iced angles run clockwise from 12 o'clock).
    gradient_at(value, t, dim, std::f32::consts::PI / 2.0)
}

/// As `gradient_fill`, but along an arbitrary direction. `angle` is iced's
/// gradient angle in radians (0 = bottom→top); used for the vertical-bar style.
fn gradient_at(value: f32, t: &Thresholds, dim: bool, angle: f32) -> Background {
    use cosmic::iced::gradient::Linear;
    use cosmic::iced::Radians;
    let v = value.clamp(0.0, 1.0);
    let alpha = if dim { 0.45 } else { 1.0 };
    let fade = |c: Color| Color { a: c.a * alpha, ..c };

    let mut lin = Linear::new(Radians(angle)).add_stop(0.0, fade(color(Level::Green)));
    if v > 0.0 {
        if t.amber > 0.0 && t.amber < v {
            lin = lin.add_stop((t.amber / v).clamp(0.0, 1.0), fade(color(Level::Amber)));
        }
        if t.red < v {
            lin = lin.add_stop((t.red / v).clamp(0.0, 1.0), fade(color(Level::Red)));
        }
    }
    lin = lin.add_stop(1.0, fade(color_at(v, t)));
    Background::Gradient(lin.into())
}

/// A horizontal meter: a theme-aware track with a rounded fill. The track tint
/// derives from the active theme's on-surface colour, so it reads correctly on
/// both light and dark panels (unlike a hardcoded white alpha).
fn meter_track<'a>(
    fill: Background,
    filled: f32,
    full: f32,
    height: f32,
) -> Element<'a, Message> {
    let radius = height / 2.0;
    widget::container(
        widget::container(
            widget::Space::new()
                .width(Length::Fixed(filled))
                .height(Length::Fixed(height)),
        )
        .style(move |_t: &cosmic::Theme| widget::container::Style {
            background: Some(fill.clone()),
            border: Border { radius: radius.into(), ..Default::default() },
            ..Default::default()
        }),
    )
    .width(Length::Fixed(full))
    .style(move |theme: &cosmic::Theme| {
        let mut track: Color = theme.cosmic().on_bg_color().into();
        track.a = 0.12;
        widget::container::Style {
            background: Some(track.into()),
            border: Border { radius: radius.into(), ..Default::default() },
            ..Default::default()
        }
    })
    .into()
}

/// Time-to-reset context for the soonest-resetting budget.
#[derive(Clone, Copy)]
pub struct ResetInfo {
    /// Fraction of the budget window elapsed (0.0 just reset → 1.0 reset imminent).
    pub elapsed: f32,
    /// Seconds until that reset.
    pub remaining: i64,
}

/// Window lengths for the two budgets, used to turn "seconds remaining" into a
/// fraction-elapsed for the time arcs.
const SESSION_WINDOW: i64 = 5 * 3600;
const WEEKLY_WINDOW: i64 = 7 * 24 * 3600;

/// The reset context for the budget the indicator is *showing*, so the
/// countdown matches the gauge: Session → session window, Weekly → weekly,
/// Worst → whichever budget is currently higher (the one drawn). Both shows two
/// gauges, so it surfaces the soonest of the pair.
pub fn reset_info(sample: &UsageSample, now: i64, scope: Scope) -> ResetInfo {
    let session = (sample.session_reset, SESSION_WINDOW);
    let weekly = (sample.weekly_reset, WEEKLY_WINDOW);
    let (reset, window) = match scope {
        Scope::Session => session,
        Scope::Weekly => weekly,
        Scope::Worst => {
            if sample.session >= sample.weekly {
                session
            } else {
                weekly
            }
        }
        Scope::Both => {
            if sample.session_reset <= sample.weekly_reset {
                session
            } else {
                weekly
            }
        }
    };
    let remaining = (reset - now).max(0);
    let elapsed = (1.0 - remaining as f32 / window as f32).clamp(0.0, 1.0);
    ResetInfo { elapsed, remaining }
}

/// Terse, proximity-coloured countdown: `2h`/`3d` (calm), `38m` (amber as it
/// nears), `soon` (green just before refresh).
fn compact_reset(remaining: i64) -> (String, Color) {
    let neutral = Color::from_rgba(1.0, 1.0, 1.0, 0.7);
    if remaining <= 5 * 60 {
        ("soon".to_string(), color(Level::Green))
    } else if remaining < 30 * 60 {
        (format!("{}m", remaining / 60), color(Level::Amber))
    } else if remaining >= 86_400 {
        (format!("{}d", remaining / 86_400), neutral)
    } else {
        (format!("{}h", remaining / 3_600), neutral)
    }
}

fn colored_text<'a>(s: String, c: Color) -> Element<'a, Message> {
    widget::text(s)
        .size(12)
        .class(cosmic::theme::Text::Color(c))
        .into()
}

/// Append a reset label to the right of the indicator.
fn with_reset_text<'a>(indicator: Element<'a, Message>, s: String, c: Color) -> Element<'a, Message> {
    widget::Row::new()
        .spacing(6)
        .align_y(Alignment::Center)
        .push(indicator)
        .push(colored_text(s, c))
        .into()
}

/// Soft colour halo behind the indicator, fading in as reset approaches
/// (within 30 min). Returns the indicator unchanged when reset is far off.
fn glow_wrap<'a>(indicator: Element<'a, Message>, remaining: i64) -> Element<'a, Message> {
    const NEAR: i64 = 30 * 60;
    if remaining >= NEAR {
        return indicator;
    }
    let t = 1.0 - (remaining.max(0) as f32 / NEAR as f32); // 0 → 1 as reset nears
    // Amber far-ish, green when about to refresh.
    let lvl = if remaining <= 5 * 60 { Level::Green } else { Level::Amber };
    let mut halo = color(lvl);
    halo.a = 0.15 + 0.35 * t;
    widget::container(indicator)
        .padding(4)
        .style(move |_t| widget::container::Style {
            background: Some(halo.into()),
            border: Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// A small filled circle, built from a fixed-size container with a rounded
/// background — the simplest portable primitive across libcosmic revs.
fn swatch<'a>(c: Color) -> Element<'a, Message> {
    widget::container(
        widget::Space::new()
            .width(Length::Fixed(DOT))
            .height(Length::Fixed(DOT)),
    )
    .style(move |_theme| widget::container::Style {
        background: Some(c.into()),
        border: Border {
            radius: (DOT / 2.0).into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn dot<'a>(g: &Gauge, cfg: &Config, dim: bool) -> Element<'a, Message> {
    let mut c = color(g.level);
    if dim {
        c.a = 0.45;
    }
    let circle = swatch(c);
    if cfg.show_percent {
        widget::Row::new()
            .spacing(4)
            .align_y(Alignment::Center)
            .push(circle)
            .push(widget::text(g.label.clone()).size(12))
            .into()
    } else {
        circle
    }
}

fn bar<'a>(g: &Gauge, cfg: &Config, dim: bool, reset: Option<&ResetInfo>) -> Element<'a, Message> {
    let full = 44.0_f32;
    let height = 7.0_f32;
    let filled = crate::fill::fill_width(g.value, full);

    // FillColor reads the proximity gradient (green→current hue); FillBar stays a
    // single neutral accent — its job is "how full", not "how alarming".
    let fill = match cfg.style {
        Style::FillColor => gradient_fill(g.value, &cfg.thresholds, dim),
        _ => {
            let mut c = ACCENT;
            if dim {
                c.a = 0.45;
            }
            Background::Color(c)
        }
    };
    let track = meter_track(fill, filled, full, height);

    // Track mode draws a thin time under-bar (accent-blue, distinct from the
    // usage fill) growing with elapsed-toward-reset — the bar analogue of the
    // ring's track arc.
    let body: Element<'a, Message> = match (cfg.reset_display, reset) {
        (ResetDisplay::Track, Some(ri)) => {
            let under = meter_track(time_fill(dim), crate::fill::fill_width(ri.elapsed, full), full, 3.0);
            widget::Column::new().spacing(2).push(track).push(under).into()
        }
        _ => track,
    };

    if cfg.show_percent {
        widget::Row::new()
            .spacing(4)
            .align_y(Alignment::Center)
            .push(body)
            .push(widget::text(g.label.clone()).size(12))
            .into()
    } else {
        body
    }
}

/// A vertical bar of width `w`, height `h`, filled bottom→top to `value` with
/// `fill`, over a theme-aware track. The primitive behind the vertical-bar style
/// and the standalone time column.
fn vcolumn<'a>(fill: Background, value: f32, w: f32, h: f32) -> Element<'a, Message> {
    let radius = w / 2.0;
    let filled_h = crate::fill::fill_width(value, h);
    let fill_box = widget::container(
        widget::Space::new()
            .width(Length::Fixed(w))
            .height(Length::Fixed(filled_h)),
    )
    .style(move |_t: &cosmic::Theme| widget::container::Style {
        background: Some(fill.clone()),
        border: Border { radius: radius.into(), ..Default::default() },
        ..Default::default()
    });
    widget::container(fill_box)
        .width(Length::Fixed(w))
        .height(Length::Fixed(h))
        .align_y(Alignment::End) // grow from the bottom
        .style(move |theme: &cosmic::Theme| {
            let mut track: Color = theme.cosmic().on_bg_color().into();
            track.a = 0.12;
            widget::container::Style {
                background: Some(track.into()),
                border: Border { radius: radius.into(), ..Default::default() },
                ..Default::default()
            }
        })
        .into()
}

/// Accent-blue background for the time tracks (under-bar / time column), dimmed
/// when the data is stale. Distinct from the usage gradient so the two read apart.
fn time_fill(dim: bool) -> Background {
    let mut c = ACCENT;
    c.a = if dim { 0.4 } else { 0.7 };
    Background::Color(c)
}

/// A vertical level column filling bottom→top with the proximity gradient. Reads
/// like a tiny VU meter; in `Both` scope two columns sit side by side. With
/// `reset_display = Track`, a thinner time column grows alongside it.
fn vbar<'a>(g: &Gauge, cfg: &Config, dim: bool, reset: Option<&ResetInfo>) -> Element<'a, Message> {
    let h = 16.0_f32;
    // 0 radians = bottom→top, so green sits at the base and the leading hue on top.
    let usage = vcolumn(gradient_at(g.value, &cfg.thresholds, dim, 0.0), g.value, 7.0, h);

    let body: Element<'a, Message> = match (cfg.reset_display, reset) {
        (ResetDisplay::Track, Some(ri)) => widget::Row::new()
            .spacing(3)
            .align_y(Alignment::End)
            .push(usage)
            .push(vcolumn(time_fill(dim), ri.elapsed, 5.0, h))
            .into(),
        _ => usage,
    };

    if cfg.show_percent {
        widget::Row::new()
            .spacing(4)
            .align_y(Alignment::Center)
            .push(body)
            .push(widget::text(g.label.clone()).size(12))
            .into()
    } else {
        body
    }
}

/// A circular progress ring — a "rolled-up" bar drawn as an inline SVG whose
/// foreground arc length is `value` of the circumference (via stroke-dasharray).
/// With `reset_display = DualRing` a thin inner arc shows time elapsed toward
/// reset; with `Track` the background track grows with elapsed time.
fn ring<'a>(g: &Gauge, cfg: &Config, dim: bool, reset: Option<&ResetInfo>) -> Element<'a, Message> {
    use std::f32::consts::PI;
    let size = 18.0_f32;
    let sw = 2.5_f32; // stroke width
    let c = size / 2.0;
    let r = (size - sw) / 2.0; // keep the stroke inside the viewBox
    let circ = 2.0 * PI * r;
    let filled = g.value.clamp(0.0, 1.0) * circ;
    // RingColor takes the gradient's leading-edge hue (the ring can't carry a
    // swept gradient cleanly, so the arc colour alone encodes proximity).
    let fg = match cfg.style {
        Style::RingColor => color_at(g.value, &cfg.thresholds),
        _ => ACCENT,
    };
    let alpha = if dim { 0.45 } else { 1.0 };
    let elapsed = reset.map(|ri| ri.elapsed).unwrap_or(0.0);

    // Track mode: a brighter background arc grows with elapsed time.
    let track_arc = if matches!(cfg.reset_display, ResetDisplay::Track) && reset.is_some() {
        format!(
            r##"<circle cx="{c}" cy="{c}" r="{r:.3}" fill="none" stroke="#ffffff" stroke-opacity="0.32" stroke-width="{sw}" stroke-dasharray="{te:.3} {circ:.3}" transform="rotate(-90 {c} {c})"/>"##,
            te = elapsed * circ,
        )
    } else {
        String::new()
    };

    // Dual ring: a thin inner concentric arc = elapsed time.
    let inner_r = r - 3.5;
    let inner_circ = 2.0 * PI * inner_r;
    let inner_arc = if matches!(cfg.reset_display, ResetDisplay::DualRing) && reset.is_some() {
        format!(
            r##"<circle cx="{c}" cy="{c}" r="{inner_r:.3}" fill="none" stroke="#ffffff" stroke-opacity="0.6" stroke-width="1.5" stroke-linecap="round" stroke-dasharray="{inf:.3} {inner_circ:.3}" transform="rotate(-90 {c} {c})"/>"##,
            inf = elapsed * inner_circ,
        )
    } else {
        String::new()
    };

    // Percentage centred inside the ring's hole when enabled — saves panel width,
    // but collides with the dual-ring time arc, so it's opt-out (percent beside).
    let inside = cfg.show_percent && cfg.percent_inside_ring;
    let center = if inside {
        let n = g.label.trim_end_matches('%');
        let fs = size * 0.40;
        format!(
            r##"<text x="{c}" y="{cy:.3}" text-anchor="middle" font-family="sans-serif" font-weight="600" font-size="{fs:.2}" fill="#ffffff" fill-opacity="{a:.2}">{n}</text>"##,
            cy = c + fs / 3.0,
            a = 0.85 * alpha,
        )
    } else {
        String::new()
    };

    // Layers: faint full track, optional time track, usage arc, optional inner arc, optional label.
    let doc = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">
<circle cx="{c}" cy="{c}" r="{r:.3}" fill="none" stroke="#ffffff" stroke-opacity="0.15" stroke-width="{sw}"/>
{track_arc}
<circle cx="{c}" cy="{c}" r="{r:.3}" fill="none" stroke="{stroke}" stroke-opacity="{alpha:.2}" stroke-width="{sw}" stroke-linecap="round" stroke-dasharray="{filled:.3} {circ:.3}" transform="rotate(-90 {c} {c})"/>
{inner_arc}
{center}
</svg>"##,
        stroke = hex(fg),
    );
    let ring_el = widget::svg(widget::svg::Handle::from_memory(doc.into_bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size));

    // Percent beside the ring when it isn't drawn inside (e.g. with dual-ring).
    if cfg.show_percent && !inside {
        widget::Row::new()
            .spacing(4)
            .align_y(Alignment::Center)
            .push(ring_el)
            .push(widget::text(g.label.clone()).size(12))
            .into()
    } else {
        ring_el.into()
    }
}

pub fn indicator_view<'a>(
    state: &IndicatorState,
    cfg: &Config,
    reset: Option<ResetInfo>,
) -> Element<'a, Message> {
    let (gauges, dim): (&[Gauge], bool) = match state {
        IndicatorState::NoData => {
            // Hollow grey dot when there is no data yet.
            let grey = Color::from_rgba(0.6, 0.6, 0.6, 0.5);
            return widget::Row::new().spacing(4).push(swatch(grey)).into();
        }
        IndicatorState::Live(g) => (g, false),
        IndicatorState::Stale(g) => (g, true),
    };

    let mut row = widget::Row::new().spacing(6).align_y(Alignment::Center);
    for g in gauges {
        let el = match cfg.style {
            Style::ColorDot => dot(g, cfg, dim),
            Style::FillBar | Style::FillColor => bar(g, cfg, dim, reset.as_ref()),
            Style::VBar => vbar(g, cfg, dim, reset.as_ref()),
            Style::Ring | Style::RingColor => ring(g, cfg, dim, reset.as_ref()),
        };
        row = row.push(el);
    }
    let indicator: Element<'a, Message> = row.into();

    // Apply the reset-display mode (the click popup always shows reset details).
    let Some(ri) = reset else {
        return indicator;
    };
    let is_ring = matches!(cfg.style, Style::Ring | Style::RingColor);
    match cfg.reset_display {
        ResetDisplay::None => indicator,
        ResetDisplay::Text => with_reset_text(
            indicator,
            format!("resets in {}", format_countdown(ri.remaining)),
            Color::from_rgba(1.0, 1.0, 1.0, 0.7),
        ),
        ResetDisplay::Compact => {
            let (s, c) = compact_reset(ri.remaining);
            with_reset_text(indicator, s, c)
        }
        ResetDisplay::Glow => glow_wrap(indicator, ri.remaining),
        // A standalone vertical time bar to the right of any indicator.
        ResetDisplay::TimeColumn => widget::Row::new()
            .spacing(6)
            .align_y(Alignment::End)
            .push(indicator)
            .push(vcolumn(time_fill(dim), ri.elapsed, 5.0, 16.0))
            .into(),
        // Track renders inside the indicator: an arc on rings, an under-bar on
        // horizontal bars, a companion column on vertical bars. DualRing is
        // ring-only. Anything not drawn inside falls back to compact text so the
        // setting is never a no-op.
        ResetDisplay::DualRing | ResetDisplay::Track => {
            let in_shape = matches!(
                cfg.style,
                Style::FillBar | Style::FillColor | Style::VBar
            );
            let drawn_inside =
                is_ring || (matches!(cfg.reset_display, ResetDisplay::Track) && in_shape);
            if drawn_inside {
                indicator
            } else {
                let (s, c) = compact_reset(ri.remaining);
                with_reset_text(indicator, s, c)
            }
        }
    }
}

/// Full popup width for the budget meters (also sets the popup's overall width).
const POPUP_METER: f32 = 200.0;

/// One budget as a gauge block: name + hero percentage on a row, a full-width
/// gradient meter beneath, and the reset countdown as a quiet caption.
fn budget_block<'a>(
    name: &str,
    value: f32,
    reset: i64,
    now: i64,
    t: &Thresholds,
) -> Element<'a, Message> {
    let pct = (value * 100.0).round() as i64;
    let lvl = level_for(value, t);
    // Bar with its percentage to the right, on one row; the reset countdown sits
    // on its own caption line below.
    let bar_row = widget::Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(meter_track(
            gradient_fill(value, t, false),
            crate::fill::fill_width(value, POPUP_METER),
            POPUP_METER,
            8.0,
        ))
        .push(
            widget::text(format!("{pct}%"))
                .size(16)
                .class(cosmic::theme::Text::Color(color(lvl))),
        );
    let countdown = widget::text::caption(format!("resets in {}", format_countdown(reset - now)))
        .class(cosmic::theme::Text::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.6)));
    widget::Column::new()
        .spacing(5)
        .push(widget::text::body(name.to_string()))
        .push(bar_row)
        .push(countdown)
        .into()
}

/// The click popup contents: both budgets as gauge blocks with a divider
/// between, and a right-aligned settings link in the footer.
pub fn popup_view<'a>(sample: &UsageSample, now: i64, cfg: &Config) -> Element<'a, Message> {
    let t = &cfg.thresholds;
    widget::Column::new()
        .spacing(14)
        .padding(14)
        .push(budget_block(
            "Session · 5h",
            sample.session,
            sample.session_reset,
            now,
            t,
        ))
        .push(widget::divider::horizontal::light())
        .push(budget_block(
            "Weekly · 7d",
            sample.weekly,
            sample.weekly_reset,
            now,
            t,
        ))
        // Footer: low-prominence settings link, pushed to the right.
        .push(
            widget::Row::new()
                .push(widget::Space::new().width(Length::Fill).height(Length::Fixed(0.0)))
                .push(
                    widget::button::custom(widget::text("⚙ Settings").size(12))
                        .class(cosmic::theme::Button::Text)
                        .on_press(Message::ToggleSettings),
                ),
        )
        .into()
}

/// A live preview of the indicator using fixed demo data (65% usage, ~18 min to
/// reset, ~60% of the window elapsed) so every style + reset-display combination
/// is visible regardless of the user's real usage. Reflects the current config.
fn settings_preview<'a>(cfg: &Config) -> Element<'a, Message> {
    use crate::indicator::{level_for, Gauge, IndicatorState};
    let g = |v: f32, label: &str| Gauge {
        value: v,
        level: level_for(v, &cfg.thresholds),
        label: label.to_string(),
    };
    let gauges = match cfg.scope {
        crate::config::Scope::Both => vec![g(0.65, "65%"), g(0.30, "30%")],
        _ => vec![g(0.65, "65%")],
    };
    let demo_reset = ResetInfo {
        // Decoupled on purpose: a clearly-partial time arc AND within glow range.
        elapsed: 0.62,
        remaining: 18 * 60,
    };
    let preview = indicator_view(&IndicatorState::Live(gauges), cfg, Some(demo_reset));
    let body = widget::Row::new()
        .align_y(Alignment::Center)
        .push(widget::text::caption("Preview"))
        .push(widget::Space::new().width(Length::Fill).height(Length::Fixed(0.0)))
        .push(preview);
    // Frame it as a card so it reads as a live sample, not part of the form.
    widget::container(body)
        .padding(12)
        .width(Length::Fill)
        .style(|theme: &cosmic::Theme| {
            let on = theme.cosmic().on_bg_color();
            let mut bg: Color = on.into();
            bg.a = 0.05;
            let mut edge: Color = on.into();
            edge.a = 0.12;
            widget::container::Style {
                background: Some(bg.into()),
                border: Border {
                    radius: 8.0.into(),
                    width: 1.0,
                    color: edge,
                },
                ..Default::default()
            }
        })
        .into()
}

/// The settings panel: dropdowns/toggles/sliders bound to config fields. Each
/// control emits a `Set*` message which mutates and persists the config.
pub fn settings_view<'a>(cfg: &Config) -> Element<'a, Message> {
    use crate::settings::{
        resets_for, scope_from_index, scope_index, style_from_index, style_index, SCOPE_LABELS,
        STYLE_LABELS,
    };
    use widget::settings::{item, section};

    let scope = widget::dropdown(&SCOPE_LABELS[..], Some(scope_index(cfg.scope)), |i| {
        Message::SetScope(scope_from_index(i))
    });
    let style = widget::dropdown(&STYLE_LABELS[..], Some(style_index(cfg.style)), |i| {
        Message::SetStyle(style_from_index(i))
    });
    // Only the reset modes this style can actually render (e.g. no Dual ring on
    // a bar). The two slices are parallel, so the dropdown index maps straight
    // back to a variant.
    let (reset_labels, reset_variants) = resets_for(cfg.style);
    let reset_sel = reset_variants.iter().position(|v| *v == cfg.reset_display);
    let reset = widget::dropdown(reset_labels, reset_sel, move |i| {
        Message::SetResetDisplay(reset_variants[i])
    });

    let amber_pct = (cfg.thresholds.amber * 100.0).round() as i64;
    let red_pct = (cfg.thresholds.red * 100.0).round() as i64;
    // The iced slider requires its value type to impl `Into<f64>`, which `u64`
    // does not; use `u32` for the widget and map back to `u64` in the message.
    let stale_mins = (cfg.stale_after / 60).max(1) as u32;
    let slider_w = Length::Fixed(160.0);

    let indicator = section()
        .title("Indicator")
        .add(item("Scope", scope))
        .add(item("Style", style))
        .add(item(
            "Show percent",
            widget::toggler(cfg.show_percent).on_toggle(Message::SetShowPercent),
        ))
        // Only meaningful for ring styles, and only once percent is shown at all.
        .add_maybe(
            (cfg.show_percent && matches!(cfg.style, Style::Ring | Style::RingColor)).then(|| {
                item(
                    "Percent inside ring",
                    widget::toggler(cfg.percent_inside_ring)
                        .on_toggle(Message::SetPercentInsideRing),
                )
            }),
        );

    let reset_sec = section()
        .title("Time to reset")
        .add(item("Display", reset));

    // Amber and red are one decision — where green turns to caution and caution
    // to alarm — so they share a section.
    let thresholds = section()
        .title("Colour thresholds")
        .add(item(
            format!("Amber at {amber_pct}%"),
            // step 0.01: the slider's default step is 1.0, which on a 0..=1 range
            // would only allow 0% or 100%.
            widget::slider(0.0..=1.0, cfg.thresholds.amber, Message::SetAmber)
                .step(0.01_f32)
                .width(slider_w),
        ))
        .add(item(
            format!("Red at {red_pct}%"),
            widget::slider(0.0..=1.0, cfg.thresholds.red, Message::SetRed)
                .step(0.01_f32)
                .width(slider_w),
        ));

    let data = section()
        .title("Data")
        .add(item(
            format!("Stale after {stale_mins} min"),
            widget::slider(1..=30u32, stale_mins, |v| Message::SetStaleAfterMins(v as u64))
                .width(slider_w),
        ))
        .add(
            widget::text_input(
                "Claude: ~/.claude/usage-history.jsonl",
                cfg.history_path.clone().unwrap_or_default(),
            )
            .on_input(Message::SetHistoryPath),
        );

    widget::Column::new()
        .spacing(16)
        .padding(16)
        .push(widget::text::title3("AI usage"))
        .push(settings_preview(cfg))
        .push(indicator)
        .push(reset_sec)
        .push(thresholds)
        .push(data)
        .push(widget::button::text("← Back").on_press(Message::ShowInfo))
        .into()
}

#[cfg(test)]
mod ttests {
    use super::*;
    use crate::usage::UsageSample;

    /// Sample where the session resets much sooner than the weekly.
    fn split_sample(now: i64, session: f32, weekly: f32) -> UsageSample {
        UsageSample {
            session,
            weekly,
            session_reset: now + 60 * 45,
            weekly_reset: now + 60 * 60 * 24 * 4,
            ts: 0,
        }
    }

    #[test]
    fn reset_info_both_uses_soonest_window() {
        let now = 2000;
        let ri = reset_info(&split_sample(now, 0.0, 0.0), now, Scope::Both);
        assert_eq!(ri.remaining, 60 * 45);
        // 45m remaining of a 5h window → 1 - 2700/18000 = 0.85 elapsed.
        assert!((ri.elapsed - 0.85).abs() < 0.001);
    }

    #[test]
    fn reset_info_weekly_follows_weekly_even_when_session_sooner() {
        let now = 2000;
        let ri = reset_info(&split_sample(now, 0.0, 0.0), now, Scope::Weekly);
        assert_eq!(ri.remaining, 60 * 60 * 24 * 4);
    }

    #[test]
    fn reset_info_worst_follows_the_higher_budget() {
        let now = 2000;
        // Weekly is the worse (higher) budget → its reset is shown, despite the
        // session resetting sooner.
        let ri = reset_info(&split_sample(now, 0.20, 0.90), now, Scope::Worst);
        assert_eq!(ri.remaining, 60 * 60 * 24 * 4);
        // Session is worse → session reset.
        let ri = reset_info(&split_sample(now, 0.90, 0.20), now, Scope::Worst);
        assert_eq!(ri.remaining, 60 * 45);
    }

    #[test]
    fn compact_reset_buckets() {
        assert_eq!(compact_reset(3 * 60).0, "soon");
        assert_eq!(compact_reset(20 * 60).0, "20m");
        assert_eq!(compact_reset(3 * 3600).0, "3h");
        assert_eq!(compact_reset(2 * 86_400).0, "2d");
    }

    #[test]
    fn color_at_hits_threshold_anchors() {
        let t = Thresholds { amber: 0.50, red: 0.80 };
        let eq = |a: Color, b: Color| {
            (a.r - b.r).abs() < 1e-4 && (a.g - b.g).abs() < 1e-4 && (a.b - b.b).abs() < 1e-4
        };
        // Anchored exactly on green/amber/red at empty / amber / red thresholds.
        assert!(eq(color_at(0.0, &t), color(Level::Green)));
        assert!(eq(color_at(0.50, &t), color(Level::Amber)));
        assert!(eq(color_at(0.80, &t), color(Level::Red)));
        // Saturates at red past the red threshold.
        assert!(eq(color_at(1.0, &t), color(Level::Red)));
    }

    #[test]
    fn color_at_blends_between_anchors() {
        let t = Thresholds { amber: 0.50, red: 0.80 };
        // Midway green→amber is a genuine blend: between the two on every channel.
        let mid = color_at(0.25, &t);
        let (g, a) = (color(Level::Green), color(Level::Amber));
        assert!(mid.r > g.r && mid.r < a.r);
    }
}
