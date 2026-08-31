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

pub fn render_calibre_view(state_entity: &Entity<AppState>, cx: &mut App) -> impl IntoElement {
    let (is_loading, books, pushing_id, active_dir, db_status) = {
        let state = state_entity.read(cx);
        let c = &state.calibre;
        let conn = c.connection.as_ref();
        let active = conn
            .and_then(|st| st.summary.library_dirs.first().cloned())
            .unwrap_or_else(|| "未配置 Calibre 书库目录".to_string());
        let db = conn
            .and_then(|st| st.summary.database_paths.first().cloned())
            .unwrap_or_else(|| "未定位 metadata.db".to_string());
        (c.is_loading, c.books.clone(), c.pushing_id, active, db)
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
                        .child(Label::new("Calibre 书库工作流").font_bold().text_base())
                        .child(Label::new("直接读取 metadata.db 书库元数据，按书籍格式与原始标题一键推送到 BOOX").text_xs().text_color(cx.theme().muted_foreground))
                )
                .child({
                    let entity = state_entity.clone();
                    Button::new("calibre-reload-btn")
                        .outline()
                        .small()
                        .label(if is_loading { "读取中…" } else { "刷新书库" })
                        .when(is_loading, |btn| btn.disabled(true))
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |state, cx| {
                                state.load_calibre(cx);
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
                                .child(Label::new("当前书库:").text_xs().text_color(cx.theme().muted_foreground))
                                .child(Label::new(active_dir).text_xs().font_semibold())
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(Label::new("数据库文件:").text_xs().text_color(cx.theme().muted_foreground))
                                .child(Label::new(db_status).text_xs())
                        )
                )
        )
        // Books List Card
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
                        .child(Label::new(format!("书库书籍 ({} 本)", books.len())).font_bold().text_sm())
                        .when(is_loading, |this| {
                            this.child(Spinner::new().small())
                        })
                )
                .child(
                    if books.is_empty() {
                        div()
                            .p_8()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Label::new(if is_loading { "正在从 Calibre 扫描书籍..." } else { "未找到书籍记录，请确认 Calibre 书库目录设置" })
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                            )
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_2()
                            .children(books.iter().enumerate().map(|(book_idx, book)| {
                                let book_title = book.title.clone();
                                let authors = book.author_summary.clone().unwrap_or_else(|| "未知作者".to_string());
                                let formats = book.formats.clone();
                                let library_dir = book.library_dir.clone();

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
                                                    .child(Label::new(book_title).font_bold().text_sm())
                                                    .child(
                                                        Label::new(authors)
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                    )
                                            )
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .children(formats.into_iter().enumerate().map(|(fmt_idx, fmt)| {
                                                let data_id = fmt.data_id;
                                                let format_name = fmt.format.to_ascii_uppercase();
                                                let is_pushing = pushing_id == Some(data_id);
                                                let lib_dir = library_dir.clone();
                                                let entity = state_entity.clone();

                                                Button::new(SharedString::from(format!("calibre-push-{}-{}-{}", book_idx, fmt_idx, format_name)))
                                                    .primary()
                                                    .small()
                                                    .label(if is_pushing { format!("{} 推送中…", format_name) } else { format!("推送 {}", format_name) })
                                                    .when(is_pushing, |btn| btn.disabled(true))
                                                    .on_click(move |_, _, cx| {
                                                        let l_dir = lib_dir.clone();
                                                        entity.update(cx, move |state, cx| {
                                                            state.push_calibre_book_format(l_dir.clone(), data_id, cx);
                                                        });
                                                    })
                                            }))
                                    )
                            }))
                            .into_any_element()
                    }
                )
        )
}
