use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::WrapErr;
use miow::pipe::connect;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use uds_windows::UnixListener;
use uds_windows::UnixStream;

use crate::DATA_DIR;
use crate::DISPLAY_INDEX_PREFERENCES;
use crate::IGNORE_IDENTIFIERS;
use crate::INITIAL_CONFIGURATION_LOADED;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::MONITOR_INDEX_PREFERENCES;
use crate::Notification;
use crate::NotificationEvent;
use crate::OBJECT_NAME_CHANGE_ON_LAUNCH;
use crate::SESSION_FLOATING_APPLICATIONS;
use crate::SUBSCRIBERS;
use crate::TCP_CONNECTIONS;
use crate::TRAY_AND_MULTI_WINDOW_IDENTIFIERS;
use crate::WORKSPACE_MATCHING_RULES;
use crate::adapters::socket_message::SocketMessageClass;
use crate::adapters::socket_message::adapt_action;
use crate::adapters::socket_message::classify;
use crate::border_manager;
use crate::build;
use crate::config_generation::WorkspaceMatchingRule;
use crate::core::ApplicationIdentifier;
use crate::core::Layout;
use crate::core::Rect;
use crate::core::SocketMessage;
use crate::core::StateQuery;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::current_virtual_desktop;
use crate::monitor::MonitorInformation;
use crate::notify_subscribers;
use crate::stackbar_manager;
use crate::state;
use crate::state::GlobalState;
use crate::state::State;
use crate::static_config::StaticConfig;
use crate::theme_manager;
use crate::transparency_manager;
use crate::window::RuleDebug;
use crate::window::Window;
use crate::window_manager::WindowManager;

pub fn bind_legacy_command_listener(socket: &Path) -> eyre::Result<UnixListener> {
    match std::fs::remove_file(socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    UnixListener::bind(socket).map_err(Into::into)
}

#[tracing::instrument(skip(listener))]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>, listener: UnixListener) {
    std::thread::spawn(move || {
        tracing::info!("listening on komorebi.sock");
        for client in listener.incoming() {
            match client {
                Ok(stream) => {
                    let wm = wm.clone();
                    std::thread::spawn(move || {
                        if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(1))) {
                            tracing::error!(%error, "could not set legacy command read timeout");
                        }
                        if let Err(error) = read_commands_uds(&wm, stream) {
                            tracing::error!(%error, "legacy command connection failed");
                        }
                    });
                }
                Err(error) => {
                    tracing::error!(%error, "legacy command listener stopped");
                    break;
                }
            }
        }
    });
}

#[tracing::instrument]
pub fn listen_for_commands_tcp(wm: Arc<Mutex<WindowManager>>, port: usize) {
    let listener =
        TcpListener::bind(format!("0.0.0.0:{port}")).expect("could not start tcp server");

    std::thread::spawn(move || {
        tracing::info!("listening on 0.0.0.0:43663");
        for client in listener.incoming() {
            match client {
                Ok(mut stream) => {
                    net2::TcpStreamExt::set_keepalive(&stream, Some(Duration::from_secs(30)))
                        .expect("TCP keepalive should be set");

                    let addr = stream
                        .peer_addr()
                        .expect("incoming connection should have an address")
                        .to_string();

                    let mut connections = TCP_CONNECTIONS.lock();

                    connections.insert(
                        addr.clone(),
                        stream.try_clone().expect("stream should be cloneable"),
                    );

                    tracing::info!("listening for incoming tcp messages from {}", &addr);

                    match read_commands_tcp(&wm, &mut stream, &addr) {
                        Ok(()) => {}
                        Err(error) => tracing::error!("{}", error),
                    }
                }
                Err(error) => {
                    tracing::error!("{}", error);
                    break;
                }
            }
        }
    });
}

