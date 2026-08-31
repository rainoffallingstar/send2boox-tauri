use crate::state::AppState;
use boox_core::util::{duration_text, reading_today_count, reading_total_count, reading_week_total_ms};
use gpui::*;
use gpui_component::{
    h_flex, v_flex,
    label::Label,
    ActiveTheme, StyledExt,
};

pub fn render_reading_view(state_entity: &Entity<AppState>, cx: &mut App) -> impl IntoElement {
    let state = state_entity.read(cx);
    let snapshot = state.snapshot.as_ref();
    let calendar = snapshot.map(|s| &s.calendar_metrics);

    let today_count = calendar
        .map(|c| reading_today_count(&c.day_read_today))
        .unwrap_or(0);
    let week_ms = calendar
        .map(|c| reading_week_total_ms(&c.read_time_week))
        .unwrap_or(0);
    let total_count = calendar
        .map(|c| reading_total_count(&c.reading_info))
        .unwrap_or(0);

    v_flex()
        .w_full()
        .gap_4()
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("阅读统计指标").font_bold().text_base())
                .child(Label::new("聚合显示您在 BOOX 设备上的阅读进度、阅读时长与完成度").text_xs().text_color(cx.theme().muted_foreground))
        )
        // KPI Cards Row
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .rounded_xl()
                        .bg(cx.theme().sidebar)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Label::new("今日阅读书籍").text_xs().text_color(cx.theme().muted_foreground))
                        .child(
                            Label::new(format!("{} 本", today_count))
                                .font_bold()
                                .text_2xl()
                                .text_color(cx.theme().primary)
                        )
                        .child(Label::new("今日有阅读记录的书目").text_xs().text_color(cx.theme().muted_foreground))
                )
                .child(
                    div()
                        .flex_1()
                        .rounded_xl()
                        .bg(cx.theme().sidebar)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Label::new("本周阅读时长").text_xs().text_color(cx.theme().muted_foreground))
                        .child(
                            Label::new(duration_text(week_ms))
                                .font_bold()
                                .text_2xl()
                                .text_color(rgb(0x10b981))
                        )
                        .child(Label::new("本周累计有效阅读时间").text_xs().text_color(cx.theme().muted_foreground))
                )
                .child(
                    div()
                        .flex_1()
                        .rounded_xl()
                        .bg(cx.theme().sidebar)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Label::new("累计完成阅读").text_xs().text_color(cx.theme().muted_foreground))
                        .child(
                            Label::new(format!("{} 本", total_count))
                                .font_bold()
                                .text_2xl()
                                .text_color(rgb(0x8b5cf6))
                        )
                        .child(Label::new("云端同步的已读完书籍总数").text_xs().text_color(cx.theme().muted_foreground))
                )
        )
        // Reading Detail Info Card
        .child(
            div()
                .w_full()
                .rounded_xl()
                .bg(cx.theme().sidebar)
                .border_1()
                .border_color(cx.theme().border)
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(Label::new("云端同步指标详情").font_bold().text_sm())
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .justify_between()
                                .p_2()
                                .rounded_md()
                                .bg(cx.theme().background)
                                .child(Label::new("今日记录明细").text_xs().text_color(cx.theme().muted_foreground))
                                .child(Label::new(format!("{} 条", today_count)).text_xs().font_semibold())
                        )
                        .child(
                            h_flex()
                                .justify_between()
                                .p_2()
                                .rounded_md()
                                .bg(cx.theme().background)
                                .child(Label::new("本周总毫秒数").text_xs().text_color(cx.theme().muted_foreground))
                                .child(Label::new(format!("{} ms", week_ms)).text_xs().font_semibold())
                        )
                        .child(
                            h_flex()
                                .justify_between()
                                .p_2()
                                .rounded_md()
                                .bg(cx.theme().background)
                                .child(Label::new("总阅读统计状态").text_xs().text_color(cx.theme().muted_foreground))
                                .child(Label::new(if total_count > 0 { "已同步" } else { "未获取到记录" }).text_xs().font_semibold())
                        )
                )
        )
}
