//! NeoHub native desktop host (macOS first).
//!
//! Phase 2's job is only to prove the host works: a real GPUI window with the
//! product's name, size limits, palette and CJK-capable text. Product views move
//! into `neohub-ui` from phase 9; nothing here is a delivered feature yet.

use gpui_kit::*;
use neohub_ui::theme::{DARK, metrics};

/// Placeholder view used by the phase-2 host verification.
struct HostProbe {
    typed: String,
}

impl Render for HostProbe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = DARK;
        let typed = self.typed.clone();
        let show_placeholder = typed.is_empty();
        div()
            .flex()
            .flex_col()
            .size_full()
            .gap(px(metrics::GAP_3))
            .bg(palette.bg_base)
            .text_color(palette.fg)
            .p(px(metrics::PAD_6))
            .child(
                div()
                    .text_size(px(metrics::TEXT_LG))
                    .child("NeoHub · native host probe · 中文输入验证"),
            )
            .child(
                div()
                    .text_size(px(metrics::TEXT_SM))
                    .text_color(palette.fg_muted)
                    .child("侧栏 / 仪表盘占位 — phase 2 host check only"),
            )
            .child(
                div()
                    .text_size(px(metrics::TEXT_SM))
                    .child("输入框（验证中文输入法 / 选择 / 粘贴）："),
            )
            .child(
                div()
                    .id("probe-input")
                    .w(px(360.0))
                    .px(px(metrics::PAD_3))
                    .py(px(metrics::PAD_2))
                    .rounded(px(metrics::RADIUS_MD))
                    .border_1()
                    .border_color(palette.border_strong)
                    .bg(palette.bg_surface)
                    .text_size(px(metrics::TEXT_BASE))
                    .child(if show_placeholder {
                        SharedString::from("在此输入…")
                    } else {
                        SharedString::from(typed)
                    }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let text = event.keystroke.key_char.clone().unwrap_or_default();
                if text.is_empty() {
                    return;
                }
                this.typed.push_str(&text);
                cx.notify();
            }))
    }
}

fn main() {
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(
                px(metrics::DEFAULT_WINDOW_WIDTH / 8.0),
                px(metrics::DEFAULT_WINDOW_HEIGHT / 8.0),
            ),
            size(
                px(metrics::DEFAULT_WINDOW_WIDTH),
                px(metrics::DEFAULT_WINDOW_HEIGHT),
            ),
        ))),
        window_min_size: Some(size(
            px(metrics::MIN_WINDOW_WIDTH),
            px(metrics::MIN_WINDOW_HEIGHT),
        )),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from("NeoHub")),
            ..Default::default()
        }),
        ..Default::default()
    };

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(window_options, cx, |_, cx| {
            cx.new(|_| HostProbe {
                typed: String::new(),
            })
        })
        .expect("failed to open the NeoHub host window");
    });
}
