#![windows_subsystem = "windows"]

use gpui::App;
use gpui::AppContext as _;
use gpui::Application;
use gpui::Bounds;
use gpui::Context;
use gpui::InteractiveElement as _;
use gpui::IntoElement;
use gpui::ParentElement as _;
use gpui::Render;
use gpui::SharedString;
use gpui::StatefulInteractiveElement as _;
use gpui::Styled as _;
use gpui::Subscription;
use gpui::Task;
use gpui::Window;
use gpui::WindowBounds;
use gpui::WindowDecorations;
use gpui::WindowKind;
use gpui::WindowOptions;
use gpui::div;
use gpui::prelude::FluentBuilder as _;
use gpui::px;
use gpui::rgb;
use gpui::size;
use gpui_component::Root;
use gpui_component::input::Escape;
use gpui_component::input::Input;
use gpui_component::input::InputEvent;
use gpui_component::input::InputState;
use gpui_component::input::MoveDown;
use gpui_component::input::MoveUp;
use gpui_component::scroll::ScrollableElement as _;
use komorebi_protocol::ActionAvailability;
use komorebi_protocol::ActionCategory;
use komorebi_protocol::InvocationSubmissionReply;
use komorebi_protocol::RoleHint;
use komorebi_shell::CommandPalette;
use komorebi_shell::PaletteAction;
use komorebi_shell::PaletteActionState;
use komorebi_shell::PaletteMatches;
use komorebi_shell::PaletteSelectionMove;
use komorebi_shell::SessionLifetime;
use komorebi_shell::ShellHandle;
use komorebi_shell::ShellSession;

const TITLE: &str = "komorebi command palette";

#[derive(Clone, Debug)]
enum ActivationStatus {
    Idle,
    Submitting { action: Box<str> },
    Success { message: SharedString },
    Failure { message: SharedString },
}

struct CommandPaletteView {
    palette: CommandPalette,
    matches: PaletteMatches,
    query: gpui::Entity<InputState>,
    shell: ShellHandle,
    status: ActivationStatus,
    invocation: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl CommandPaletteView {
    fn new(
        palette: CommandPalette,
        shell: ShellHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let matches = palette.search("");
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search commands"));
        let input_subscription = cx.subscribe_in(
            &query,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.matches = this.palette.search(input.read(cx).value().as_ref());
                    this.status = ActivationStatus::Idle;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.activate_selected(window, cx),
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
        query.update(cx, |query, cx| query.focus(window, cx));
        Self {
            palette,
            matches,
            query,
            shell,
            status: ActivationStatus::Idle,
            invocation: None,
            _subscriptions: vec![input_subscription],
        }
    }

    fn move_selection(
        &mut self,
        movement: PaletteSelectionMove,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.matches.move_selection(movement);
        cx.notify();
    }

    fn select_previous(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(PaletteSelectionMove::Previous, window, cx);
    }

    fn select_next(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(PaletteSelectionMove::Next, window, cx);
    }

    fn select_and_activate(
        &mut self,
        position: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.matches.select_position(position) {
            self.activate_selected(window, cx);
        }
    }

    fn activate_selected(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.status, ActivationStatus::Submitting { .. }) {
            return;
        }
        let Some(action) = self.matches.selected(&self.palette) else {
            return;
        };
        let title = action.title().to_owned().into_boxed_str();
        match action.state() {
            PaletteActionState::Unavailable(reason) => {
                self.status = ActivationStatus::Failure {
                    message: format!("Command unavailable: {reason:?}").into(),
                };
                cx.notify();
            }
            PaletteActionState::RequiresInput(parameters) => {
                self.status = ActivationStatus::Failure {
                    message: format!(
                        "{} requires {} argument{}",
                        action.title(),
                        parameters.len(),
                        if parameters.len() == 1 { "" } else { "s" }
                    )
                    .into(),
                };
                cx.notify();
            }
            PaletteActionState::Ready(binding) => match self.shell.invoke_binding(binding) {
                Err(error) => {
                    self.status = ActivationStatus::Failure {
                        message: error.to_string().into(),
                    };
                    cx.notify();
                }
                Ok(ticket) => {
                    self.status = ActivationStatus::Submitting {
                        action: title.clone(),
                    };
                    self.invocation = Some(cx.spawn(async move |this, cx| {
                        let result = ticket.outcome().await;
                        _ = this.update(cx, |this, cx| {
                            this.status = match result {
                                Ok(
                                    InvocationSubmissionReply::Accepted(_)
                                    | InvocationSubmissionReply::Retained(_),
                                ) => ActivationStatus::Success {
                                    message: format!("Ran {title}").into(),
                                },
                                Ok(InvocationSubmissionReply::Rejected(reason)) => {
                                    ActivationStatus::Failure {
                                        message: format!("Command rejected: {reason:?}").into(),
                                    }
                                }
                                Err(error) => ActivationStatus::Failure {
                                    message: error.to_string().into(),
                                },
                            };
                            cx.notify();
                        });
                    }));
                    cx.notify();
                }
            },
        }
    }

    fn row(
        position: usize,
        action: &PaletteAction,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let availability = match action.availability() {
            ActionAvailability::Available => "ready",
            ActionAvailability::Unavailable(_) => "unavailable",
        };
        div()
            .id(("palette-action", position))
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(rgb(0x28_31_3d))
            .when(selected, |row| row.bg(rgb(0x22_30_42)))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_and_activate(position, window, cx);
            }))
            .child(
                div()
                    .w(px(96.0))
                    .flex_none()
                    .text_xs()
                    .text_color(rgb(0x87_98_ad))
                    .child(category_label(action.category())),
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
                                    .child(action.title().to_owned()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(match action.availability() {
                                        ActionAvailability::Available => rgb(0x6f_d9_9a),
                                        ActionAvailability::Unavailable(_) => rgb(0xe4_8f_8f),
                                    })
                                    .child(availability),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xa9_b6_c8))
                            .child(action.description().to_owned()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6f_7d_90))
                            .child(action.action_id().to_owned()),
                    ),
            )
            .into_any_element()
    }
}

