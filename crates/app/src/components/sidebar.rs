use crate::state::{AppState, ViewTab};
use boox_core::util::{bytes_to_text, time_ago_text};
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex,
    label::Label,
    progress::Progress,
    ActiveTheme, Sizable, StyledExt,
};

pub fn render_sidebar(state_entity: &Entity<AppState>, cx: &mut App) -> impl IntoElement {
    let (nickname, auth_subtitle, is_authorized, storage_text, storage_percent, sync_time, upload_status_text, active_tab) = {
        let state = state_entity.read(cx);
        let snapshot = state.snapshot.as_ref();
        let auth = snapshot.map(|s| &s.auth);
        let is_auth = auth.map(|a| a.authorized).unwrap_or(false);
        let profile = snapshot.and_then(|s| s.profile.as_ref());
        let storage = snapshot.map(|s| &s.storage);
        let upload = snapshot.map(|s| &s.upload);

        let nick = profile
            .and_then(|p| p.nickname.clone())
            .unwrap_or_else(|| {
                if is_auth {
                    "BOOX 用户".to_string()
                } else {
                    "未登录".to_string()
                }
            });

        let sub = if is_auth {
            auth.map(|a| a.message.clone())
                .unwrap_or_else(|| "已登录".to_string())
        } else {
            auth.map(|a| a.message.clone())
                .unwrap_or_else(|| "正在检查本地授权状态…".to_string())
        };

        let st_text = match storage {
            Some(s) if s.used.is_some() && s.limit.is_some() => {
                let used = bytes_to_text(s.used.unwrap());
                let limit = bytes_to_text(s.limit.unwrap());
                format!("{used} / {limit}")
            }
            _ => "未知".to_string(),
        };

        let st_pct = storage
            .and_then(|s| s.percent)
            .map(|p| p as f32)
            .unwrap_or(0.0);

        let s_time = time_ago_text(state.last_sync_time_ms);
        let up_text = upload
            .map(|u| u.status_text.clone())
            .unwrap_or_else(|| "上传进度: 空闲".to_string());

        (nick, sub, is_auth, st_text, st_pct, s_time, up_text, state.active_tab)
    };

    v_flex()
        .w(px(260.0))
        .h_full()
        .bg(cx.theme().sidebar)
        .border_r_1()
        .border_color(cx.theme().border)
        .p_3()
        .justify_between()
        // Top section
        .child(
            v_flex()
                .gap_3()
                // Brand Header
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .p_1()
                        .child(
                            div()
                                .w(px(32.0))
                                .h(px(32.0))
                                .rounded_md()
                                .bg(rgb(0x0a84ff))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(rgb(0xffffff))
                                .font_bold()
                                .child("S")
                        )
                        .child(
                            v_flex()
                                .child(
                                    Label::new("sandFlos")
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                )
                                .child(
                                    Label::new("控制中心")
                                        .font_bold()
                                        .text_sm()
                                )
                        )
                )
                // Account Card
                .child(
                    div()
                        .rounded_lg()
                        .bg(cx.theme().background)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(Label::new("账户").text_xs().text_color(cx.theme().muted_foreground))
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(if is_authorized { rgb(0x22c55e) } else { rgb(0x64748b) })
                                        .text_color(rgb(0xffffff))
                                        .text_xs()
                                        .child(if is_authorized { "已授权" } else { "未登录" })
                                )
                        )
                        .child(Label::new(nickname).font_bold().text_sm())
                        .child(
                            Label::new(auth_subtitle)
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .pt_1()
                                .child({
                                    let entity = state_entity.clone();
                                    Button::new("login-btn")
                                        .primary()
                                        .small()
                                        .label("浏览器登录")
                                        .on_click(move |_, _, cx| {
                                            entity.update(cx, |state, cx| {
                                                state.start_login(cx);
                                            });
                                        })
                                })
                                .child({
                                    let entity = state_entity.clone();
                                    Button::new("upload-btn")
                                        .outline()
                                        .small()
                                        .label("上传文件")
                                        .on_click(move |_, _, cx| {
                                            entity.update(cx, |state, cx| {
                                                state.pick_and_upload_files(cx);
                                            });
                                        })
                                })
                        )
                )
                // Navigation menu
                .child(
                    v_flex()
                        .gap_1()
                        .pt_1()
                        .child(render_nav_item(
                            "nav-overview",
                            ViewTab::Overview,
                            "概览",
                            "摘要与状态",
                            active_tab == ViewTab::Overview,
                            state_entity,
                            cx,
                        ))
                        .child(render_nav_item(
                            "nav-push",
                            ViewTab::Push,
                            "互动文件",
                            "推送与上传",
                            active_tab == ViewTab::Push,
                            state_entity,
                            cx,
                        ))
                        .child(render_nav_item(
                            "nav-devices",
                            ViewTab::Devices,
                            "设备",
                            "局域网与在线设备",
                            active_tab == ViewTab::Devices,
                            state_entity,
                            cx,
                        ))
                        .child(render_nav_item(
                            "nav-reading",
                            ViewTab::Reading,
                            "阅读",
                            "今日与本周指标",
                            active_tab == ViewTab::Reading,
                            state_entity,
                            cx,
                        ))
                        .child(render_nav_item(
                            "nav-zotero",
                            ViewTab::Zotero,
                            "Zotero",
                            "附件工作流与推送",
                            active_tab == ViewTab::Zotero,
                            state_entity,
                            cx,
                        ))
                        .child(render_nav_item(
                            "nav-calibre",
                            ViewTab::Calibre,
                            "Calibre",
                            "书库读取与推送",
                            active_tab == ViewTab::Calibre,
                            state_entity,
                            cx,
                        ))
                )
        )
        // Bottom section
        .child(
            v_flex()
                .gap_2()
                // Storage Card
                .child(
                    div()
                        .rounded_lg()
                        .bg(cx.theme().background)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            h_flex()
                                .justify_between()
                                .child(Label::new("云空间").text_xs().text_color(cx.theme().muted_foreground))
                                .child(Label::new(storage_text).text_xs().font_semibold())
                        )
                        .child(Progress::new().value(storage_percent))
                )
                // Meta & Status
                .child(
                    div()
                        .rounded_lg()
                        .bg(cx.theme().background)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            h_flex()
                                .justify_between()
                                .child(Label::new("同步状态").text_xs().text_color(cx.theme().muted_foreground))
                                .child(Label::new(sync_time).text_xs())
                        )
                        .child(
                            Label::new(upload_status_text)
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                        )
                )
        )
}

fn render_nav_item(
    id: &'static str,
    tab: ViewTab,
    title: &'static str,
    subtitle: &'static str,
    is_active: bool,
    state_entity: &Entity<AppState>,
    cx: &mut App,
) -> impl IntoElement {
    let entity = state_entity.clone();
    div()
        .id(id)
        .w_full()
        .p_2()
        .rounded_md()
        .cursor_pointer()
        .bg(if is_active {
            cx.theme().accent
        } else {
            gpui::transparent_black()
        })
        .hover(|s| {
            if !is_active {
                s.bg(cx.theme().accent.opacity(0.5))
            } else {
                s
            }
        })
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            Label::new(title)
                .text_sm()
                .font_semibold()
                .text_color(if is_active {
                    cx.theme().accent_foreground
                } else {
                    cx.theme().foreground
                })
        )
        .child(
            Label::new(subtitle)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
        )
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            entity.update(cx, |state, cx| {
                state.set_active_tab(tab, cx);
            });
        })
}
