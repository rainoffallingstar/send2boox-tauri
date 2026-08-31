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

pub fn render_zotero_view(state_entity: &Entity<AppState>, cx: &mut App) -> impl IntoElement {
    let (is_loading, items, pushing_id, db_status, webdav_status) = {
        let state = state_entity.read(cx);
        let z = &state.zotero;
        let conn = z.connection.as_ref();
        let db = conn
            .and_then(|c| c.summary.database_path.clone())
            .unwrap_or_else(|| "未配置或未找到 zotero.sqlite".to_string());
        let webdav = conn
            .map(|c| if c.summary.webdav_verified { "WebDAV 凭据已验证" } else if c.summary.password_saved { "WebDAV 已配置 (未验证)" } else { "未配置 WebDAV" })
            .unwrap_or("未连接");
        (z.is_loading, z.items.clone(), z.pushing_id, db, webdav)
    };

    v_flex()
        .w_full()
        .gap_4()
        // Top Header
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    v_flex()
                        .child(Label::new("Zotero 附件工作流").font_bold().text_base())
                        .child(Label::new("直接读取本地 Zotero 数据库，支持将文献 PDF / 附件直接推送到 BOOX 设备").text_xs().text_color(cx.theme().muted_foreground))
                )
                .child({
                    let entity = state_entity.clone();
                    Button::new("zotero-reload-btn")
                        .outline()
                        .small()
                        .label(if is_loading { "读取中…" } else { "刷新文献列表" })
                        .when(is_loading, |btn| btn.disabled(true))
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |state, cx| {
                                state.load_zotero(cx);
                            });
                        })
                })
        )
        // Connection Info Banner
        .child(
            div()
                .w_full()
                .rounded_xl()
                .bg(cx.theme().sidebar)
                .border_1()
                .border_color(cx.theme().border)
                .p_3()
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
                                .child(Label::new("Zotero 数据库:").text_xs().text_color(cx.theme().muted_foreground))
                                .child(Label::new(db_status).text_xs().font_semibold())
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(Label::new("同步渠道:").text_xs().text_color(cx.theme().muted_foreground))
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(cx.theme().primary.opacity(0.12))
                                        .text_color(cx.theme().primary)
                                        .text_xs()
                                        .child(webdav_status)
                                )
                        )
                )
        )
        // Items List Card
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
                        .child(Label::new(format!("最近文献附件 ({} 篇)", items.len())).font_bold().text_sm())
                        .when(is_loading, |this| {
                            this.child(Spinner::new().small())
                        })
                )
                .child(
                    if items.is_empty() {
                        div()
                            .p_8()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Label::new(if is_loading { "正在从 Zotero 数据库扫描文献..." } else { "未找到文献记录，请确认本地 Zotero 数据库路径" })
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                            )
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_2()
                            .children(items.iter().enumerate().map(|(item_idx, item)| {
                                let title = item.title.clone();
                                let author = item.author_summary.clone().unwrap_or_else(|| "未知作者".to_string());
                                let year_str = item.year.clone().unwrap_or_default();
                                let attachments = item.attachments.clone();

                                div()
                                    .w_full()
                                    .p_3()
                                    .rounded_lg()
                                    .bg(cx.theme().background)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .items_start()
                                            .child(
                                                v_flex()
                                                    .gap_0p5()
                                                    .child(Label::new(title).font_bold().text_sm())
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .child(Label::new(author).text_xs().text_color(cx.theme().muted_foreground))
                                                            .when(!year_str.is_empty(), |h| {
                                                                h.child(Label::new("·").text_xs().text_color(cx.theme().muted_foreground))
                                                                 .child(Label::new(year_str).text_xs().text_color(cx.theme().muted_foreground))
                                                            })
                                                    )
                                            )
                                    )
                                    .when(!attachments.is_empty(), |card| {
                                        card.child(
                                            v_flex()
                                                .gap_1()
                                                .pt_1()
                                                .border_t_1()
                                                .border_color(cx.theme().border)
                                                .children(attachments.into_iter().enumerate().map(|(att_idx, att)| {
                                                    let att_id = att.attachment_item_id;
                                                    let file_name = att.file_name.clone().unwrap_or_else(|| "附件".to_string());
                                                    let is_pushing = pushing_id == Some(att_id);
                                                    let local_exists = att.local_exists;

                                                    h_flex()
                                                        .justify_between()
                                                        .items_center()
                                                        .child(
                                                            h_flex()
                                                                .gap_2()
                                                                .items_center()
                                                                .child(
                                                                    div()
                                                                        .px_1p5()
                                                                        .py_0p5()
                                                                        .rounded_sm()
                                                                        .bg(cx.theme().muted)
                                                                        .text_color(cx.theme().muted_foreground)
                                                                        .text_xs()
                                                                        .child(if local_exists { "本地已下载" } else { "需 WebDAV 拉取" })
                                                                )
                                                                .child(Label::new(file_name).text_xs())
                                                        )
                                                        .child({
                                                            let entity = state_entity.clone();
                                                            Button::new(SharedString::from(format!("zotero-push-{}-{}", item_idx, att_idx)))
                                                                .primary()
                                                                .small()
                                                                .label(if is_pushing { "推送中…" } else { "推送到 BOOX" })
                                                                .when(is_pushing, |btn| btn.disabled(true))
                                                                .on_click(move |_, _, cx| {
                                                                    entity.update(cx, |state, cx| {
                                                                        state.push_zotero_item(att_id, cx);
                                                                    });
                                                                })
                                                        })
                                                }))
                                        )
                                    })
                            }))
                            .into_any_element()
                    }
                )
        )
}
