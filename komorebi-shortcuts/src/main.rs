use gpui::App;
use gpui::AppContext as _;
use gpui::Application;
use gpui::Bounds;
use gpui::Context;
use gpui::IntoElement;
use gpui::ParentElement as _;
use gpui::Render;
use gpui::SharedString;
use gpui::Styled as _;
use gpui::Subscription;
use gpui::Window;
use gpui::WindowBounds;
use gpui::WindowOptions;
use gpui::div;
use gpui::prelude::FluentBuilder as _;
use gpui::px;
use gpui::rgb;
use gpui::size;
use gpui_component::Root;
use gpui_component::input::Input;
use gpui_component::input::InputEvent;
use gpui_component::input::InputState;
use gpui_component::scroll::ScrollableElement as _;
use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionCategory;
use komorebi_protocol::RoleHint;
use komorebi_shell::SessionLifetime;
use komorebi_shell::ShellSession;
use komorebi_shell::ShortcutGuide;
use komorebi_shell::ShortcutGuideEntry;

const TITLE: &str = "komorebi shortcuts";

struct ShortcutGuideView {
    guide: ShortcutGuide,
    filter: gpui::Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl ShortcutGuideView {
    fn new(guide: ShortcutGuide, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search shortcuts, actions, or descriptions")
        });
        let _subscriptions =
            vec![
                cx.subscribe_in(&filter, window, |_, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                }),
            ];
        Self {
            guide,
            filter,
            _subscriptions,
        }
    }

    fn row(entry: &ShortcutGuideEntry) -> impl IntoElement {
        let availability = match entry.availability() {
            ActionAvailability::Available => "available",
            ActionAvailability::Unavailable(_) => "unavailable",
        };
        div()
            .flex()
            .items_center()
            .gap_4()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(rgb(0x2d_35_42))
            .child(
                div().w(px(180.0)).flex_none().child(
                    div()
                        .flex()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .bg(rgb(0x27_31_40))
                        .text_color(rgb(0xd8_e2_f0))
                        .child(entry.trigger().to_owned()),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(entry.title().to_owned()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x89_98_ac))
                                    .child(category_label(entry.category())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(match entry.availability() {
                                        ActionAvailability::Available => rgb(0x67_d3_91),
                                        ActionAvailability::Unavailable(_) => rgb(0xe0_8b_8b),
                                    })
                                    .child(availability),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xa8_b4_c4))
                            .child(entry.description().to_owned()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x72_80_92))
                            .child(entry.action_id().to_owned()),
                    ),
            )
    }
}

impl Render for ShortcutGuideView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query: SharedString = self.filter.read(cx).value();
        let rows = self
            .guide
            .search(query.as_ref())
            .map(Self::row)
            .collect::<Vec<_>>();
        let count = rows.len();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x12_17_1f))
            .text_color(rgb(0xeb_f0_f7))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_6()
                    .border_b_1()
                    .border_color(rgb(0x2d_35_42))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(TITLE),
                    )
                    .child(Input::new(&self.filter).cleanable(true)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(rows)
                    .when(count == 0, |view| {
                        view.child(
                            div()
                                .p_8()
                                .text_color(rgb(0x89_98_ac))
                                .child("No configured shortcuts match this search."),
                        )
                    }),
            )
    }
}

const fn category_label(category: ActionCategory) -> &'static str {
    match category {
        ActionCategory::Window => "window",
        ActionCategory::Workspace => "workspace",
        ActionCategory::Configuration => "configuration",
    }
}

fn run(guide: ShortcutGuide) {
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(880.0), px(620.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            },
            {
                let guide = guide.clone();
                move |window, cx| {
                    window.set_window_title(TITLE);
                    let view = cx.new(|cx| ShortcutGuideView::new(guide, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                }
            },
        );
        if let Err(error) = opened {
            eprintln!("could not open shortcuts window: {error}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = ShellSession::start(RoleHint::OwnerControl, SessionLifetime::OneShot)?;
    let ticket = session.handle().catalog_snapshot()?;
    let catalog = ticket.snapshot().await;
    let shutdown = session.shutdown().await;
    let guide = ShortcutGuide::project(&catalog?);
    shutdown?;
    run(guide);
    Ok(())
}
