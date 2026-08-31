#![warn(clippy::all)]

mod command;

use command::CommandQueue;
use command::CommandQueueError;
use eframe::egui;
use eframe::egui::Color32;
use eframe::egui::ViewportBuilder;
use eframe::egui::color_picker::Alpha;
use komorebi_client::BorderStyle;
use komorebi_client::Colour;
use komorebi_client::DefaultLayout;
use komorebi_client::GlobalState;
use komorebi_client::Layout;
use komorebi_client::Rect;
use komorebi_client::Rgb;
use komorebi_client::RuleDebug;
use komorebi_client::SocketMessage;
use komorebi_client::StackbarLabel;
use komorebi_client::StackbarMode;
use komorebi_client::State;
use komorebi_client::Window;
use komorebi_client::WindowKind;
use std::collections::HashMap;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

#[tokio::main]
async fn main() {
    let (commands, command_actor) = CommandQueue::start();
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_always_on_top()
            .with_inner_size([320.0, 500.0]),
        ..Default::default()
    };

    let gui = eframe::run_native(
        "komorebi-gui",
        native_options,
        Box::new(move |cc| Ok(Box::new(KomorebiGui::new(cc, commands)))),
    );
    if let Err(error) = command_actor.await {
        eprintln!("GUI command actor failed: {error}");
    }
    if let Err(error) = gui {
        eprintln!("GUI failed: {error}");
    }
}

struct BorderColours {
    single: Color32,
    stack: Color32,
    monocle: Color32,
    floating: Color32,
    unfocused: Color32,
    unfocused_locked: Color32,
}

struct BorderConfig {
    border_enabled: bool,
    border_colours: BorderColours,
    border_style: BorderStyle,
    border_offset: i32,
    border_width: i32,
}

struct StackbarConfig {
    mode: StackbarMode,
    label: StackbarLabel,
    height: i32,
    width: i32,
    focused_text_colour: Color32,
    unfocused_text_colour: Color32,
    background_colour: Color32,
}

struct MonitorConfig {
    size: Rect,
    work_area_offset: Rect,
    workspaces: Vec<WorkspaceConfig>,
}

impl From<&komorebi_client::Monitor> for MonitorConfig {
    fn from(value: &komorebi_client::Monitor) -> Self {
        let mut workspaces = vec![];
        for ws in value.workspaces() {
            workspaces.push(WorkspaceConfig::from(ws));
        }

        Self {
            size: value.size,
            work_area_offset: value.work_area_offset.unwrap_or_default(),
            workspaces,
        }
    }
}

struct WorkspaceConfig {
    name: String,
    tile: bool,
    layout: DefaultLayout,
    container_padding: i32,
    workspace_padding: i32,
}

impl From<&komorebi_client::Workspace> for WorkspaceConfig {
    fn from(value: &komorebi_client::Workspace) -> Self {
        let layout = match value.layout {
            Layout::Default(layout) => layout,
            Layout::Custom(_) => DefaultLayout::BSP,
        };

        let name = value
            .name
            .to_owned()
            .unwrap_or_else(|| random_word::get(random_word::Lang::En).to_string());

        Self {
            layout,
            name,
            tile: value.tile,
            workspace_padding: value.workspace_padding.unwrap_or(20),
            container_padding: value.container_padding.unwrap_or(20),
        }
    }
}

struct KomorebiGui {
    commands: CommandQueue,
    border_config: BorderConfig,
    stackbar_config: StackbarConfig,
    mouse_follows_focus: bool,
    monitors: Vec<MonitorConfig>,
    workspace_names: HashMap<usize, Vec<String>>,
    debug_hwnd: isize,
    debug_windows: Vec<Window>,
    debug_rule: Option<RuleDebug>,
}

fn colour32(colour: Option<Colour>) -> Color32 {
    match colour {
        Some(Colour::Rgb(rgb)) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        Some(Colour::Hex(hex)) => {
            let rgb = Rgb::from(hex);
            Color32::from_rgb(rgb.r, rgb.g, rgb.b)
        }
        None => Color32::from_rgb(0, 0, 0),
    }
}