impl WindowManager {
    // TODO(raggi): wrap reply in a newtype that can decorate a human friendly
    // name for the peer, such as getting the pid of the komorebic process for
    // the UDS or the IP:port for TCP.
    #[tracing::instrument(skip(self, reply))]
    pub fn process_command(
        &mut self,
        message: SocketMessage,
        mut reply: impl std::io::Write,
    ) -> eyre::Result<()> {
        if let Some(virtual_desktop_id) = &self.virtual_desktop_id
            && let Some(id) = current_virtual_desktop()
            && id != *virtual_desktop_id
        {
            tracing::info!(
                "ignoring events and commands while not on virtual desktop {:?}",
                virtual_desktop_id
            );
            return Ok(());
        }

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        let mut force_update_borders = false;
        if let Some(action) = adapt_action(&message)? {
            self.admit_socket_action(action)?;
        } else {
            match message {
                SocketMessage::FocusWindow(..)
                | SocketMessage::MoveWindow(..)
                | SocketMessage::PreselectDirection(..)
                | SocketMessage::CancelPreselect
                | SocketMessage::CycleFocusWindow(..)
                | SocketMessage::CycleMoveWindow(..)
                | SocketMessage::StackWindow(..)
                | SocketMessage::UnstackWindow
                | SocketMessage::CycleStack(..)
                | SocketMessage::CycleStackIndex(..)
                | SocketMessage::FocusStackWindow(..)
                | SocketMessage::StackAll
                | SocketMessage::UnstackAll
                | SocketMessage::ResizeWindowEdge(..)
                | SocketMessage::ResizeWindowAxis(..)
                | SocketMessage::ResizeDelta(..)
                | SocketMessage::Transparency(..)
                | SocketMessage::ToggleTransparency
                | SocketMessage::TransparencyAlpha(..)
                | SocketMessage::MoveContainerToLastWorkspace
                | SocketMessage::SendContainerToLastWorkspace
                | SocketMessage::MoveContainerToMonitorNumber(..)
                | SocketMessage::CycleMoveContainerToMonitor(..)
                | SocketMessage::MoveContainerToWorkspaceNumber(..)
                | SocketMessage::MoveContainerToNamedWorkspace(..)
                | SocketMessage::CycleMoveContainerToWorkspace(..)
                | SocketMessage::SendContainerToMonitorNumber(..)
                | SocketMessage::CycleSendContainerToMonitor(..)
                | SocketMessage::SendContainerToWorkspaceNumber(..)
                | SocketMessage::CycleSendContainerToWorkspace(..)
                | SocketMessage::SendContainerToMonitorWorkspaceNumber(..)
                | SocketMessage::MoveContainerToMonitorWorkspaceNumber(..)
                | SocketMessage::SendContainerToNamedWorkspace(..)
                | SocketMessage::CycleMoveWorkspaceToMonitor(..)
                | SocketMessage::MoveWorkspaceToMonitorNumber(..)
                | SocketMessage::SwapWorkspacesToMonitorNumber(..)
                | SocketMessage::ForceFocus
                | SocketMessage::Close
                | SocketMessage::Minimize
                | SocketMessage::Promote
                | SocketMessage::PromoteSwap
                | SocketMessage::PromoteFocus
                | SocketMessage::PromoteWindow(..)
                | SocketMessage::EagerFocus(..)
                | SocketMessage::LockMonitorWorkspaceContainer(..)
                | SocketMessage::UnlockMonitorWorkspaceContainer(..)
                | SocketMessage::ToggleLock
                | SocketMessage::ToggleFloat
                | SocketMessage::ToggleMonocle
                | SocketMessage::ToggleMaximize
                | SocketMessage::ToggleWindowContainerBehaviour
                | SocketMessage::ToggleFloatOverride
                | SocketMessage::WindowHidingBehaviour(..)
                | SocketMessage::ToggleCrossMonitorMoveBehaviour
                | SocketMessage::CrossMonitorMoveBehaviour(..)
                | SocketMessage::ToggleMonocleFocusBehaviour
                | SocketMessage::MonocleFocusBehaviour(..)
                | SocketMessage::UnmanagedWindowOperationBehaviour(..)
                | SocketMessage::ManageFocusedWindow
                | SocketMessage::UnmanageFocusedWindow
                | SocketMessage::AdjustContainerPadding(..)
                | SocketMessage::AdjustWorkspacePadding(..)
                | SocketMessage::ChangeLayout(..)
                | SocketMessage::CycleLayout(..)
                | SocketMessage::LayoutRatios(..)
                | SocketMessage::ScrollingLayoutColumns(..)
                | SocketMessage::ChangeLayoutCustom(..)
                | SocketMessage::FlipLayout(..)
                | SocketMessage::ToggleWorkspaceWindowContainerBehaviour
                | SocketMessage::ToggleWorkspaceFloatOverride
                | SocketMessage::EnsureWorkspaces(..)
                | SocketMessage::EnsureNamedWorkspaces(..)
                | SocketMessage::NewWorkspace
                | SocketMessage::ToggleTiling
                | SocketMessage::TogglePause
                | SocketMessage::Retile
                | SocketMessage::RetileWithResizeDimensions
                | SocketMessage::CycleFocusMonitor(..)
                | SocketMessage::CycleFocusWorkspace(..)
                | SocketMessage::CycleFocusEmptyWorkspace(..)
                | SocketMessage::FocusMonitorNumber(..)
                | SocketMessage::FocusMonitorAtCursor
                | SocketMessage::FocusLastWorkspace
                | SocketMessage::CloseWorkspace
                | SocketMessage::FocusWorkspaceNumber(..)
                | SocketMessage::FocusWorkspaceNumbers(..)
                | SocketMessage::FocusMonitorWorkspaceNumber(..)
                | SocketMessage::FocusNamedWorkspace(..)
                | SocketMessage::ContainerPadding(..)
                | SocketMessage::NamedWorkspaceContainerPadding(..)
                | SocketMessage::FocusedWorkspaceContainerPadding(..)
                | SocketMessage::WorkspacePadding(..)
                | SocketMessage::NamedWorkspacePadding(..)
                | SocketMessage::FocusedWorkspacePadding(..)
                | SocketMessage::WorkspaceTiling(..)
                | SocketMessage::NamedWorkspaceTiling(..)
                | SocketMessage::WorkspaceName(..)
                | SocketMessage::WorkspaceLayout(..)
                | SocketMessage::NamedWorkspaceLayout(..)
                | SocketMessage::WorkspaceLayoutCustom(..)
                | SocketMessage::NamedWorkspaceLayoutCustom(..)
                | SocketMessage::WorkspaceLayoutRule(..)
                | SocketMessage::NamedWorkspaceLayoutRule(..)
                | SocketMessage::WorkspaceLayoutCustomRule(..)
                | SocketMessage::NamedWorkspaceLayoutCustomRule(..)
                | SocketMessage::ClearWorkspaceLayoutRules(..)
                | SocketMessage::ClearNamedWorkspaceLayoutRules(..)
                | SocketMessage::ToggleWorkspaceLayer
                | SocketMessage::FocusFollowsMouse(..)
                | SocketMessage::ToggleFocusFollowsMouse(..)
                | SocketMessage::MouseFollowsFocus(..)
                | SocketMessage::ToggleMouseFollowsFocus
                | SocketMessage::RemoveTitleBar(..)
                | SocketMessage::ToggleTitleBars
                | SocketMessage::SessionFloatRule
                | SocketMessage::ClearSessionFloatRules
                | SocketMessage::EnforceWorkspaceRules => {
                    unreachable!("action messages are adapted before legacy dispatch")
                }
                SocketMessage::InitialWorkspaceRule(
                    identifier,
                    ref id,
                    monitor_idx,
                    workspace_idx,
                ) => {
                    let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                    let workspace_matching_rule = WorkspaceMatchingRule {
                        monitor_index: monitor_idx,
                        workspace_index: workspace_idx,
                        matching_rule: MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.to_string(),
                            matching_strategy: Some(MatchingStrategy::Legacy),
                        }),
                        initial_only: true,
                    };

                    if !workspace_rules.contains(&workspace_matching_rule) {
                        workspace_rules.push(workspace_matching_rule);
                    }
                }
                SocketMessage::InitialNamedWorkspaceRule(identifier, ref id, ref workspace) => {
                    if let Some((monitor_idx, workspace_idx)) =
                        self.monitor_workspace_index_by_name(workspace)
                    {
                        let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                        let workspace_matching_rule = WorkspaceMatchingRule {
                            monitor_index: monitor_idx,
                            workspace_index: workspace_idx,
                            matching_rule: MatchingRule::Simple(IdWithIdentifier {
                                kind: identifier,
                                id: id.to_string(),
                                matching_strategy: Some(MatchingStrategy::Legacy),
                            }),
                            initial_only: true,
                        };

                        if !workspace_rules.contains(&workspace_matching_rule) {
                            workspace_rules.push(workspace_matching_rule);
                        }
                    }
                }
                SocketMessage::WorkspaceRule(identifier, ref id, monitor_idx, workspace_idx) => {
                    let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                    let workspace_matching_rule = WorkspaceMatchingRule {
                        monitor_index: monitor_idx,
                        workspace_index: workspace_idx,
                        matching_rule: MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.to_string(),
                            matching_strategy: Some(MatchingStrategy::Legacy),
                        }),
                        initial_only: false,
                    };

