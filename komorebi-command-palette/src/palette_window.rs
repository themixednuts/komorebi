use std::future::Future;

use gpui::App;
use gpui::AppContext as _;
use gpui::Application;
use gpui::Bounds;
use gpui::Context;
use gpui::InteractiveElement as _;
use gpui::IntoElement;
use gpui::ParentElement as _;
use gpui::Render;
use gpui::Rgba;
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
use komorebi_search::FileSearchMatch;
use komorebi_shell::CommandPalette;
use komorebi_shell::FileActivationClient;
use komorebi_shell::PaletteAction;
use komorebi_shell::PaletteContent;
use komorebi_shell::PaletteController;
use komorebi_shell::PaletteEffect;
use komorebi_shell::PaletteFailure;
use komorebi_shell::PaletteFileSearch;
use komorebi_shell::PaletteFileSearchBroker;
use komorebi_shell::PaletteSelectionMove;
use komorebi_shell::PaletteStatus;
use komorebi_shell::PaletteSubmission;
use komorebi_shell::ShellHandle;
use komorebi_shell::WebSearchBroker;

const TITLE: &str = "komorebi command palette";

struct CommandPaletteView {
    controller: PaletteController,
    query: gpui::Entity<InputState>,
    shell: ShellHandle,
    web: WebSearchBroker,
    file_search: PaletteFileSearchBroker,
    file_activation: FileActivationClient,
    invocation: Option<Task<()>>,
    query_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl CommandPaletteView {
    fn new(
        palette: CommandPalette,
        shell: ShellHandle,
        web: WebSearchBroker,
        file_search: PaletteFileSearchBroker,
        file_activation: FileActivationClient,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let controller = PaletteController::new(palette);
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search commands"));
        let input_subscription = cx.subscribe_in(
            &query,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let search = this
                        .controller
                        .update_query(input.read(cx).value().as_ref());
                    if let Some(search) = search {
                        this.search_files(search, cx);
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.activate_selected(window, cx),
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
        query.update(cx, |query, cx| query.focus(window, cx));
        Self {
            controller,
            query,
            shell,
            web,
            file_search,
            file_activation,
            invocation: None,
            query_task: None,
            _subscriptions: vec![input_subscription],
        }
    }

    fn move_selection(
        &mut self,
        movement: PaletteSelectionMove,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.controller.move_selection(movement);
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
        if self.controller.select_position(position) {
            self.activate_selected(window, cx);
        }
    }

    fn activate_selected(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(effect) = self.controller.activate() else {
            cx.notify();
            return;
        };
        match effect {
            PaletteEffect::Invoke(invocation) => {
                let submission = invocation.submit(&self.shell);
                self.observe_submission(std::future::ready(submission), cx);
            }
            PaletteEffect::Web(invocation) => {
                let web = self.web.clone();
                self.observe_submission(async move { invocation.submit(&web).await }, cx);
            }
            PaletteEffect::File(invocation) => {
                let files = self.file_activation.clone();
                self.observe_submission(async move { invocation.submit(&files).await }, cx);
            }
        }
        cx.notify();
    }

    fn observe_submission(
        &mut self,
        submission: impl Future<Output = PaletteSubmission> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.invocation = Some(cx.spawn(async move |this, cx| {
            let completion = submission.await.complete().await;
            _ = this.update(cx, |this, cx| {
                _ = this.controller.complete(completion);
                cx.notify();
            });
        }));
    }

    fn search_files(&mut self, search: PaletteFileSearch, cx: &mut Context<Self>) {
        let files = self.file_search.clone();
        self.query_task = Some(cx.spawn(async move |this, cx| {
            let completion = search.submit(&files).await;
            _ = this.update(cx, |this, cx| {
                _ = this.controller.complete_file_search(completion);
                cx.notify();
            });
        }));
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

    fn file_row(
        position: usize,
        file: &FileSearchMatch,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id(("palette-file", position))
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
                    .child("file"),
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
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(file.display_path().to_owned()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x6f_7d_90))
                            .child(format!("match score {}", file.score())),
                    ),
            )
            .into_any_element()
    }
}

impl Render for CommandPaletteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.controller.selected_position();
        let (web_prompt, web_terms) = match self.controller.content() {
            PaletteContent::Actions => (false, None),
            PaletteContent::WebPrompt => (true, None),
            PaletteContent::WebSearch(request) => (false, Some(request.terms().to_owned())),
        };
        let no_results = !web_prompt && web_terms.is_none() && self.controller.is_empty();
        let mut rows = self
            .controller
            .actions()
            .enumerate()
            .map(|(position, action)| Self::row(position, action, selected == Some(position), cx))
            .collect::<Vec<_>>();
        let action_count = rows.len();
        rows.extend(self.controller.files().enumerate().map(|(offset, file)| {
            let position = action_count + offset;
            Self::file_row(position, file, selected == Some(position), cx)
        }));
        let status = status_message(self.controller.status());

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
                    .when_some(web_terms, |list, terms| {
                        list.child(
                            div()
                                .id("palette-web-search")
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_4()
                                .py_3()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.activate_selected(window, cx);
                                }))
                                .child(
                                    div()
                                        .w(px(96.0))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(rgb(0x87_98_ad))
                                        .child("web"),
                                )
                                .child(format!("Search the web for “{terms}”")),
                        )
                    })
                    .when(web_prompt, |list| {
                        list.child(
                            div()
                                .p_8()
                                .text_color(rgb(0x87_98_ad))
                                .child("Type search terms after !"),
                        )
                    })
                    .when(no_results, |list| {
                        list.child(
                            div()
                                .p_8()
                                .text_color(rgb(0x87_98_ad))
                                .child("No actions or files match this search."),
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

fn status_message(status: &PaletteStatus) -> Option<(String, Rgba)> {
    match status {
        PaletteStatus::Idle => None,
        PaletteStatus::RequiresInput { action, parameters } => Some((
            format!(
                "{action} requires {} argument{}",
                parameters.len(),
                if parameters.len() == 1 { "" } else { "s" }
            ),
            rgb(0xe4_8f_8f),
        )),
        PaletteStatus::Unavailable { action, reason } => Some((
            format!("{action} is unavailable: {reason:?}"),
            rgb(0xe4_8f_8f),
        )),
        PaletteStatus::Submitting { label, .. } => {
            Some((format!("Running {label}…"), rgb(0x8f_b9_ec)))
        }
        PaletteStatus::Succeeded { label } => Some((format!("Finished {label}"), rgb(0x6f_d9_9a))),
        PaletteStatus::Failed { label, failure } => Some((
            match failure {
                PaletteFailure::Submission(error) => format!("Could not queue {label}: {error}"),
                PaletteFailure::Rejected(reason) => {
                    format!("{label} was rejected: {reason:?}")
                }
                PaletteFailure::Execution(error) => format!("{label} failed: {error}"),
                PaletteFailure::WebSubmission(error) => {
                    format!("Could not queue web search: {error}")
                }
                PaletteFailure::WebCompletion(error) => {
                    format!("Web search stopped before completion: {error}")
                }
                PaletteFailure::WebLaunch(error) => {
                    format!("Windows could not open web search: {error}")
                }
                PaletteFailure::WebRejected => "Windows declined the web-search launch.".to_owned(),
                PaletteFailure::WebUnavailable => "Web search is not configured yet.".to_owned(),
                PaletteFailure::FileSubmission(error) => {
                    format!("Could not queue file activation: {error}")
                }
                PaletteFailure::FileCompletion(error) => {
                    format!("File activation stopped before completion: {error}")
                }
                PaletteFailure::FileActivation(error) => {
                    format!("Windows could not open {label}: {error}")
                }
            },
            rgb(0xe4_8f_8f),
        )),
        PaletteStatus::AttemptIdsExhausted => Some((
            "The palette exhausted its invocation identity space.".to_owned(),
            rgb(0xe4_8f_8f),
        )),
    }
}

const fn category_label(category: ActionCategory) -> &'static str {
    match category {
        ActionCategory::Window => "window",
        ActionCategory::Workspace => "workspace",
        ActionCategory::Configuration => "configuration",
    }
}

pub(crate) fn run_palette(
    palette: CommandPalette,
    shell: ShellHandle,
    web: WebSearchBroker,
    file_search: PaletteFileSearchBroker,
    file_activation: FileActivationClient,
) {
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
                let web = web.clone();
                let file_search = file_search.clone();
                let file_activation = file_activation.clone();
                move |window, cx| {
                    window.set_window_title(TITLE);
                    let view = cx.new(|cx| {
                        CommandPaletteView::new(
                            palette,
                            shell,
                            web,
                            file_search,
                            file_activation,
                            window,
                            cx,
                        )
                    });
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