impl KomorebiGui {
    fn new(_cc: &eframe::CreationContext<'_>, commands: CommandQueue) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        let global_state: GlobalState = serde_json::from_str(
            &komorebi_client::send_query(&SocketMessage::GlobalState).unwrap(),
        )
        .unwrap();

        let state: State =
            serde_json::from_str(&komorebi_client::send_query(&SocketMessage::State).unwrap())
                .unwrap();

        let border_colours = BorderColours {
            single: colour32(global_state.border_colours.single),
            stack: colour32(global_state.border_colours.stack),
            monocle: colour32(global_state.border_colours.monocle),
            floating: colour32(global_state.border_colours.floating),
            unfocused: colour32(global_state.border_colours.unfocused),
            unfocused_locked: colour32(global_state.border_colours.unfocused_locked),
        };

        let border_config = BorderConfig {
            border_enabled: global_state.border_enabled,
            border_colours,
            border_style: global_state.border_style,
            border_offset: global_state.border_offset.get(),
            border_width: global_state.border_width.get(),
        };

        let mut monitors = vec![];
        for m in state.monitors.elements() {
            monitors.push(MonitorConfig::from(m));
        }

        let mut workspace_names = HashMap::new();

        for (monitor_idx, m) in monitors.iter().enumerate() {
            for ws in &m.workspaces {
                let names = workspace_names.entry(monitor_idx).or_insert_with(Vec::new);
                names.push(ws.name.clone());
            }
        }

        let stackbar_config = StackbarConfig {
            mode: global_state.stackbar_mode,
            height: global_state.stackbar_height,
            width: global_state.stackbar_tab_width,
            label: global_state.stackbar_label,
            focused_text_colour: colour32(Some(global_state.stackbar_focused_text_colour)),
            unfocused_text_colour: colour32(Some(global_state.stackbar_unfocused_text_colour)),
            background_colour: colour32(Some(global_state.stackbar_tab_background_colour)),
        };

        let mut debug_windows = vec![];

        unsafe {
            EnumWindows(
                Some(enum_window),
                windows::Win32::Foundation::LPARAM(&mut debug_windows as *mut Vec<Window> as isize),
            )
            .unwrap();
        };

        Self {
            commands,
            border_config,
            mouse_follows_focus: state.mouse_follows_focus,
            monitors,
            workspace_names,
            debug_hwnd: 0,
            debug_windows,
            stackbar_config,
            debug_rule: None,
        }
    }
}

const fn rgb(colour: Color32) -> Rgb {
    Rgb::new(colour.r(), colour.g(), colour.b())
}

fn report_command(result: Result<(), CommandQueueError>) {
    if let Err(error) = result {
        eprintln!("could not queue GUI command: {error}");
    }
}

extern "system" fn enum_window(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows_core::BOOL {
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<Window>) };
    let window = Window::from(hwnd.0 as isize);

    if window.is_window()
        && !window.is_miminized()
        && window.is_visible()
        && window.title().is_ok()
        && window.exe().is_ok()
    {
        windows.push(window);
    }

    true.into()
}

fn json_view_ui(ui: &mut egui::Ui, code: &str) {
    let language = "json";
    let theme =
        egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), &ui.ctx().style());
    egui_extras::syntax_highlighting::code_view_ui(ui, &theme, code, language);
}