                    if !workspace_rules.contains(&workspace_matching_rule) {
                        workspace_rules.push(workspace_matching_rule);
                    }
                }
                SocketMessage::NamedWorkspaceRule(identifier, ref id, ref workspace) => {
                    if let Some((monitor_idx, workspace_idx)) =
                        self.monitor_workspace_index_by_name(workspace)
                    {
                        let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                        let workspace_matching_rule = WorkspaceMatchingRule {
                            monitor_index: monitor_idx,
                            workspace_index: workspace_idx,
                            matching_rule: MatchingRule::Simple(IdWithIdentifier {
                                kind: identifier,
                                id: id.to_string(),
                                matching_strategy: Some(MatchingStrategy::Legacy),
                            }),
                            initial_only: false,
                        };

                        if !workspace_rules.contains(&workspace_matching_rule) {
                            workspace_rules.push(workspace_matching_rule);
                        }
                    }
                }
                SocketMessage::ClearWorkspaceRules(monitor_idx, workspace_idx) => {
                    let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();

                    workspace_rules.retain(|r| {
                        r.monitor_index != monitor_idx && r.workspace_index != workspace_idx
                    });
                }
                SocketMessage::ClearNamedWorkspaceRules(ref workspace) => {
                    if let Some((monitor_idx, workspace_idx)) =
                        self.monitor_workspace_index_by_name(workspace)
                    {
                        let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                        workspace_rules.retain(|r| {
                            r.monitor_index != monitor_idx && r.workspace_index != workspace_idx
                        });
                    }
                }
                SocketMessage::ClearAllWorkspaceRules => {
                    let mut workspace_rules = WORKSPACE_MATCHING_RULES.lock();
                    workspace_rules.clear();
                }
                SocketMessage::ManageRule(identifier, ref id) => {
                    let mut manage_identifiers = MANAGE_IDENTIFIERS.lock();

                    let mut should_push = true;
                    for m in &*manage_identifiers {
                        if let MatchingRule::Simple(m) = m
                            && m.id.eq(id)
                        {
                            should_push = false;
                        }
                    }

                    if should_push {
                        manage_identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.clone(),
                            matching_strategy: Option::from(MatchingStrategy::Legacy),
                        }));
                    }
                }
                SocketMessage::SessionFloatRules => {
                    let session_floating_applications = SESSION_FLOATING_APPLICATIONS.lock();
                    let rules = match serde_json::to_string_pretty(&*session_floating_applications)
                    {
                        Ok(rules) => rules,
                        Err(error) => error.to_string(),
                    };

                    reply.write_all(rules.as_bytes())?;
                }
                SocketMessage::IgnoreRule(identifier, ref id) => {
                    let mut ignore_identifiers = IGNORE_IDENTIFIERS.lock();

                    let mut should_push = true;
                    for i in &*ignore_identifiers {
                        if let MatchingRule::Simple(i) = i
                            && i.id.eq(id)
                        {
                            should_push = false;
                        }
                    }

                    if should_push {
                        ignore_identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.clone(),
                            matching_strategy: Option::from(MatchingStrategy::Legacy),
                        }));
                    }

                    let offset = self.work_area_offset;

                    let mut hwnds_to_purge = vec![];
                    for (i, monitor) in self.monitors().iter().enumerate() {
                        for container in monitor
                            .focused_workspace()
                            .ok_or_eyre("there is no workspace")?
                            .containers()
                        {
                            for window in container.windows() {
                                match identifier {
                                    ApplicationIdentifier::Path => {
                                        if window.path()? == *id {
                                            hwnds_to_purge.push((i, window.hwnd));
                                        }
                                    }
                                    ApplicationIdentifier::Exe => {
                                        if window.exe()? == *id {
                                            hwnds_to_purge.push((i, window.hwnd));
                                        }
                                    }
                                    ApplicationIdentifier::Class => {
                                        if window.class()? == *id {
                                            hwnds_to_purge.push((i, window.hwnd));
                                        }
                                    }
                                    ApplicationIdentifier::Title => {
                                        if window.title()? == *id {
                                            hwnds_to_purge.push((i, window.hwnd));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    for (monitor_idx, hwnd) in hwnds_to_purge {
                        let monitor = self
                            .monitors_mut()
                            .get_mut(monitor_idx)
                            .ok_or_eyre("there is no monitor")?;

                        monitor
                            .focused_workspace_mut()
                            .ok_or_eyre("there is no focused workspace")?
                            .remove_window(hwnd)?;

                        monitor.update_focused_workspace(offset)?;
                    }
                }
                SocketMessage::Stop => {
                    self.stop(false)?;
                }
                SocketMessage::StopIgnoreRestore => {
                    self.stop(true)?;
                }
                SocketMessage::MonitorIndexPreference(
                    index_preference,
                    left,
                    top,
                    right,
                    bottom,
                ) => {
                    let mut monitor_index_preferences = MONITOR_INDEX_PREFERENCES.lock();
                    monitor_index_preferences.insert(
                        index_preference,
                        Rect {
                            left,
                            top,
                            right,
                            bottom,
                        },
                    );
                }
                SocketMessage::DisplayIndexPreference(index_preference, ref display) => {
                    let mut display_index_preferences = DISPLAY_INDEX_PREFERENCES.write();
                    display_index_preferences.insert(index_preference, display.clone());
                }
                SocketMessage::State => {
                    let state = match serde_json::to_string_pretty(&state::State::from(&*self)) {
                        Ok(state) => state,
                        Err(error) => error.to_string(),
                    };

                    tracing::info!("replying to state");

                    reply.write_all(state.as_bytes())?;

                    tracing::info!("replying to state done");
                }
                SocketMessage::GlobalState => {
                    let state = match serde_json::to_string_pretty(&GlobalState::default()) {
                        Ok(state) => state,
                        Err(error) => error.to_string(),
                    };

                    tracing::info!("replying to global state");

                    reply.write_all(state.as_bytes())?;

                    tracing::info!("replying to global state done");
                }
                SocketMessage::VisibleWindows => {
                    let mut monitor_visible_windows = HashMap::new();

                    for monitor in self.monitors() {
                        if let Some(ws) = monitor.focused_workspace() {
                            monitor_visible_windows.insert(
                                monitor.device_id.clone(),
                                ws.visible_window_details().clone(),
                            );
                        }
                    }

                    let visible_windows_state =
                        serde_json::to_string_pretty(&monitor_visible_windows)
                            .unwrap_or_else(|error| error.to_string());

                    reply.write_all(visible_windows_state.as_bytes())?;
                }
                SocketMessage::MonitorInformation => {
                    let mut monitors = vec![];
                    for monitor in self.monitors() {
                        monitors.push(MonitorInformation::from(monitor));
                    }

                    let monitors_state = serde_json::to_string_pretty(&monitors)
                        .unwrap_or_else(|error| error.to_string());

                    reply.write_all(monitors_state.as_bytes())?;
                }
                SocketMessage::Query(query) => {
                    let response = match query {
                        StateQuery::FocusedMonitorIndex => self.focused_monitor_idx().to_string(),
                        StateQuery::FocusedWorkspaceIndex => self
                            .focused_monitor()
                            .ok_or_eyre("there is no monitor")?
                            .focused_workspace_idx()
                            .to_string(),
                        StateQuery::FocusedContainerIndex => self
                            .focused_workspace()?
                            .focused_container_idx()
                            .to_string(),
                        StateQuery::FocusedWindowIndex => {
                            self.focused_container()?.focused_window_idx().to_string()
                        }
                        StateQuery::FocusedWorkspaceName => {
                            let focused_monitor =
                                self.focused_monitor().ok_or_eyre("there is no monitor")?;

                            focused_monitor.focused_workspace_name().unwrap_or_else(|| {
                                focused_monitor.focused_workspace_idx().to_string()
                            })
                        }
                        StateQuery::Version => build::RUST_VERSION.to_string(),
                        StateQuery::FocusedWorkspaceLayout => {
                            let focused_monitor =
                                self.focused_monitor().ok_or_eyre("there is no monitor")?;

                            focused_monitor.focused_workspace_layout().map_or_else(
                                || "None".to_string(),
                                |layout| match layout {
                                    Layout::Default(default_layout) => default_layout.to_string(),
                                    Layout::Custom(_) => "Custom".to_string(),
                                },
                            )
                        }
                        StateQuery::FocusedContainerKind => {
                            match self.focused_workspace()?.focused_container() {
                                None => "None".to_string(),
                                Some(container) => {
                                    if container.windows().len() > 1 {
                                        "Stack".to_string()
                                    } else {
                                        "Single".to_string()
                                    }
                                }
                            }
                        }
                    };

                    reply.write_all(response.as_bytes())?;
                }
                SocketMessage::ReloadConfiguration => {
                    Self::reload_configuration();
                    force_update_borders = true;
                }
                SocketMessage::ReplaceConfiguration(ref config) => {
                    // Check that this is a valid static config file first
                    if StaticConfig::read(config).is_ok() {
                        // Clear workspace rules; these will need to be replaced
                        WORKSPACE_MATCHING_RULES.lock().clear();
                        // Pause so that restored windows come to the foreground from all workspaces
                        self.is_paused = true;
                        // Bring all windows to the foreground
                        self.restore_all_windows(false)?;

                        // Create a new wm from the config path
                        let mut wm = StaticConfig::preload(config, self.manager_epoch)?;

                        // Initialize the new wm
                        wm.init()?;

                        wm.restore_all_windows(true)?;

                        // This is equivalent to StaticConfig::postload for this use case
                        StaticConfig::reload(config, &mut wm)?;

                        // Set self to the new wm instance
                        *self = wm;

                        // check if there are any bars
                        let mut system = sysinfo::System::new_all();
                        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

                        let has_bar = system
                            .processes_by_name("komorebi-bar.exe".as_ref())
                            .next()
                            .is_some();

                        // stop bar(s)
                        if has_bar {
                            let script = r"
Stop-Process -Name:komorebi-bar -ErrorAction SilentlyContinue
                ";
                            match powershell_script::run(script) {
                                Ok(_) => {
                                    println!("{script}");

                                    // start new bar(s)
                                    let mut config = StaticConfig::read(config)?;
                                    if let Some(display_bar_configurations) =
                                        &mut config.bar_configurations
                                    {
                                        for config_file_path in &mut *display_bar_configurations {
                                            let script = r#"Start-Process "komorebi-bar" '"--config" "CONFIGFILE"' -WindowStyle hidden"#
                                            .replace("CONFIGFILE", &config_file_path.to_string_lossy());

                                            match powershell_script::run(&script) {
                                                Ok(_) => {
                                                    println!("{script}");
                                                }
                                                Err(error) => {
                                                    println!("Error: {error}");
                                                }
                                            }
                                        }
                                    } else {
                                        let script = r"
if (!(Get-Process komorebi-bar -ErrorAction SilentlyContinue))
{
  Start-Process komorebi-bar -WindowStyle hidden
}
                ";
                                        match powershell_script::run(script) {
                                            Ok(_) => {
                                                println!("{script}");
                                            }
                                            Err(error) => {
                                                println!("Error: {error}");
                                            }
                                        }
                                    }
                                }
                                Err(error) => {
                                    println!("Error: {error}");
                                }
                            }
                        }

                        force_update_borders = true;
                    }
                }
                SocketMessage::ReloadStaticConfiguration(ref pathbuf) => {
                    self.reload_static_configuration(pathbuf)?;
                    force_update_borders = true;
                }
                SocketMessage::CompleteConfiguration => {
                    if !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
                        INITIAL_CONFIGURATION_LOADED.store(true, Ordering::SeqCst);
                        self.update_focused_workspace(false, false)?;
                        force_update_borders = true;
                    }
                }
                SocketMessage::WatchConfiguration(enable) => {
                    self.watch_configuration(enable)?;
                }
                SocketMessage::IdentifyObjectNameChangeApplication(identifier, ref id) => {
                    let mut identifiers = OBJECT_NAME_CHANGE_ON_LAUNCH.lock();

                    let mut should_push = true;
                    for i in &*identifiers {
                        if let MatchingRule::Simple(i) = i
                            && i.id.eq(id)
                        {
                            should_push = false;
                        }
                    }

                    if should_push {
                        identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.clone(),
                            matching_strategy: Option::from(MatchingStrategy::Legacy),
                        }));
                    }
                }
                SocketMessage::IdentifyTrayApplication(identifier, ref id) => {
                    let mut identifiers = TRAY_AND_MULTI_WINDOW_IDENTIFIERS.lock();
                    let mut should_push = true;
                    for i in &*identifiers {
                        if let MatchingRule::Simple(i) = i
                            && i.id.eq(id)
                        {
                            should_push = false;
                        }
                    }

                    if should_push {
                        identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.clone(),
                            matching_strategy: Option::from(MatchingStrategy::Legacy),
                        }));
                    }
                }
                SocketMessage::IdentifyLayeredApplication(identifier, ref id) => {
                    let mut identifiers = LAYERED_WHITELIST.lock();

                    let mut should_push = true;
                    for i in &*identifiers {
                        if let MatchingRule::Simple(i) = i
                            && i.id.eq(id)
                        {
                            should_push = false;
                        }
                    }

                    if should_push {
                        identifiers.push(MatchingRule::Simple(IdWithIdentifier {
                            kind: identifier,
                            id: id.clone(),
                            matching_strategy: Option::from(MatchingStrategy::Legacy),
                        }));
                    }
                }
                SocketMessage::QuickSave => {
                    let workspace = self.focused_workspace()?;
                    let resize = &workspace.resize_dimensions;

                    let quicksave_json = std::env::temp_dir().join("komorebi.quicksave.json");

                    let file = OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .create(true)
                        .open(quicksave_json)?;

                    serde_json::to_writer_pretty(&file, &resize)?;
                }
                SocketMessage::QuickLoad => {
                    let workspace = self.focused_workspace_mut()?;

                    let quicksave_json = std::env::temp_dir().join("komorebi.quicksave.json");

                    let file = File::open(&quicksave_json).wrap_err(format!(
                        "no quicksave found at {}",
                        quicksave_json.display()
                    ))?;

                    let resize: Vec<Option<Rect>> = serde_json::from_reader(file)?;

                    workspace.resize_dimensions = resize;
                    self.update_focused_workspace(false, false)?;
                }
                SocketMessage::Save(ref path) => {
                    let workspace = self.focused_workspace_mut()?;
                    let resize = &workspace.resize_dimensions;

                    let file = OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .create(true)
                        .open(path)?;

                    serde_json::to_writer_pretty(&file, &resize)?;
                }
                SocketMessage::Load(ref path) => {
                    let workspace = self.focused_workspace_mut()?;

                    let file = File::open(path)
                        .wrap_err(format!("no file found at {}", path.display()))?;

                    let resize: Vec<Option<Rect>> = serde_json::from_reader(file)?;

                    workspace.resize_dimensions = resize;
                    self.update_focused_workspace(false, false)?;
                }
                SocketMessage::AddSubscriberSocket(ref socket) => {
                    SUBSCRIBERS
                        .lock()
                        .add_socket(&DATA_DIR, socket.clone(), None)?;
                }
                SocketMessage::AddSubscriberSocketWithOptions(ref socket, options) => {
                    SUBSCRIBERS
                        .lock()
                        .add_socket(&DATA_DIR, socket.clone(), Some(options))?;
                }
                SocketMessage::RemoveSubscriberSocket(ref socket) => {
                    SUBSCRIBERS.lock().remove_socket(socket);
                }
                SocketMessage::AddSubscriberPipe(ref subscriber) => {
                    let pipe_path = subscriber.named_pipe_path();
                    let pipe = connect(&pipe_path).wrap_err(format!(
                    "the named pipe '{pipe_path}' has not yet been created; please create it before running this command"
                ))?;

                    SUBSCRIBERS.lock().add_pipe(subscriber.clone(), pipe);
                }
                SocketMessage::RemoveSubscriberPipe(ref subscriber) => {
                    SUBSCRIBERS.lock().remove_pipe(subscriber);
                }
                SocketMessage::ApplicationSpecificConfigurationSchema => {
                    #[cfg(feature = "schemars")]
                    {
                        let asc = schemars::schema_for!(
                            Vec<crate::core::config_generation::ApplicationConfiguration>
                        );
                        let schema = serde_json::to_string_pretty(&asc)?;

                        reply.write_all(schema.as_bytes())?;
                    }
                }
                SocketMessage::NotificationSchema => {
                    #[cfg(feature = "schemars")]
                    {
                        let notification = schemars::schema_for!(Notification);
                        let schema = serde_json::to_string_pretty(&notification)?;

                        reply.write_all(schema.as_bytes())?;
                    }
                }
                SocketMessage::SocketSchema => {
                    #[cfg(feature = "schemars")]
                    {
                        let socket_message = schemars::schema_for!(SocketMessage);
                        let schema = serde_json::to_string_pretty(&socket_message)?;

                        reply.write_all(schema.as_bytes())?;
                    }
                }
                SocketMessage::StaticConfigSchema => {
                    #[cfg(feature = "schemars")]
                    {
                        let socket_message = schemars::schema_for!(SocketMessage);
                        let schema = serde_json::to_string_pretty(&socket_message)?;

                        reply.write_all(schema.as_bytes())?;
                    }
                }
                SocketMessage::GenerateStaticConfig => {
                    let config = serde_json::to_string_pretty(&StaticConfig::from(&*self))?;

                    reply.write_all(config.as_bytes())?;
                }
                SocketMessage::DebugWindow(hwnd) => {
                    let window = Window::from(hwnd);
                    let mut rule_debug = RuleDebug::default();
                    let _ = window.should_manage(None, &mut rule_debug);
                    let schema = serde_json::to_string_pretty(&rule_debug)?;

                    reply.write_all(schema.as_bytes())?;
                }
                SocketMessage::Theme(ref theme) => {
                    theme_manager::send_notification(*theme.clone());
                }
                SocketMessage::ApplyState(ref state) => {
                    self.apply_state(state.clone());
                }
                // Deprecated commands
                SocketMessage::AltFocusHack(_)
                | SocketMessage::IdentifyBorderOverflowApplication(_, _) => {}
            };
        }

        // Update list of known_hwnds and their monitor/workspace index pair
        self.update_known_hwnds();

        notify_subscribers(
            Notification {
                event: NotificationEvent::Socket(message.clone()),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;

        if force_update_borders {
            border_manager::send_force_update();
        } else {
            border_manager::send_notification(None);
        }
        transparency_manager::send_notification();
        stackbar_manager::send_notification();

        tracing::info!("processed");
        Ok(())
    }
}