impl Render for CommandPaletteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.matches.selected_position();
        let rows = self
            .matches
            .actions(&self.palette)
            .enumerate()
            .map(|(position, action)| Self::row(position, action, selected == Some(position), cx))
            .collect::<Vec<_>>();
        let status = match &self.status {
            ActivationStatus::Idle => None,
            ActivationStatus::Submitting { action } => {
                Some((format!("Running {action}…"), rgb(0x8f_b9_ec)))
            }
            ActivationStatus::Success { message } => Some((message.to_string(), rgb(0x6f_d9_9a))),
            ActivationStatus::Failure { message } => Some((message.to_string(), rgb(0xe4_8f_8f))),
        };

        div()
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_next))
            .on_action(|_: &Escape, window, _| window.remove_window())
            .flex()
            .flex_col()
            .size_full()
            .rounded_xl()
            .overflow_hidden()
            .shadow_2xl()
            .bg(rgb(0x10_16_1e))
            .text_color(rgb(0xec_f2_f9))
            .child(
                div()
                    .p_4()
                    .border_b_1()
                    .border_color(rgb(0x28_31_3d))
                    .child(Input::new(&self.query).cleanable(true)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(rows)
                    .when(self.matches.is_empty(), |list| {
                        list.child(
                            div()
                                .p_8()
                                .text_color(rgb(0x87_98_ad))
                                .child("No commands match this search."),
                        )
                    }),
            )
            .when_some(status, |view, (message, color)| {
                view.child(
                    div()
                        .px_4()
                        .py_3()
                        .border_t_1()
                        .border_color(rgb(0x28_31_3d))
                        .text_sm()
                        .text_color(color)
                        .child(message),
                )
            })
    }
}

const fn category_label(category: ActionCategory) -> &'static str {
    match category {
        ActionCategory::Window => "window",
        ActionCategory::Workspace => "workspace",
        ActionCategory::Configuration => "configuration",
    }
}

fn run_palette(palette: CommandPalette, shell: ShellHandle) {
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(760.0), px(560.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                kind: WindowKind::PopUp,
                is_movable: false,
                window_decorations: Some(WindowDecorations::Client),
                ..WindowOptions::default()
            },
            {
                let palette = palette.clone();
                let shell = shell.clone();
                move |window, cx| {
                    window.set_window_title(TITLE);
                    let view = cx.new(|cx| CommandPaletteView::new(palette, shell, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                }
            },
        );
        if opened.is_err() {
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = ShellSession::start(RoleHint::OwnerControl, SessionLifetime::Persistent)?;
    let catalog = session.handle().catalog_snapshot()?.snapshot().await?;
    let palette = CommandPalette::project(&catalog);
    run_palette(palette, session.handle());
    session.shutdown().await?;
    Ok(())
}
