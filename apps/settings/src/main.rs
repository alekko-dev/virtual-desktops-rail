#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod report;
use rail_settings_protocol::{self as wire, Action, Request, State};
use std::io::BufReader;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use windows_reactor::*;

type InputPipe = Arc<Mutex<BufReader<std::io::Stdin>>>;

fn app_icon_path() -> &'static str {
    static ICON: OnceLock<String> = OnceLock::new();
    ICON.get_or_init(|| {
        std::env::current_exe()
            .expect("Settings executable path")
            .with_file_name("rail.ico")
            .to_string_lossy()
            .into_owned()
    })
    .as_str()
}

fn light_switch_radio(
    label: &str,
    target: rail_core::LightSwitchTarget,
    selected: rail_core::LightSwitchTarget,
    enabled: bool,
    send: impl Fn(Action) + 'static,
) -> View {
    RadioButton::new()
        .group_name("light-switch")
        .is_enabled(enabled)
        .is_checked(selected == target)
        // Reactor 0.100 reports IsChecked changes in both directions. An old
        // group's false event must never compete with the newly selected target.
        .on_checked(move |checked| {
            if checked {
                send(Action::LightSwitchTarget { target });
            }
        })
        .content(label.to_string())
}
#[derive(Clone)]
struct Input {
    state: State,
    pipe: InputPipe,
    output: mpsc::SyncSender<Request>,
}
impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state && Arc::ptr_eq(&self.pipe, &other.pipe)
    }
}
#[derive(Clone)]
enum Message {
    State(Result<State, String>),
    Action(Action),
    Report,
    Close,
    TimedOut(u64),
}
struct Settings {
    input: Input,
    sequence: u64,
    pending: Option<u64>,
    error: String,
}
impl Settings {
    fn receive(&self, context: &ComponentContext<Self>) {
        let pipe = Arc::clone(&self.input.pipe);
        context.spawn_background(move |_| Message::State(wire::read(&mut *pipe.lock().unwrap())));
    }
}
impl Component for Settings {
    type Input = Input;
    type Message = Message;
    fn create(input: &Input, context: &ComponentContext<Self>) -> Self {
        let _ = input.output.try_send(Request {
            version: wire::VERSION,
            id: 0,
            action: Action::Ready,
        });
        let settings = Self {
            input: input.clone(),
            sequence: 0,
            pending: None,
            error: String::new(),
        };
        settings.receive(context);
        settings
    }
    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::TimedOut(id) => {
                if self.pending == Some(id) {
                    self.pending = None;
                    self.error = "The rail has not confirmed this change. Check its status or reopen Settings.".into();
                }
            }
            Message::State(Ok(state)) if state.version == wire::VERSION => {
                if self.pending.is_some_and(|id| state.acknowledged >= id) {
                    self.pending = None;
                }
                self.input.state = state;
                self.receive(context);
            }
            Message::State(_) | Message::Close => {
                let _ = context.window().request_close();
            }
            Message::Report => match report::save() {
                Ok(Some(path)) => self.update(
                    Message::Action(Action::Report { destination: path }),
                    context,
                ),
                Ok(None) => {}
                Err(error) => self.error = error,
            },
            Message::Action(action) => {
                if self.pending.is_some() || !self.input.state.loaded {
                    return;
                }
                self.sequence += 1;
                let request = Request {
                    version: wire::VERSION,
                    id: self.sequence,
                    action,
                };
                if self.input.output.try_send(request).is_ok() {
                    self.pending = Some(self.sequence);
                    self.error.clear();
                    let id = self.sequence;
                    context.spawn_background(move |_| {
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        Message::TimedOut(id)
                    });
                } else {
                    self.error =
                        "Settings connection is busy or closed. Close and reopen Settings.".into();
                }
            }
        }
    }
    fn view(&self, _: &Input, context: &mut ViewContext<Self>) -> View {
        context.window_title("Virtual Desktop Rail — Settings");
        context.window_visuals(
            WindowVisuals::new()
                .icon(app_icon_path())
                .client_size(560.0, 620.0)
                .constraints(WindowConstraints {
                    min_width: Some(380.0),
                    min_height: Some(320.0),
                    ..Default::default()
                })
                .theme(WindowTheme::System),
        );
        let state = &self.input.state;
        let enabled = state.loaded && self.pending.is_none();
        let heading = |text: &str| {
            TextBlock::new()
                .text(text)
                .font_size(18.0)
                .margin(Thickness::new(0.0, 12.0, 0.0, 2.0))
        };
        let check = |label: &str, checked: bool, available: bool, action: fn(bool) -> Action| {
            CheckBox::new()
                .is_checked(checked)
                .is_enabled(enabled && available)
                .on_is_checked_changed(
                    context.callback(move |value| Message::Action(action(value))),
                )
                .content(label.to_string())
        };
        let radio = |label: &str, target: rail_core::LightSwitchTarget| {
            let sender = context.sender();
            light_switch_radio(
                label,
                target,
                state.light_switch_target,
                enabled,
                move |action| {
                    sender.send(Message::Action(action));
                },
            )
        };
        let children: Vec<View> = vec![
            heading("Window behavior").into(),
            check(
                "Pause automatic window actions",
                state.paused,
                true,
                |enabled| Action::Pause { enabled },
            ),
            check(
                "Restore saved layouts on desktop switch",
                state.auto_restore,
                true,
                |enabled| Action::AutoRestore { enabled },
            ),
            check(
                "Minimize snapped windows on other desktops",
                state.minimize_snapped_elsewhere,
                state.auto_restore,
                |enabled| Action::MinimizeSnappedElsewhere { enabled },
            ),
            heading("Startup").into(),
            check(
                "Start with Windows",
                state.startup,
                !state.startup_busy,
                |enabled| Action::Startup { enabled },
            ),
            heading("Appearance").into(),
            check(
                "Tint rail with active desktop color",
                state.desktop_tint,
                true,
                |enabled| Action::DesktopTint { enabled },
            ),
            check(
                "Show desktop name",
                state.show_desktop_name,
                true,
                |enabled| Action::ShowDesktopName { enabled },
            ),
            TextBlock::new()
                .text("Light switch toggles")
                .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
                .into(),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(16.0)
                .children((
                    radio("applications", rail_core::LightSwitchTarget::Applications),
                    radio("Windows", rail_core::LightSwitchTarget::Windows),
                    radio("both", rail_core::LightSwitchTarget::Both),
                )),
            heading("Desktop integration").into(),
            Grid::new()
                .columns([GridLength::STAR, GridLength::STAR])
                .column_spacing(12.0)
                .children((
                    Button::new()
                        .horizontal_alignment(HorizontalAlignment::Stretch)
                        .is_enabled(enabled)
                        .on_click(context.message(Message::Action(Action::Retry)))
                        .content(
                            TextBlock::new()
                                .text("Retry desktop integration")
                                .text_wrapping(TextWrapping::Wrap),
                        ),
                    Button::new()
                        .horizontal_alignment(HorizontalAlignment::Stretch)
                        .is_enabled(enabled && !state.report_busy)
                        .on_click(context.message(Message::Report))
                        .grid_column(1)
                        .content(
                            TextBlock::new()
                                .text("Save diagnostic report…")
                                .text_wrapping(TextWrapping::Wrap),
                        ),
                )),
        ];
        let children: Vec<KeyedView> = children
            .into_iter()
            .enumerate()
            .map(|(index, view)| KeyedView::new(index.to_string(), view))
            .collect();
        let mut notices = Vec::new();
        for (key, text) in [("notice", &state.notice), ("error", &self.error)] {
            if !text.is_empty() {
                notices.push(KeyedView::new(
                    key,
                    TextBlock::new()
                        .text(text)
                        .text_wrapping(TextWrapping::Wrap)
                        .is_text_selection_enabled(true),
                ));
            }
        }
        let has_notices = !notices.is_empty();
        let mut footer_children: Vec<KeyedView> = vec![
            KeyedView::new(
                "status",
                StackPanel::new().spacing(4.0).children((
                    TextBlock::new()
                        .text(&state.status)
                        .text_wrapping(TextWrapping::Wrap),
                    TextBlock::new()
                        .text(format!("Version {}", state.build))
                        .font_size(12.0),
                )),
            ),
            KeyedView::new(
                "done",
                Button::new()
                    .width(88.0)
                    .height(34.0)
                    .horizontal_alignment(HorizontalAlignment::Right)
                    .vertical_alignment(VerticalAlignment::Center)
                    .on_click(context.message(Message::Close))
                    .grid_column(1)
                    .content("Done"),
            ),
        ];
        if has_notices {
            footer_children.push(KeyedView::new(
                "notices",
                ScrollViewer::new()
                    .max_height(160.0)
                    .grid_row(1)
                    .grid_column_span(2)
                    .content(StackPanel::new().spacing(6.0).keyed_children(notices)),
            ));
        }
        let footer = Border::new()
            .background(ThemeBrush::CardBackground)
            .border_brush(ThemeBrush::CardStroke)
            .border_thickness(Thickness::new(0.0, 1.0, 0.0, 0.0))
            .padding(Thickness::new(24.0, 16.0, 24.0, 16.0))
            .grid_row(1)
            .content(
                Grid::new()
                    // Reserve Done's width so native hover/focus measurement cannot
                    // change the status wrapping and therefore the footer height.
                    .columns([GridLength::STAR, GridLength::Pixel(88.0)])
                    .rows([GridLength::Auto, GridLength::Auto])
                    .column_spacing(16.0)
                    .keyed_children(footer_children),
            );
        Grid::new()
            .background(ThemeBrush::SolidBackground)
            .rows([GridLength::STAR, GridLength::Auto])
            .children((
                ScrollViewer::new().content(
                    StackPanel::new()
                        .spacing(6.0)
                        .margin(Thickness::new(24.0, 24.0, 24.0, 12.0))
                        .keyed_children(children),
                ),
                footer,
            ))
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() != Some("--pipe") {
        return;
    }
    let pipe = Arc::new(Mutex::new(BufReader::new(std::io::stdin())));
    let state: State = match wire::read::<State>(&mut *pipe.lock().unwrap()) {
        Ok(state) if state.version == wire::VERSION => state,
        _ => return,
    };
    let (output, requests) = mpsc::sync_channel::<Request>(8);
    std::thread::spawn(move || {
        let mut stdout = std::io::stdout().lock();
        while let Ok(request) = requests.recv() {
            if wire::write(&mut stdout, &request).is_err() {
                std::process::exit(1);
            }
        }
    });
    if let Err(error) = App::run_component::<Settings>(Input {
        state,
        pipe,
        output,
    }) {
        eprintln!("Settings: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use windows_reactor::test::{EventId, EventPayload, Pump, QueuedEvent, RecordingRuntime};

    #[test]
    fn radio_group_deselection_never_sends_the_old_target() {
        use rail_core::LightSwitchTarget::{Applications, Both, Windows};
        for previous in [Applications, Windows, Both] {
            for next in [Applications, Windows, Both] {
                if previous == next {
                    continue;
                }
                let sent = Rc::new(RefCell::new(Vec::new()));
                let views = [Applications, Windows, Both].map(|target| {
                    let sent = Rc::clone(&sent);
                    light_switch_radio("target", target, previous, true, move |action| {
                        sent.borrow_mut().push(action)
                    })
                });
                let mut pump = Pump::new(RecordingRuntime::default());
                pump.mount_view(StackPanel::new().children(views)).unwrap();
                let root = pump.root().unwrap();
                let nodes = pump.runtime().node(root).unwrap().children().to_vec();
                for (target, checked) in [(previous, false), (next, true)] {
                    let index = [Applications, Windows, Both]
                        .iter()
                        .position(|value| *value == target)
                        .unwrap();
                    let node = nodes[index];
                    let revision = pump
                        .event_revision(node, EventId::RadioButtonChecked)
                        .unwrap();
                    pump.queue_event(QueuedEvent::new(
                        node,
                        EventId::RadioButtonChecked,
                        revision,
                        EventPayload::Bool(checked),
                    ));
                }
                pump.dispatch_events().unwrap();
                assert_eq!(
                    *sent.borrow(),
                    vec![Action::LightSwitchTarget { target: next }],
                    "{previous:?} -> {next:?}"
                );
            }
        }
    }
}