pub fn read_commands_uds(
    wm: &Arc<Mutex<WindowManager>>,
    mut stream: UnixStream,
) -> eyre::Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    // TODO(raggi): while this processes more than one command, if there are
    // replies there is no clearly defined protocol for framing yet - it's
    // perhaps whole-json objects for now, but termination is signalled by
    // socket shutdown.
    for line in reader.lines() {
        let message = SocketMessage::from_str(&line?)?;

        match wm.try_lock_for(Duration::from_secs(1)) {
            None => {
                tracing::warn!(
                    "could not acquire window manager lock, not processing message: {message}"
                );
            }
            Some(mut wm) => {
                if wm.is_paused {
                    return match message {
                        SocketMessage::TogglePause
                        | SocketMessage::State
                        | SocketMessage::GlobalState
                        | SocketMessage::Stop => Ok(wm.process_command(message, &mut stream)?),
                        other if classify(&other) == SocketMessageClass::Action => {
                            Ok(wm.process_command(other, &mut stream)?)
                        }
                        _ => {
                            tracing::trace!("ignoring while paused");
                            Ok(())
                        }
                    };
                }

                wm.process_command(message.clone(), &mut stream)?;
            }
        }
    }

    Ok(())
}

pub fn read_commands_tcp(
    wm: &Arc<Mutex<WindowManager>>,
    stream: &mut TcpStream,
    addr: &str,
) -> eyre::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    loop {
        let mut buf = vec![0; 1024];
        match reader.read(&mut buf) {
            Err(..) => {
                tracing::warn!("removing disconnected tcp client: {addr}");
                let mut connections = TCP_CONNECTIONS.lock();
                connections.remove(addr);
                break;
            }
            Ok(size) => {
                let Ok(message) = SocketMessage::from_str(&String::from_utf8_lossy(&buf[..size]))
                else {
                    tracing::warn!("client sent an invalid message, disconnecting: {addr}");
                    let mut connections = TCP_CONNECTIONS.lock();
                    connections.remove(addr);
                    break;
                };

                let mut wm = wm.lock();

                if wm.is_paused {
                    return match message {
                        SocketMessage::TogglePause
                        | SocketMessage::State
                        | SocketMessage::GlobalState
                        | SocketMessage::Stop => Ok(wm.process_command(message, stream)?),
                        other if classify(&other) == SocketMessageClass::Action => {
                            Ok(wm.process_command(other, stream)?)
                        }
                        _ => {
                            tracing::trace!("ignoring while paused");
                            Ok(())
                        }
                    };
                }

                wm.process_command(message.clone(), &mut *stream)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::Rect;
    use crate::SocketMessage;
    use crate::monitor;
    use crate::window_manager::WindowManager;
    use komorebi_protocol::ManagerEpoch;
    use std::io::BufRead;
    use std::io::BufReader;
    use std::io::Write;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::time::Duration;
    use uds_windows::UnixStream;
    use uuid::Uuid;

    fn manager_epoch() -> ManagerEpoch {
        ManagerEpoch::new([1; 16]).expect("test epoch is non-nil")
    }

    fn window_manager() -> WindowManager {
        WindowManager::new(manager_epoch()).expect("test manager should initialize")
    }

    fn paused_manager() -> WindowManager {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        wm.is_paused = true;
        wm
    }

    fn assert_paused_rejects(message: SocketMessage) {
        let mut wm = paused_manager();
        let error = wm
            .process_command(message, Vec::new())
            .expect_err("paused command must not apply");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
    }

    fn send_socket_message(socket: &PathBuf, message: SocketMessage) {
        let mut stream = UnixStream::connect(socket).unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        stream
            .write_all(serde_json::to_string(&message).unwrap().as_bytes())
            .unwrap();
    }

    #[test]
    fn test_receive_socket_message() {
        let test_socket_name = format!("komorebi-test-{}.sock", Uuid::new_v4());
        let test_socket_path = PathBuf::from(&test_socket_name);
        let listener = super::bind_legacy_command_listener(&test_socket_path).unwrap();
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        wm.monitors_mut().push_back(m);

        // send a message
        send_socket_message(&test_socket_path, SocketMessage::FocusWorkspaceNumber(5));

        let (stream, _) = listener.accept().unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let next = reader.lines().next();

        // read and deserialize the message
        let message_string = next.unwrap().unwrap();
        let message = SocketMessage::from_str(&message_string).unwrap();
        assert!(matches!(message, SocketMessage::FocusWorkspaceNumber(5)));

        // process the message
        wm.process_command(message, stream).unwrap();

        // check the updated window manager state
        assert_eq!(wm.focused_workspace_idx().unwrap(), 5);

        std::fs::remove_file(test_socket_path).unwrap();
    }

    #[test]
    fn live_cycle_focus_workspace_advances_index() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);

        let stream = Vec::new();
        wm.process_command(SocketMessage::FocusWorkspaceNumber(1), stream)
            .unwrap();
        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);

        let stream = Vec::new();
        wm.process_command(SocketMessage::FocusWorkspaceNumber(0), stream)
            .unwrap();
        assert_eq!(wm.focused_workspace_idx().unwrap(), 0);

        let stream = Vec::new();
        wm.process_command(
            SocketMessage::CycleFocusWorkspace(crate::core::CycleDirection::Next),
            stream,
        )
        .unwrap();
        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
    }

    #[test]
    fn live_focus_monitor_workspace_number_selects_pair() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);

        let stream = Vec::new();
        wm.process_command(SocketMessage::FocusMonitorWorkspaceNumber(0, 3), stream)
            .unwrap();
        assert_eq!(wm.focused_monitor_idx(), 0);
        assert_eq!(wm.focused_workspace_idx().unwrap(), 3);
    }

    #[test]
    fn paused_focus_window_is_rejected_instead_of_ignored() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        wm.is_paused = true;

        let stream = Vec::new();
        let error = wm
            .process_command(
                SocketMessage::FocusWindow(crate::core::OperationDirection::Left),
                stream,
            )
            .expect_err("paused focus must not no-op");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
    }

    #[test]
    fn paused_move_window_is_rejected_instead_of_ignored() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        wm.is_paused = true;

        let stream = Vec::new();
        let error = wm
            .process_command(
                SocketMessage::MoveWindow(crate::core::OperationDirection::Left),
                stream,
            )
            .expect_err("paused move must not no-op");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
    }

    #[test]
    fn paused_change_layout_is_rejected_instead_of_applied() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        wm.is_paused = true;

        let stream = Vec::new();
        let error = wm
            .process_command(
                SocketMessage::ChangeLayout(crate::core::DefaultLayout::Columns),
                stream,
            )
            .expect_err("paused layout must not apply");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
        let layout = wm.focused_workspace().unwrap().layout.clone();
        assert!(
            matches!(
                layout,
                crate::core::Layout::Default(crate::core::DefaultLayout::BSP)
            ),
            "paused layout must stay BSP, got {layout:?}"
        );
    }

    #[test]
    fn change_layout_commits_the_focused_workspace_layout() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);

        let stream = Vec::new();
        wm.process_command(
            SocketMessage::ChangeLayout(crate::core::DefaultLayout::Columns),
            stream,
        )
        .unwrap();

        let layout = wm.focused_workspace().unwrap().layout.clone();
        assert!(
            matches!(
                layout,
                crate::core::Layout::Default(crate::core::DefaultLayout::Columns)
            ),
            "expected Columns, got {layout:?}"
        );
    }

    #[test]
    fn paused_toggle_float_is_rejected_instead_of_ignored() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        wm.is_paused = true;

        let stream = Vec::new();
        let error = wm
            .process_command(SocketMessage::ToggleFloat, stream)
            .expect_err("paused float must not reach Win32");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
    }

    #[test]
    fn paused_resize_window_axis_is_rejected_instead_of_applied() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        wm.is_paused = true;

        let stream = Vec::new();
        let error = wm
            .process_command(
                SocketMessage::ResizeWindowAxis(
                    crate::core::Axis::Horizontal,
                    crate::core::Sizing::Increase,
                ),
                stream,
            )
            .expect_err("paused resize must not apply");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
    }

    #[test]
    fn paused_cycle_focus_window_is_rejected_instead_of_ignored() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        wm.is_paused = true;

        let stream = Vec::new();
        let error = wm
            .process_command(
                SocketMessage::CycleFocusWindow(crate::core::CycleDirection::Next),
                stream,
            )
            .expect_err("paused cycle focus must not no-op");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
    }

    #[test]
    fn paused_cycle_move_window_is_rejected_instead_of_ignored() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        wm.is_paused = true;

        let stream = Vec::new();
        let error = wm
            .process_command(
                SocketMessage::CycleMoveWindow(crate::core::CycleDirection::Previous),
                stream,
            )
            .expect_err("paused cycle move must not no-op");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
    }

    #[test]
    fn paused_toggle_monocle_is_rejected_instead_of_ignored() {
        assert_paused_rejects(SocketMessage::ToggleMonocle);
    }

    #[test]
    fn paused_toggle_maximize_is_rejected_instead_of_ignored() {
        assert_paused_rejects(SocketMessage::ToggleMaximize);
    }

    #[test]
    fn paused_toggle_lock_is_rejected_instead_of_ignored() {
        assert_paused_rejects(SocketMessage::ToggleLock);
    }

    #[test]
    fn paused_stack_window_is_rejected_instead_of_ignored() {
        assert_paused_rejects(SocketMessage::StackWindow(
            crate::core::OperationDirection::Left,
        ));
    }

    #[test]
    fn paused_stack_all_is_rejected_instead_of_ignored() {
        assert_paused_rejects(SocketMessage::StackAll);
    }

    #[test]
    fn paused_focus_workspace_number_is_rejected_instead_of_applied() {
        let mut wm = paused_manager();
        let stream = Vec::new();
        let error = wm
            .process_command(SocketMessage::FocusWorkspaceNumber(5), stream)
            .expect_err("paused workspace focus must not apply");
        assert!(
            error.to_string().contains("unavailable"),
            "unexpected rejection: {error}"
        );
        assert_eq!(wm.focused_workspace_idx().unwrap(), 0);
    }

    #[test]
    fn paused_workspace_and_monitor_navigation_is_rejected() {
        assert_paused_rejects(SocketMessage::CycleFocusWorkspace(
            crate::core::CycleDirection::Next,
        ));
        assert_paused_rejects(SocketMessage::CycleFocusEmptyWorkspace(
            crate::core::CycleDirection::Previous,
        ));
        assert_paused_rejects(SocketMessage::FocusLastWorkspace);
        assert_paused_rejects(SocketMessage::CloseWorkspace);
        assert_paused_rejects(SocketMessage::FocusMonitorNumber(0));
        assert_paused_rejects(SocketMessage::CycleFocusMonitor(
            crate::core::CycleDirection::Next,
        ));
        assert_paused_rejects(SocketMessage::FocusMonitorAtCursor);
        assert_paused_rejects(SocketMessage::FocusWorkspaceNumbers(2));
        assert_paused_rejects(SocketMessage::FocusMonitorWorkspaceNumber(0, 1));
    }

    #[test]
    fn paused_window_lifecycle_and_layout_actions_are_rejected() {
        assert_paused_rejects(SocketMessage::Close);
        assert_paused_rejects(SocketMessage::Minimize);
        assert_paused_rejects(SocketMessage::ForceFocus);
        assert_paused_rejects(SocketMessage::Promote);
        assert_paused_rejects(SocketMessage::PromoteSwap);
        assert_paused_rejects(SocketMessage::PromoteFocus);
        assert_paused_rejects(SocketMessage::PromoteWindow(
            crate::core::OperationDirection::Left,
        ));
        assert_paused_rejects(SocketMessage::NewWorkspace);
        assert_paused_rejects(SocketMessage::ToggleTiling);
        assert_paused_rejects(SocketMessage::CycleLayout(
            crate::core::CycleDirection::Next,
        ));
        assert_paused_rejects(SocketMessage::FlipLayout(crate::core::Axis::Horizontal));
        assert_paused_rejects(SocketMessage::ToggleWorkspaceLayer);
        assert_paused_rejects(SocketMessage::MoveContainerToLastWorkspace);
        assert_paused_rejects(SocketMessage::SendContainerToLastWorkspace);
        assert_paused_rejects(SocketMessage::MoveContainerToWorkspaceNumber(1));
        assert_paused_rejects(SocketMessage::CycleMoveContainerToWorkspace(
            crate::core::CycleDirection::Next,
        ));
        assert_paused_rejects(SocketMessage::SendContainerToWorkspaceNumber(1));
        assert_paused_rejects(SocketMessage::CycleSendContainerToWorkspace(
            crate::core::CycleDirection::Previous,
        ));
        assert_paused_rejects(SocketMessage::MoveContainerToMonitorNumber(0));
        assert_paused_rejects(SocketMessage::CycleMoveContainerToMonitor(
            crate::core::CycleDirection::Next,
        ));
        assert_paused_rejects(SocketMessage::SendContainerToMonitorNumber(0));
        assert_paused_rejects(SocketMessage::CycleSendContainerToMonitor(
            crate::core::CycleDirection::Previous,
        ));
        assert_paused_rejects(SocketMessage::MoveContainerToMonitorWorkspaceNumber(0, 1));
        assert_paused_rejects(SocketMessage::SendContainerToMonitorWorkspaceNumber(0, 1));
        assert_paused_rejects(SocketMessage::MoveWorkspaceToMonitorNumber(0));
        assert_paused_rejects(SocketMessage::CycleMoveWorkspaceToMonitor(
            crate::core::CycleDirection::Next,
        ));
        assert_paused_rejects(SocketMessage::SwapWorkspacesToMonitorNumber(0));
        assert_paused_rejects(SocketMessage::PreselectDirection(
            crate::core::OperationDirection::Left,
        ));
        assert_paused_rejects(SocketMessage::CancelPreselect);
        assert_paused_rejects(SocketMessage::Retile);
        assert_paused_rejects(SocketMessage::RetileWithResizeDimensions);
        assert_paused_rejects(SocketMessage::ManageFocusedWindow);
        assert_paused_rejects(SocketMessage::UnmanageFocusedWindow);
        assert_paused_rejects(SocketMessage::AdjustContainerPadding(
            crate::core::Sizing::Increase,
            5,
        ));
        assert_paused_rejects(SocketMessage::AdjustWorkspacePadding(
            crate::core::Sizing::Decrease,
            5,
        ));
        assert_paused_rejects(SocketMessage::ToggleMouseFollowsFocus);
        assert_paused_rejects(SocketMessage::MouseFollowsFocus(true));
        assert_paused_rejects(SocketMessage::ToggleWindowContainerBehaviour);
        assert_paused_rejects(SocketMessage::ToggleFloatOverride);
        assert_paused_rejects(SocketMessage::ToggleWorkspaceWindowContainerBehaviour);
        assert_paused_rejects(SocketMessage::ToggleWorkspaceFloatOverride);
        assert_paused_rejects(SocketMessage::ToggleCrossMonitorMoveBehaviour);
        assert_paused_rejects(SocketMessage::ToggleMonocleFocusBehaviour);
    }

    #[test]
    fn live_toggle_mouse_follows_focus_flips_setting() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        let before = wm.mouse_follows_focus;

        let stream = Vec::new();
        wm.process_command(SocketMessage::ToggleMouseFollowsFocus, stream)
            .unwrap();
        assert_eq!(wm.mouse_follows_focus, !before);
    }

    #[test]
    fn live_resize_delta_uses_the_canonical_resize_step_action() {
        let mut wm = window_manager();
        let step = crate::core::ResizeStep::new(91).expect("test resize step is positive");

        wm.process_command(SocketMessage::ResizeDelta(step.get()), Vec::new())
            .expect("legacy request should converge on the canonical action");

        assert_eq!(wm.resize_step, step);
    }

    #[test]
    fn live_new_workspace_advances_focus() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        assert_eq!(wm.focused_workspace_idx().unwrap(), 0);

        let stream = Vec::new();
        wm.process_command(SocketMessage::NewWorkspace, stream)
            .unwrap();
        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
    }

    #[test]
    fn live_toggle_tiling_flips_focused_workspace() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        assert!(wm.focused_workspace().unwrap().tile);

        let stream = Vec::new();
        wm.process_command(SocketMessage::ToggleTiling, stream)
            .unwrap();
        assert!(!wm.focused_workspace().unwrap().tile);
    }

    #[test]
    fn paused_toggle_pause_is_still_available() {
        let mut wm = paused_manager();
        assert!(wm.is_paused);
        let stream = Vec::new();
        wm.process_command(SocketMessage::TogglePause, stream)
            .unwrap();
        assert!(!wm.is_paused);
    }

    #[test]
    fn paused_padding_and_workspace_setup_actions_are_rejected() {
        assert_paused_rejects(SocketMessage::FocusedWorkspaceContainerPadding(8));
        assert_paused_rejects(SocketMessage::FocusedWorkspacePadding(8));
        assert_paused_rejects(SocketMessage::ContainerPadding(0, 0, 8));
        assert_paused_rejects(SocketMessage::WorkspacePadding(0, 0, 8));
        assert_paused_rejects(SocketMessage::WorkspaceTiling(0, 0, false));
        assert_paused_rejects(SocketMessage::WorkspaceLayout(
            0,
            0,
            crate::core::DefaultLayout::Columns,
        ));
        assert_paused_rejects(SocketMessage::EnsureWorkspaces(0, 3));
        assert_paused_rejects(SocketMessage::ClearWorkspaceLayoutRules(0, 0));
        assert_paused_rejects(SocketMessage::ScrollingLayoutColumns(
            std::num::NonZeroUsize::new(3).unwrap(),
        ));
        assert_paused_rejects(SocketMessage::LockMonitorWorkspaceContainer(0, 0, 0));
        assert_paused_rejects(SocketMessage::UnlockMonitorWorkspaceContainer(0, 0, 0));
        assert_paused_rejects(SocketMessage::ToggleTitleBars);
        assert_paused_rejects(SocketMessage::EnforceWorkspaceRules);
        assert_paused_rejects(SocketMessage::SessionFloatRule);
        assert_paused_rejects(SocketMessage::ClearSessionFloatRules);
        assert_paused_rejects(SocketMessage::ResizeWindowEdge(
            crate::core::OperationDirection::Right,
            crate::core::Sizing::Increase,
        ));
        assert_paused_rejects(SocketMessage::WindowHidingBehaviour(
            crate::core::HidingBehaviour::Cloak,
        ));
        assert_paused_rejects(SocketMessage::CrossMonitorMoveBehaviour(
            crate::core::MoveBehaviour::Insert,
        ));
        assert_paused_rejects(SocketMessage::MonocleFocusBehaviour(
            crate::core::MonocleFocusBehaviour::Cycle,
        ));
        assert_paused_rejects(SocketMessage::UnmanagedWindowOperationBehaviour(
            crate::core::OperationBehaviour::NoOp,
        ));
        assert_paused_rejects(SocketMessage::FocusFollowsMouse(
            crate::core::FocusFollowsMouseImplementation::Windows,
            true,
        ));
        assert_paused_rejects(SocketMessage::ToggleFocusFollowsMouse(
            crate::core::FocusFollowsMouseImplementation::Windows,
        ));
        assert_paused_rejects(SocketMessage::WorkspaceLayoutRule(
            0,
            0,
            2,
            crate::core::DefaultLayout::Columns,
        ));
        assert_paused_rejects(SocketMessage::FocusNamedWorkspace("code".into()));
        assert_paused_rejects(SocketMessage::MoveContainerToNamedWorkspace("code".into()));
        assert_paused_rejects(SocketMessage::SendContainerToNamedWorkspace("code".into()));
        assert_paused_rejects(SocketMessage::NamedWorkspaceContainerPadding(
            "code".into(),
            8,
        ));
        assert_paused_rejects(SocketMessage::NamedWorkspacePadding("code".into(), 8));
        assert_paused_rejects(SocketMessage::NamedWorkspaceTiling("code".into(), false));
        assert_paused_rejects(SocketMessage::NamedWorkspaceLayout(
            "code".into(),
            crate::core::DefaultLayout::Columns,
        ));
        assert_paused_rejects(SocketMessage::ClearNamedWorkspaceLayoutRules("code".into()));
        assert_paused_rejects(SocketMessage::EnsureNamedWorkspaces(
            0,
            vec!["code".into(), "chat".into()],
        ));
        assert_paused_rejects(SocketMessage::WorkspaceName(0, 0, "code".into()));
        assert_paused_rejects(SocketMessage::LayoutRatios(Some(vec![0.5, 0.5]), None));
        assert_paused_rejects(SocketMessage::EagerFocus("wezterm-gui.exe".into()));
        assert_paused_rejects(SocketMessage::RemoveTitleBar(
            crate::core::ApplicationIdentifier::Exe,
            "wezterm-gui.exe".into(),
        ));
    }

    #[test]
    fn live_named_workspace_focus_after_naming() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);

        let stream = Vec::new();
        wm.process_command(SocketMessage::EnsureWorkspaces(0, 2), stream)
            .unwrap();
        let stream = Vec::new();
        wm.process_command(SocketMessage::WorkspaceName(0, 1, "chat".into()), stream)
            .unwrap();
        assert_eq!(wm.focused_workspace_idx().unwrap(), 0);

        let stream = Vec::new();
        wm.process_command(SocketMessage::FocusNamedWorkspace("chat".into()), stream)
            .unwrap();
        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);
        assert_eq!(
            wm.focused_workspace().unwrap().name.as_deref(),
            Some("chat")
        );
    }

    #[test]
    fn live_ensure_named_workspaces_names_the_ring() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);

        let stream = Vec::new();
        wm.process_command(
            SocketMessage::EnsureNamedWorkspaces(0, vec!["code".into(), "chat".into()]),
            stream,
        )
        .unwrap();
        let workspaces = wm.focused_monitor().unwrap().workspaces();
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].name.as_deref(), Some("code"));
        assert_eq!(workspaces[1].name.as_deref(), Some("chat"));
    }

    #[test]
    fn live_cross_monitor_move_behaviour_is_set() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        assert_eq!(
            wm.cross_monitor_move_behaviour,
            crate::core::MoveBehaviour::Swap
        );

        let stream = Vec::new();
        wm.process_command(
            SocketMessage::CrossMonitorMoveBehaviour(crate::core::MoveBehaviour::Insert),
            stream,
        )
        .unwrap();
        assert_eq!(
            wm.cross_monitor_move_behaviour,
            crate::core::MoveBehaviour::Insert
        );
    }

    #[test]
    fn live_ensure_workspaces_grows_the_ring() {
        let mut wm = window_manager();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );
        wm.monitors_mut().push_back(m);
        assert_eq!(wm.focused_monitor().unwrap().workspaces().len(), 1);

        let stream = Vec::new();
        wm.process_command(SocketMessage::EnsureWorkspaces(0, 4), stream)
            .unwrap();
        assert_eq!(wm.focused_monitor().unwrap().workspaces().len(), 4);
    }
}