impl eframe::App for KomorebiGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ctx.set_pixels_per_point(2.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_width(ctx.content_rect().width());
                ui.collapsing("Debugging", |ui| {
                    ui.collapsing("Window Rules", |ui| {
                        let window = Window::from(self.debug_hwnd);

                        let label = if let (Ok(title), Ok(exe)) = (window.title(), window.exe()) {
                            format!("{title} ({exe})")
                        } else {
                            String::from("Select a Window")
                        };

                        if ui.button("Refresh Windows").clicked() {
                            let mut debug_windows = vec![];

                            unsafe {
                                EnumWindows(
                                    Some(enum_window),
                                    windows::Win32::Foundation::LPARAM(
                                        &mut debug_windows as *mut Vec<Window> as isize,
                                    ),
                                )
                                .unwrap();
                            };

                            self.debug_windows = debug_windows;
                        }

                        egui::ComboBox::from_label("Select a Window")
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                for w in &self.debug_windows {
                                    if ui
                                        .selectable_value(
                                            &mut self.debug_hwnd,
                                            w.hwnd,
                                            format!(
                                                "{} ({})",
                                                w.title().unwrap(),
                                                w.exe().unwrap()
                                            ),
                                        )
                                        .changed()
                                    {
                                        let debug_rule: RuleDebug = serde_json::from_str(
                                            &komorebi_client::send_query(
                                                &SocketMessage::DebugWindow(self.debug_hwnd),
                                            )
                                            .unwrap(),
                                        )
                                        .unwrap();

                                        self.debug_rule = Some(debug_rule)
                                    }
                                }
                            });

                        if let Some(debug_rule) = &self.debug_rule {
                            json_view_ui(ui, &serde_json::to_string_pretty(debug_rule).unwrap())
                        }
                    });
                });

                ui.collapsing("Mouse", |ui| {
                    if ui
                        .toggle_value(&mut self.mouse_follows_focus, "Mouse Follows Focus")
                        .changed()
                    {
                        komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                            self.mouse_follows_focus,
                        ))
                        .unwrap();
                    }
                });

                ui.collapsing("Border", |ui| {
                    if ui
                        .toggle_value(&mut self.border_config.border_enabled, "Border")
                        .changed()
                    {
                        report_command(
                            self.commands
                                .set_border_enabled(self.border_config.border_enabled),
                        );
                    }

                    ui.collapsing("Colours", |ui| {
                        ui.collapsing("Single", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.single,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_border_colour(
                                    WindowKind::Single,
                                    rgb(self.border_config.border_colours.single),
                                ));
                            }
                        });

                        ui.collapsing("Stack", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.stack,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_border_colour(
                                    WindowKind::Stack,
                                    rgb(self.border_config.border_colours.stack),
                                ));
                            }
                        });

                        ui.collapsing("Monocle", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.monocle,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_border_colour(
                                    WindowKind::Monocle,
                                    rgb(self.border_config.border_colours.monocle),
                                ));
                            }
                        });

                        ui.collapsing("Floating", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.floating,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_border_colour(
                                    WindowKind::Floating,
                                    rgb(self.border_config.border_colours.floating),
                                ));
                            }
                        });

                        ui.collapsing("Unfocused", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.unfocused,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_border_colour(
                                    WindowKind::Unfocused,
                                    rgb(self.border_config.border_colours.unfocused),
                                ));
                            }
                        });

                        ui.collapsing("Unfocused Locked", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.border_config.border_colours.unfocused_locked,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_border_colour(
                                    WindowKind::UnfocusedLocked,
                                    rgb(self.border_config.border_colours.unfocused_locked),
                                ));
                            }
                        })
                    });

                    ui.collapsing("Style", |ui| {
                        for option in [
                            BorderStyle::System,
                            BorderStyle::Rounded,
                            BorderStyle::Square,
                        ] {
                            if ui
                                .add(egui::Button::selectable(
                                    self.border_config.border_style == option,
                                    option.to_string(),
                                ))
                                .clicked()
                            {
                                self.border_config.border_style = option;
                                report_command(
                                    self.commands
                                        .set_border_style(self.border_config.border_style),
                                );
                            }
                        }
                    });

                    ui.collapsing("Width", |ui| {
                        if ui
                            .add(egui::Slider::new(
                                &mut self.border_config.border_width,
                                -50..=50,
                            ))
                            .changed()
                        {
                            report_command(
                                self.commands
                                    .set_border_width(self.border_config.border_width),
                            );
                        };
                    });

                    ui.collapsing("Offset", |ui| {
                        if ui
                            .add(egui::Slider::new(
                                &mut self.border_config.border_offset,
                                -50..=50,
                            ))
                            .changed()
                        {
                            report_command(
                                self.commands
                                    .set_border_offset(self.border_config.border_offset),
                            );
                        };
                    });
                });

                ui.collapsing("Stackbar", |ui| {
                    for option in [
                        StackbarMode::Never,
                        StackbarMode::OnStack,
                        StackbarMode::Always,
                    ] {
                        if ui
                            .add(egui::Button::selectable(
                                self.stackbar_config.mode == option,
                                option.to_string(),
                            ))
                            .clicked()
                        {
                            self.stackbar_config.mode = option;
                            report_command(
                                self.commands.set_stackbar_mode(self.stackbar_config.mode),
                            );
                        }
                    }

                    ui.collapsing("Label", |ui| {
                        for option in [StackbarLabel::Process, StackbarLabel::Title] {
                            if ui
                                .add(egui::Button::selectable(
                                    self.stackbar_config.label == option,
                                    option.to_string(),
                                ))
                                .clicked()
                            {
                                self.stackbar_config.label = option;
                                report_command(
                                    self.commands.set_stackbar_label(self.stackbar_config.label),
                                );
                            }
                        }
                    });

                    ui.collapsing("Colours", |ui| {
                        ui.collapsing("Focused Text", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.stackbar_config.focused_text_colour,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_stackbar_focused_text_colour(
                                    rgb(self.stackbar_config.focused_text_colour),
                                ));
                            }
                        });

                        ui.collapsing("Unfocused Text", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.stackbar_config.unfocused_text_colour,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_stackbar_unfocused_text_colour(
                                    rgb(self.stackbar_config.unfocused_text_colour),
                                ));
                            }
                        });

                        ui.collapsing("Background", |ui| {
                            if egui::color_picker::color_picker_color32(
                                ui,
                                &mut self.stackbar_config.background_colour,
                                Alpha::Opaque,
                            ) {
                                report_command(self.commands.set_stackbar_background_colour(rgb(
                                    self.stackbar_config.background_colour,
                                )));
                            }
                        })
                    });

                    ui.collapsing("Width", |ui| {
                        if ui
                            .add(egui::Slider::new(&mut self.stackbar_config.width, 0..=500))
                            .drag_stopped()
                        {
                            report_command(
                                self.commands
                                    .set_stackbar_tab_width(self.stackbar_config.width),
                            );
                        };
                    });

                    ui.collapsing("Height", |ui| {
                        if ui
                            .add(egui::Slider::new(&mut self.stackbar_config.height, 0..=100))
                            .drag_stopped()
                        {
                            report_command(
                                self.commands
                                    .set_stackbar_height(self.stackbar_config.height),
                            );
                        };
                    });
                });

                for (monitor_idx, monitor) in self.monitors.iter_mut().enumerate() {
                    ui.collapsing(
                        format!(
                            "Monitor {monitor_idx} ({}x{})",
                            monitor.size.right, monitor.size.bottom
                        ),
                        |ui| {
                            ui.collapsing("Work Area Offset", |ui| {
                                let changed = ui
                                    .add(
                                        egui::Slider::new(
                                            &mut monitor.work_area_offset.left,
                                            0..=500,
                                        )
                                        .text("Left"),
                                    )
                                    .drag_stopped()
                                    | ui.add(
                                        egui::Slider::new(
                                            &mut monitor.work_area_offset.top,
                                            0..=500,
                                        )
                                        .text("Top"),
                                    )
                                    .drag_stopped()
                                    | ui.add(
                                        egui::Slider::new(
                                            &mut monitor.work_area_offset.right,
                                            0..=500,
                                        )
                                        .text("Right"),
                                    )
                                    .drag_stopped()
                                    | ui.add(
                                        egui::Slider::new(
                                            &mut monitor.work_area_offset.bottom,
                                            0..=500,
                                        )
                                        .text("Bottom"),
                                    )
                                    .drag_stopped();

                                if changed {
                                    report_command(self.commands.set_monitor_work_area_offset(
                                        monitor_idx,
                                        monitor.work_area_offset,
                                    ));
                                }
                            });

                            ui.collapsing("Workspaces", |ui| {
                                for (workspace_idx, workspace) in
                                    monitor.workspaces.iter_mut().enumerate()
                                {
                                    ui.collapsing(
                                        format!("Workspace {workspace_idx} ({})", workspace.name),
                                        |ui| {
                                            if ui.button("Focus").clicked() {
                                                report_command(
                                                    self.commands.focus_monitor_workspace(
                                                        monitor_idx,
                                                        workspace_idx,
                                                    ),
                                                );
                                            }

                                            if ui
                                                .toggle_value(&mut workspace.tile, "Tiling")
                                                .changed()
                                            {
                                                report_command(self.commands.set_workspace_tiling(
                                                    monitor_idx,
                                                    workspace_idx,
                                                    workspace.tile,
                                                ));
                                            }

                                            ui.collapsing("Name", |ui| {
                                                let monitor_workspaces = self
                                                    .workspace_names
                                                    .get_mut(&monitor_idx)
                                                    .unwrap();
                                                let workspace_name =
                                                    &mut monitor_workspaces[workspace_idx];
                                                if ui
                                                    .text_edit_singleline(workspace_name)
                                                    .lost_focus()
                                                {
                                                    workspace.name.clone_from(workspace_name);
                                                    report_command(
                                                        self.commands.set_workspace_name(
                                                            monitor_idx,
                                                            workspace_idx,
                                                            &workspace.name,
                                                        ),
                                                    );
                                                }
                                            });

                                            ui.collapsing("Layout", |ui| {
                                                for option in [
                                                    DefaultLayout::BSP,
                                                    DefaultLayout::Columns,
                                                    DefaultLayout::Rows,
                                                    DefaultLayout::VerticalStack,
                                                    DefaultLayout::HorizontalStack,
                                                    DefaultLayout::UltrawideVerticalStack,
                                                    DefaultLayout::Grid,
                                                ] {
                                                    if ui
                                                        .add(egui::Button::selectable(
                                                            workspace.layout == option,
                                                            option.to_string(),
                                                        ))
                                                        .clicked()
                                                    {
                                                        workspace.layout = option;
                                                        report_command(
                                                            self.commands.set_workspace_layout(
                                                                monitor_idx,
                                                                workspace_idx,
                                                                workspace.layout,
                                                            ),
                                                        );
                                                    }
                                                }
                                            });

                                            ui.collapsing("Container Padding", |ui| {
                                                if ui
                                                    .add(egui::Slider::new(
                                                        &mut workspace.container_padding,
                                                        0..=100,
                                                    ))
                                                    .drag_stopped()
                                                {
                                                    report_command(
                                                        self.commands.set_container_padding(
                                                            monitor_idx,
                                                            workspace_idx,
                                                            workspace.container_padding,
                                                        ),
                                                    );
                                                };
                                            });

                                            ui.collapsing("Workspace Padding", |ui| {
                                                if ui
                                                    .add(egui::Slider::new(
                                                        &mut workspace.workspace_padding,
                                                        0..=100,
                                                    ))
                                                    .drag_stopped()
                                                {
                                                    report_command(
                                                        self.commands.set_workspace_padding(
                                                            monitor_idx,
                                                            workspace_idx,
                                                            workspace.workspace_padding,
                                                        ),
                                                    );
                                                };
                                            });
                                        },
                                    );
                                }
                            });
                        },
                    );
                }
            });
        });
    }
}
