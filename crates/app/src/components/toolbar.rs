use crate::state::AppState;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex,
    label::Label,
    spinner::Spinner,
    ActiveTheme, Disableable, Sizable, StyledExt,
};

pub fn render_toolbar(state_entity: &Entity<AppState>, cx: &mut App) -> impl IntoElement {
    let (tab, is_loading, notification) = {
        let state = state_entity.read(cx);
        (state.active_tab, state.is_loading, state.status_notification.clone())
    };

    h_flex()
        .w_full()
        .h(px(64.0))
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .justify_between()
        .items_center()
        // Left Title block
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Label::new(tab.kicker())
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().primary)
                        )
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(cx.theme().muted)
                                .text_color(cx.theme().muted_foreground)
                                .text_xs()
                                .child(tab.badge())
                        )
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Label::new(tab.title()).font_bold().text_base())
                        .child(
                            Label::new(tab.subtitle())
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                        )
                )
        )
        // Right Action block
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .when_some(notification, |this, msg| {
                    this.child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(cx.theme().accent)
                            .text_color(cx.theme().accent_foreground)
                            .text_xs()
                            .child(msg)
                    )
                })
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(Label::new("自动刷新: 1分钟").text_xs().text_color(cx.theme().muted_foreground))
                )
                .child({
                    let entity = state_entity.clone();
                    Button::new("refresh-btn")
                        .outline()
                        .small()
                        .label(if is_loading { "刷新中…" } else { "刷新" })
                        .when(is_loading, |btn| btn.disabled(true))
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |state, cx| {
                                state.refresh_snapshot(cx);
                            });
                        })
                })
                .when(is_loading, |this| {
                    this.child(Spinner::new().small())
                })
        )
}
