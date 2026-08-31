use crate::state::AppState;
use boox_core::util::{bytes_to_text, duration_text, reading_today_count, reading_total_count, reading_week_total_ms, time_ago_text};
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex,
    label::Label,
    progress::Progress,
    ActiveTheme, Sizable, StyledExt,
};

pub fn render_overview_view(state_entity: &Entity<AppState>, cx: &mut App) -> impl IntoElement {
    let (is_authorized, nickname, devices_len, push_queue_items, active_upload, storage_used_str, today_count, week_ms, total_count) = {
        let state = state_entity.read(cx);
        let snapshot = state.snapshot.as_ref();
        let is_auth = snapshot.map(|s| s.auth.authorized).unwrap_or(false);
        let nick = snapshot
            .and_then(|s| s.profile.as_ref())
            .and_then(|p| p.nickname.clone())
            .unwrap_or_else(|| if is_auth { "BOOX 用户".to_string() } else { "未登录".to_string() });
        let d_len = snapshot.map(|s| s.devices.len()).unwrap_or(0);
        let q_items = snapshot.map(|s| s.push_queue.clone()).unwrap_or_default();
        let up = snapshot.map(|s| s.upload.clone()).filter(|u| u.in_progress);
        let st_used = snapshot
            .and_then(|s| s.storage.used)
            .map(bytes_to_text)
            .unwrap_or_else(|| "0 B".to_string());
        let cal = snapshot.map(|s| &s.calendar_metrics);
        let t_count = cal.map(|c| reading_today_count(&c.day_read_today)).unwrap_or(0);
        let w_ms = cal.map(|c| reading_week_total_ms(&c.read_time_week)).unwrap_or(0);
        let tot_count = cal.map(|c| reading_total_count(&c.reading_info)).unwrap_or(0);

        (is_auth, nick, d_len, q_items, up, st_used, t_count, w_ms, tot_count)
    };

    v_flex()
        .w_full()
        .gap_4()
        // Upload Progress Card if active
        .when_some(active_upload, |this, u| {
            let percent = u.progress_percent.unwrap_or(0.0) as f32;
            let file_name = u.current_file.unwrap_or_else(|| "正在传输...".to_string());
            let speed_text = u.speed_bps.map(|s| format!("{}/s", bytes_to_text(s as u64))).unwrap_or_else(|| "-/s".to_string());
            let eta_text = u.eta_seconds.map(|e| format!("剩余 {}s", e as u64)).unwrap_or_else(|| "".to_string());

            this.child(
                div()
                    .rounded_xl()
                    .bg(cx.theme().accent.opacity(0.3))
                    .border_1()
                    .border_color(cx.theme().primary)
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(Label::new("正在上传并推送文件").font_bold().text_sm())
                                    .child(Label::new(file_name).text_sm().text_color(cx.theme().primary))
                            )
                            .child(
                                h_flex()
                                    .gap_3()
                                    .child(Label::new(speed_text).text_xs().font_semibold())
                                    .child(Label::new(eta_text).text_xs().text_color(cx.theme().muted_foreground))
                            )
                    )
                    .child(Progress::new().value(percent))
            )
        })
        // Top KPI Grid (4 Cards)
        .child(
            h_flex()
                .w_full()
                .gap_3()
                // Card 1: Auth / Account
                .child(
                    div()
                        .flex_1()
                        .rounded_xl()
                        .bg(cx.theme().sidebar)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(Label::new("授权状态").text_xs().text_color(cx.theme().muted_foreground))
                        .child(
                            Label::new(if is_authorized { "已登录授权" } else { "未登录" })
                                .font_bold()
                                .text_lg()
                                .text_color(if is_authorized { rgb(0x22c55e) } else { rgb(0xef4444) })
                        )
                        .child(
                            Label::new(nickname)
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                        )
                )
                // Card 2: Devices
                .child(
                    div()
                        .flex_1()
                        .rounded_xl()
                        .bg(cx.theme().sidebar)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(Label::new("在线设备").text_xs().text_color(cx.theme().muted_foreground))
                        .child(
                            Label::new(format!("{} 台", devices_len))
                                .font_bold()
                                .text_lg()
                        )
                        .child(
                            Label::new(if devices_len == 0 { "暂无绑定设备".to_string() } else { format!("已发现 {} 台设备", devices_len) })
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                        )
                )
                // Card 3: Reading Stats
                .child(
                    div()
                        .flex_1()
                        .rounded_xl()
                        .bg(cx.theme().sidebar)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(Label::new("今日与本周阅读").text_xs().text_color(cx.theme().muted_foreground))
                        .child(
                            Label::new(format!("今日 {} 本 / 本周 {}", today_count, duration_text(week_ms)))
                                .font_bold()
                                .text_base()
                        )
                        .child(
                            Label::new(format!("累计完成阅读 {} 本", total_count))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                        )
                )
                // Card 4: Push Queue
                .child(
                    div()
                        .flex_1()
                        .rounded_xl()
                        .bg(cx.theme().sidebar)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(Label::new("互动文件队列").text_xs().text_color(cx.theme().muted_foreground))
                        .child(
                            Label::new(format!("{} 条记录", push_queue_items.len()))
                                .font_bold()
                                .text_lg()
                        )
                        .child(
                            Label::new(format!("空间使用: {}", storage_used_str))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                        )
                )
        )
        // Recent Push Items Card
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
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            v_flex()
                                .child(Label::new("最近互动文件").font_bold().text_base())
                                .child(Label::new("展示最近推送至 BOOX 设备的文件队列").text_xs().text_color(cx.theme().muted_foreground))
                        )
                        .child({
                            let entity = state_entity.clone();
                            Button::new("upload-more-btn")
                                .primary()
                                .small()
                                .label("上传新文件")
                                .on_click(move |_, _, cx| {
                                    entity.update(cx, |state, cx| {
                                        state.pick_and_upload_files(cx);
                                    });
                                })
                        })
                )
                .child(
                    if push_queue_items.is_empty() {
                        div()
                            .p_6()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Label::new("暂无互动文件记录，点击右上角「上传新文件」开始推送").text_sm().text_color(cx.theme().muted_foreground))
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_2()
                            .children(push_queue_items.iter().take(6).enumerate().map(|(idx, item)| {
                                let doc_id = item.id.clone();
                                let title = item.name.clone();
                                let time_text = item.updated_at.map(|t| time_ago_text(t as u128)).unwrap_or_else(|| "-".to_string());
                                let size_text = item.size.map(bytes_to_text).unwrap_or_default();
                                let format_text = item.format.clone().unwrap_or_else(|| "PDF".to_string()).to_ascii_uppercase();

                                h_flex()
                                    .w_full()
                                    .p_2()
                                    .rounded_lg()
                                    .bg(cx.theme().background)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .items_center()
                                            .child(
                                                div()
                                                    .px_2()
                                                    .py_1()
                                                    .rounded_md()
                                                    .bg(cx.theme().primary.opacity(0.15))
                                                    .text_color(cx.theme().primary)
                                                    .font_bold()
                                                    .text_xs()
                                                    .child(format_text)
                                            )
                                            .child(
                                                v_flex()
                                                    .child(Label::new(title).font_semibold().text_sm())
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .child(Label::new(size_text).text_xs().text_color(cx.theme().muted_foreground))
                                                            .child(Label::new("·").text_xs().text_color(cx.theme().muted_foreground))
                                                            .child(Label::new(time_text).text_xs().text_color(cx.theme().muted_foreground))
                                                    )
                                            )
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child({
                                                let entity = state_entity.clone();
                                                let id = doc_id.clone();
                                                Button::new(SharedString::from(format!("ov-resend-{}", idx)))
                                                    .outline()
                                                    .small()
                                                    .label("重推")
                                                    .on_click(move |_, _, cx| {
                                                        entity.update(cx, |state, cx| {
                                                            state.resend_push_item(id.clone(), cx);
                                                        });
                                                    })
                                            })
                                            .child({
                                                let entity = state_entity.clone();
                                                let id = doc_id.clone();
                                                Button::new(SharedString::from(format!("ov-delete-{}", idx)))
                                                    .ghost()
                                                    .small()
                                                    .label("删除")
                                                    .on_click(move |_, _, cx| {
                                                        entity.update(cx, |state, cx| {
                                                            state.delete_push_item(id.clone(), cx);
                                                        });
                                                    })
                                            })
                                    )
                            }))
                            .into_any_element()
                    }
                )
        )
}
