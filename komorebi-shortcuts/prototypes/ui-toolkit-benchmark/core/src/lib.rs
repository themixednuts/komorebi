use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    Command,
    Application,
    File,
}

impl ResultKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Command => "COMMAND",
            Self::Application => "APP",
            Self::File => "FILE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PaletteItem {
    pub id: &'static str,
    pub kind: ResultKind,
    pub title: &'static str,
    pub detail: &'static str,
    pub hint: &'static str,
}

const ITEMS: [PaletteItem; 18] = [
    PaletteItem {
        id: "action.focus-left",
        kind: ResultKind::Command,
        title: "Focus left",
        detail: "Move focus to the window on the left",
        hint: "Alt + H",
    },
    PaletteItem {
        id: "action.focus-right",
        kind: ResultKind::Command,
        title: "Focus right",
        detail: "Move focus to the window on the right",
        hint: "Alt + L",
    },
    PaletteItem {
        id: "action.toggle-float",
        kind: ResultKind::Command,
        title: "Toggle floating",
        detail: "Toggle the selected window between tiled and floating",
        hint: "Alt + F",
    },
    PaletteItem {
        id: "action.workspace-overview",
        kind: ResultKind::Command,
        title: "Workspace overview",
        detail: "Show every workspace on the active monitor",
        hint: "Alt + Tab",
    },
    PaletteItem {
        id: "action.close-window",
        kind: ResultKind::Command,
        title: "Close focused window",
        detail: "Close the window focused before the palette opened",
        hint: "Alt + Q",
    },
    PaletteItem {
        id: "action.reload-profile",
        kind: ResultKind::Command,
        title: "Reload configuration",
        detail: "Compile and activate the selected configuration profile",
        hint: "",
    },
    PaletteItem {
        id: "app.terminal",
        kind: ResultKind::Application,
        title: "Windows Terminal",
        detail: "Terminal",
        hint: "APP",
    },
    PaletteItem {
        id: "app.vscode",
        kind: ResultKind::Application,
        title: "Visual Studio Code",
        detail: "Code editor",
        hint: "APP",
    },
    PaletteItem {
        id: "app.chrome",
        kind: ResultKind::Application,
        title: "Google Chrome",
        detail: "Web browser",
        hint: "APP",
    },
    PaletteItem {
        id: "app.explorer",
        kind: ResultKind::Application,
        title: "File Explorer",
        detail: "Windows shell",
        hint: "APP",
    },
    PaletteItem {
        id: "app.spotify",
        kind: ResultKind::Application,
        title: "Spotify",
        detail: "Music",
        hint: "APP",
    },
    PaletteItem {
        id: "app.discord",
        kind: ResultKind::Application,
        title: "Discord",
        detail: "Chat",
        hint: "APP",
    },
    PaletteItem {
        id: "file.komorebi",
        kind: ResultKind::File,
        title: "komorebi.json",
        detail: "C:\\Users\\jonfo\\komorebi.json",
        hint: "JSON",
    },
    PaletteItem {
        id: "file.bar",
        kind: ResultKind::File,
        title: "komorebi.bar.json",
        detail: "C:\\Users\\jonfo\\komorebi.bar.json",
        hint: "JSON",
    },
    PaletteItem {
        id: "file.context",
        kind: ResultKind::File,
        title: "CONTEXT.md",
        detail: "E:\\Projects\\komorebi\\CONTEXT.md",
        hint: "MD",
    },
    PaletteItem {
        id: "file.whkd",
        kind: ResultKind::File,
        title: "whkdrc",
        detail: "C:\\Users\\jonfo\\.config\\whkdrc",
        hint: "CONFIG",
    },
    PaletteItem {
        id: "file.palette",
        kind: ResultKind::File,
        title: "command-palette.md",
        detail: "E:\\Projects\\komorebi\\docs\\command-palette.md",
        hint: "MD",
    },
    PaletteItem {
        id: "file.theme",
        kind: ResultKind::File,
        title: "demon-slayer.theme",
        detail: "C:\\Users\\jonfo\\AppData\\Local\\Microsoft\\Windows\\Themes",
        hint: "THEME",
    },
];

#[derive(Debug)]
pub struct PaletteState {
    query: String,
    matches: Vec<usize>,
    selected: Option<&'static str>,
    activated: Option<&'static str>,
}

