#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Role,
    SharedString, Styled as _, Task, Window, WindowBounds, WindowOptions, div, px, rgb, size,
};
use gpui_component::{
    ActiveTheme as _, IndexPath, Root, Theme,
    list::{List, ListDelegate, ListItem, ListState},
    v_flex,
};
use palette_prototype_core::{PaletteState, Probe, ResultKind};

struct PaletteDelegate {
    state: PaletteState,
    probe: Probe,
}

impl ListDelegate for PaletteDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.state.set_query(query);
        self.probe.record("query", Some(self.state.query()));
        cx.notify();
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.state.items().len()
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let item = self.state.items().nth(index.row)?.to_owned();
        let kind_color = match item.kind {
            ResultKind::Command => rgb(0xf472b6),
            ResultKind::Application => rgb(0x60a5fa),
            ResultKind::File => rgb(0x34d399),
        };
        Some(
            ListItem::new(SharedString::from(item.id))
                .h(px(54.))
                .rounded(px(7.))
                .child(
                    gpui_base::Button::new(format!("{}.semantics", item.id))
                        .accessibility_label(format!(
                            "{}: {}. {}",
                            item.kind.label(),
                            item.title,
                            item.detail
                        ))
                        .role(Role::ListItem)
                        .tab_stop(false)
                        .focusable(false)
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .w(px(62.))
                                .text_xs()
                                .font_family("Consolas")
                                .text_color(kind_color)
                                .child(item.kind.label()),
                        )
                        .child(
                            v_flex()
                                .flex_grow(1.)
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().foreground)
                                        .child(item.title),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(item.detail),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_family("Consolas")
                                .text_color(cx.theme().muted_foreground)
                                .child(item.hint),
                        ),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        let selected = index
            .and_then(|index| self.state.items().nth(index.row))
            .map(|item| item.id);
        if let Some(selected) = selected {
            self.state.select(selected);
        }
        self.probe.record("selection", self.state.selected_id());
    }

    fn confirm(&mut self, _: bool, _: &mut Window, _: &mut Context<ListState<Self>>) {
        let activated = self.state.activate_selected();
        self.probe.record("activation", activated);
    }
}

struct PaletteView {
    list: Entity<ListState<PaletteDelegate>>,
}

impl PaletteView {
    fn new(window: &mut Window, cx: &mut Context<Self>, probe: Probe) -> Self {
        let delegate = PaletteDelegate {
            state: PaletteState::default(),
            probe: probe.clone(),
        };
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        let list_to_focus = list.clone();
        window.on_next_frame(move |_, _| {
            unsafe {
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            }
            probe.record_first_frame();
        });
        window.on_next_frame(move |window, cx| {
            list_to_focus.update(cx, |list, cx| list.focus(window, cx));
        });
        Self { list }
    }
}

impl Render for PaletteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .p_4()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .text_xs()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(0xf472b6))
                            .child("KOMOREBI"),
                    )
                    .child(div().mx_2().text_color(cx.theme().border).child("|"))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("COMMAND PALETTE"),
                    )
                    .child(div().flex_grow(1.))
                    .child(
                        div()
                            .font_family("Consolas")
                            .text_color(rgb(0xf472b6))
                            .child("LOCAL"),
                    ),
            )
            .child(
                div()
                    .flex_grow(1.)
                    .min_h_0()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().colors.list)
                    .overflow_hidden()
                    .child(
                        List::new(&self.list)
                            .search_placeholder("Search commands, apps, and files")
                            .scrollbar_visible(true)
                            .size_full()
                            .p_1(),
                    ),
            )
            .child(
                div()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .flex()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("↑ ↓ navigate    Enter open    Esc close")
                    .child(div().flex_grow(1.))
                    .child("LOCAL SOURCES"),
            )
    }
}

fn apply_theme(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.colors.background = rgb(0x0d0f16).into();
    theme.colors.foreground = rgb(0xffffff).into();
    theme.colors.list = rgb(0x161924).into();
    theme.colors.list_active = rgb(0x38253d).into();
    theme.colors.list_active_border = rgb(0xf472b6).into();
    theme.colors.list_hover = rgb(0x24283a).into();
    theme.colors.border = rgb(0x373c4e).into();
    theme.colors.muted_foreground = rgb(0x9197af).into();
    theme.colors.primary = rgb(0xf472b6).into();
    theme.colors.ring = rgb(0xf472b6).into();
    theme.radius = px(7.);
    theme.radius_lg = px(10.);
}

fn main() {
    let probe = Probe::from_env("gpui-components");
    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        apply_theme(cx);
        let bounds = WindowBounds::centered(size(px(720.), px(520.)), cx);
        let probe = probe.clone();
        cx.spawn(async move |cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(bounds),
                    titlebar: None,
                    is_resizable: true,
                    window_min_size: Some(size(px(520.), px(360.))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("Komorebi Palette Prototype - GPUI Components");
                    probe.record("window_created", None);
                    let view = cx.new(|cx| PaletteView::new(window, cx, probe.clone()));
                    cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
                },
            )
            .expect("prototype window opens");
        })
        .detach();
    });
}
