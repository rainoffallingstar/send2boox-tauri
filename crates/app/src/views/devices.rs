use crate::state::AppState;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex,
    label::Label,
    ActiveTheme, Sizable, StyledExt,
};

pub fn render_devices_view(state_entity: &Entity<AppState>, cx: &mut App) -> impl IntoElement {
    let devices = {
        let state = state_entity.read(cx);
        let snapshot = state.snapshot.as_ref();
        snapshot.map(|s| s.devices.clone()).unwrap_or_default()
    };

    v_flex()
        .w_full()
        .gap_4()
        // Top Header
        .child(
            v_flex()
                .gap_1()
                .child(Label::new(format!("绑定与在线设备 ({} 台)", devices.len())).font_bold().text_base())
                .child(Label::new("查看账号下已绑定的 BOOX 墨水屏设备，并在同一局域网下快速打开互传通道").text_xs().text_color(cx.theme().muted_foreground))
        )
        // Devices Cards List
        .child(
            if devices.is_empty() {
                div()
                    .w_full()
                    .rounded_xl()
                    .bg(cx.theme().sidebar)
                    .border_1()
                    .border_color(cx.theme().border)
                    .p_8()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Label::new("暂未发现绑定设备，请在 BOOX 设备上登录同一账号").text_sm().text_color(cx.theme().muted_foreground))
                    .into_any_element()
            } else {
                v_flex()
                    .w_full()
                    .gap_3()
                    .children(devices.iter().enumerate().map(|(idx, device)| {
                        let model_name = device.model.clone().unwrap_or_else(|| "BOOX 设备".to_string());
                        let mac = device.mac_address.clone().unwrap_or_else(|| "-".to_string());
                        let ip = device.ip_address.clone().unwrap_or_else(|| "-".to_string());
                        let is_online = device.same_lan || device.login_status.as_deref() == Some("online") || device.login_status.as_deref() == Some("1");
                        let last_active = device.latest_login_time.clone().unwrap_or_else(|| "未知".to_string());
                        let transfer_host = device.transfer_host.clone();

                        div()
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
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                div()
                                                    .w(px(10.0))
                                                    .h(px(10.0))
                                                    .rounded_full()
                                                    .bg(if is_online { rgb(0x22c55e) } else { rgb(0x94a3b8) })
                                            )
                                            .child(Label::new(model_name).font_bold().text_base())
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .bg(if is_online { cx.theme().accent } else { cx.theme().muted })
                                            .text_color(if is_online { cx.theme().accent_foreground } else { cx.theme().muted_foreground })
                                            .text_xs()
                                            .child(if is_online { "在线/同局域网" } else { "离线" })
                                    )
                            )
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .child(Label::new("局域网 IP").text_xs().text_color(cx.theme().muted_foreground))
                                            .child(Label::new(ip).text_xs().font_semibold())
                                    )
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .child(Label::new("MAC 地址").text_xs().text_color(cx.theme().muted_foreground))
                                            .child(Label::new(mac).text_xs())
                                    )
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .child(Label::new("登录时间").text_xs().text_color(cx.theme().muted_foreground))
                                            .child(Label::new(last_active).text_xs().text_color(cx.theme().muted_foreground))
                                    )
                            )
                            .child(
                                if let Some(host) = transfer_host {
                                    let entity_clone = state_entity.clone();
                                    let host_clone = host.clone();
                                    h_flex()
                                        .justify_between()
                                        .items_center()
                                        .p_2()
                                        .rounded_lg()
                                        .bg(cx.theme().background)
                                        .child(
                                            v_flex()
                                                .child(Label::new("局域网互传已就绪").font_semibold().text_xs().text_color(cx.theme().primary))
                                                .child(Label::new(host.clone()).text_xs().text_color(cx.theme().muted_foreground))
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!("transfer-{}", idx)))
                                                .primary()
                                                .small()
                                                .label("打开互传")
                                                .on_click(move |_, _, cx| {
                                                    entity_clone.update(cx, |state, cx| {
                                                        state.open_transfer_url(host_clone.clone(), cx);
                                                    });
                                                })
                                        )
                                        .into_any_element()
                                } else {
                                    div()
                                        .p_2()
                                        .rounded_lg()
                                        .bg(cx.theme().background)
                                        .child(Label::new("未探测到局域网互传服务，请确保设备与电脑在同一 Wi-Fi").text_xs().text_color(cx.theme().muted_foreground))
                                        .into_any_element()
                                }
                            )
                    }))
                    .into_any_element()
            }
        )
}