impl Default for PaletteState {
    fn default() -> Self {
        Self {
            query: String::new(),
            matches: (0..ITEMS.len()).collect(),
            selected: ITEMS.first().map(|item| item.id),
            activated: None,
        }
    }
}

impl PaletteState {
    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        let needle = self.query.trim().to_lowercase();
        self.matches = ITEMS
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                needle.is_empty()
                    || item.title.to_lowercase().contains(&needle)
                    || item.detail.to_lowercase().contains(&needle)
                    || item.kind.label().to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();

        let selected_is_visible = self.selected.is_some_and(|selected| {
            self.matches
                .iter()
                .any(|index| ITEMS[*index].id == selected)
        });
        if !selected_is_visible {
            self.selected = self.matches.first().map(|index| ITEMS[*index].id);
        }
    }

    pub fn items(&self) -> impl ExactSizeIterator<Item = &'static PaletteItem> + '_ {
        self.matches.iter().map(|index| &ITEMS[*index])
    }

    pub fn selected_id(&self) -> Option<&'static str> {
        self.selected
    }

    pub fn select(&mut self, id: &'static str) {
        if self.matches.iter().any(|index| ITEMS[*index].id == id) {
            self.selected = Some(id);
        }
    }

    pub fn move_selection(&mut self, offset: isize) {
        let Some(current) = self.selected else {
            self.selected = self.matches.first().map(|index| ITEMS[*index].id);
            return;
        };
        let Some(position) = self
            .matches
            .iter()
            .position(|index| ITEMS[*index].id == current)
        else {
            self.selected = self.matches.first().map(|index| ITEMS[*index].id);
            return;
        };
        let len = self.matches.len();
        if len == 0 {
            self.selected = None;
            return;
        }
        let next = (position as isize + offset).rem_euclid(len as isize) as usize;
        self.selected = self.matches.get(next).map(|index| ITEMS[*index].id);
    }

    pub fn activate_selected(&mut self) -> Option<&'static str> {
        self.activated = self.selected;
        self.activated
    }

    pub fn activated_id(&self) -> Option<&'static str> {
        self.activated
    }
}

#[derive(Clone)]
pub struct Probe(Arc<ProbeInner>);

struct ProbeInner {
    surface: &'static str,
    start: Instant,
    path: Option<PathBuf>,
    write_lock: Mutex<()>,
    first_frame_recorded: AtomicBool,
}

#[derive(Serialize)]
struct TraceEvent<'a> {
    surface: &'static str,
    event: &'a str,
    elapsed_us: u128,
    value: Option<&'a str>,
}

impl Probe {
    pub fn from_env(surface: &'static str) -> Self {
        let path = std::env::var_os("KOMOREBI_UI_PROTOTYPE_TRACE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "komorebi-ui-prototype-{surface}-{}.jsonl",
                    std::process::id()
                ))
            });
        let probe = Self(Arc::new(ProbeInner {
            surface,
            start: Instant::now(),
            path: Some(path),
            write_lock: Mutex::new(()),
            first_frame_recorded: AtomicBool::new(false),
        }));
        probe.record("process_ready", None);
        probe
    }

    pub fn record(&self, event: &str, value: Option<&str>) {
        let Some(path) = &self.0.path else {
            return;
        };
        let Ok(_guard) = self.0.write_lock.lock() else {
            return;
        };
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        let event = TraceEvent {
            surface: self.0.surface,
            event,
            elapsed_us: self.0.start.elapsed().as_micros(),
            value,
        };
        if serde_json::to_writer(&mut file, &event).is_ok() {
            let _ = writeln!(file);
        }
    }

    pub fn record_first_frame(&self) {
        if !self.0.first_frame_recorded.swap(true, Ordering::AcqRel) {
            self.record("first_frame", None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_stays_on_identity_when_filter_changes() {
        let mut state = PaletteState::default();
        state.select("app.vscode");
        state.set_query("code");
        assert_eq!(state.selected_id(), Some("app.vscode"));
    }

    #[test]
    fn selection_moves_with_wraparound() {
        let mut state = PaletteState::default();
        state.move_selection(-1);
        assert_eq!(state.selected_id(), Some("file.theme"));
    }
}
