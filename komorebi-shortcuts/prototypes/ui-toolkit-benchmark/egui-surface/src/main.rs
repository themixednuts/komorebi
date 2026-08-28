#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, CornerRadius, FontId, RichText, Stroke, ViewportBuilder};
use palette_prototype_core::{PaletteState, Probe, ResultKind};

const ACCENT: Color32 = Color32::from_rgb(244, 114, 182);
const BACKGROUND: Color32 = Color32::from_rgb(13, 15, 22);
const PANEL: Color32 = Color32::from_rgb(22, 25, 36);
const SELECTED: Color32 = Color32::from_rgb(56, 37, 61);
const BORDER: Color32 = Color32::from_rgb(55, 60, 78);
const MUTED: Color32 = Color32::from_rgb(145, 151, 175);

struct PaletteApp {
    state: PaletteState,
    probe: Probe,
    frame_count: u8,
    input_focused: bool,
}

impl PaletteApp {
    fn new(context: &eframe::CreationContext<'_>, probe: Probe) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = BACKGROUND;
        visuals.extreme_bg_color = PANEL;
        visuals.faint_bg_color = PANEL;
        visuals.selection.bg_fill = SELECTED;
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        context.egui_ctx.set_visuals(visuals);
        context.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(8.0, 7.0);
            style.spacing.button_padding = egui::vec2(10.0, 8.0);
        });
        probe.record("window_created", None);
        Self {
            state: PaletteState::default(),
            probe,
            frame_count: 0,
            input_focused: false,
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (up, down, enter, escape) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::ArrowUp),
                input.key_pressed(egui::Key::ArrowDown),
                input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if up {
            self.state.move_selection(-1);
            self.probe.record("selection", self.state.selected_id());
        }
        if down {
            self.state.move_selection(1);
            self.probe.record("selection", self.state.selected_id());
        }
        if enter {
            let activated = self.state.activate_selected();
            self.probe.record("activation", activated);
        }
        if escape {
            if self.state.query().is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.state.set_query("");
                self.probe.record("query", Some(""));
            }
        }
    }
}

impl eframe::App for PaletteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_count = self.frame_count.saturating_add(1);
        if self.frame_count == 1 {
            ctx.request_repaint();
        } else if self.frame_count == 2 {
            unsafe {
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            }
            self.probe.record_first_frame();
        }

        self.handle_keys(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(18.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("KOMOREBI").color(ACCENT).strong().size(12.0));
                    ui.separator();
                    ui.label(RichText::new("COMMAND PALETTE").color(MUTED).size(11.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("LOCAL").color(ACCENT).monospace().size(10.0));
                    });
                });
                ui.add_space(10.0);

                let mut query = self.state.query().to_owned();
                let response = egui::Frame::new()
                    .fill(PANEL)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut query)
                                .hint_text("Search commands, apps, and files")
                                .font(FontId::proportional(17.0))
                                .frame(false)
                                .desired_width(f32::INFINITY),
                        )
                    })
                    .inner;
                if !self.input_focused {
                    response.request_focus();
                    self.input_focused = true;
                }
                if query != self.state.query() {
                    self.state.set_query(query);
                    self.probe.record("query", Some(self.state.query()));
                }

                ui.add_space(8.0);
                let rows = self.state.items().copied().collect::<Vec<_>>();
                let results_height = (ui.available_height() - 42.0).max(120.0);
                egui::ScrollArea::vertical()
                    .max_height(results_height)
                    .show(ui, |ui| {
                        for item in rows {
                            let selected = self.state.selected_id() == Some(item.id);
                            let fill = if selected {
                                SELECTED
                            } else {
                                Color32::TRANSPARENT
                            };
                            let row = egui::Frame::new()
                                .fill(fill)
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if selected {
                                        ACCENT
                                    } else {
                                        Color32::TRANSPARENT
                                    },
                                ))
                                .corner_radius(CornerRadius::same(7))
                                .inner_margin(egui::Margin::symmetric(11, 8))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let kind_color = match item.kind {
                                            ResultKind::Command => ACCENT,
                                            ResultKind::Application => {
                                                Color32::from_rgb(96, 165, 250)
                                            }
                                            ResultKind::File => Color32::from_rgb(52, 211, 153),
                                        };
                                        ui.label(
                                            RichText::new(item.kind.label())
                                                .color(kind_color)
                                                .monospace()
                                                .size(9.0),
                                        );
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(item.title)
                                                    .color(Color32::WHITE)
                                                    .size(14.0),
                                            );
                                            ui.label(
                                                RichText::new(item.detail).color(MUTED).size(11.0),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(item.hint)
                                                        .color(MUTED)
                                                        .monospace()
                                                        .size(10.0),
                                                );
                                            },
                                        );
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());
                            if row.hovered() {
                                self.state.select(item.id);
                            }
                            if row.clicked() {
                                self.state.select(item.id);
                                let activated = self.state.activate_selected();
                                self.probe.record("activation", activated);
                            }
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("↑ ↓ navigate    Enter open    Esc close")
                            .color(MUTED)
                            .size(10.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let count = self.state.items().len();
                        ui.label(
                            RichText::new(format!("{count} results"))
                                .color(MUTED)
                                .size(10.0),
                        );
                    });
                });
                if let Some(id) = self.state.activated_id() {
                    ui.label(
                        RichText::new(format!("Accepted {id}"))
                            .color(ACCENT)
                            .size(11.0),
                    );
                }
            });
    }
}

fn main() -> eframe::Result {
    let probe = Probe::from_env("egui");
    let viewport = ViewportBuilder::default()
        .with_title("Komorebi Palette Prototype - egui")
        .with_inner_size([720.0, 520.0])
        .with_min_inner_size([520.0, 360.0])
        .with_decorations(false)
        .with_resizable(true);
    eframe::run_native(
        "Komorebi Palette Prototype - egui",
        eframe::NativeOptions {
            viewport,
            centered: true,
            ..Default::default()
        },
        Box::new(move |context| Ok(Box::new(PaletteApp::new(context, probe)))),
    )
}
