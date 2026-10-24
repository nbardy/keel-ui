//! The README's first example: a local app (no server) whose UI reads app data and asks for commands. If you change
//! the README's code, change it here too (the block between `README:begin` and `README:end` is the published snippet).
//!
//! It guards the claim the example makes: the app owns the values, and a control shows what the app holds, not what
//! the control last sent. An app that refuses a command (here: speed above 2x while recording) keeps the slider where it
//! was.
use crate::support::Handle;
use kit_core::{Key, MotionPref, NamedKey, Time};
use kit_view::Role;
use kit_widgets::{Icon, Ink, PathEl};

const DISK: Icon = Icon::new(
    // Icons are drawn on a 32x32 canvas: a square outline.
    &[
        PathEl::Move([8.0, 8.0]),
        PathEl::Line([24.0, 8.0]),
        PathEl::Line([24.0, 24.0]),
        PathEl::Line([8.0, 24.0]),
        PathEl::Close,
    ],
    Ink::Stroke(1.5),
);

// README:begin
use kit_core::Theme;
use kit_state::{Cx, Host, Intent};
use kit_view::{Node, column};
use kit_widgets::slider::Step;
use kit_widgets::{Slider, Ui, slider};

/// A simulation viewer. The app owns its data; the UI only reads it and asks for changes.
struct Viewer {
    speed: f64,
    recording: bool,
    saved: Vec<f64>, // speeds at which a snapshot was saved
    now: Time,
}

/// What the UI may ask the app to do. Could be anything: set a parameter, save a file, start a job, send a message.
#[derive(Clone, Debug, PartialEq)]
enum Command {
    Speed(f64),
    ToggleRecording,
    SaveSnapshot,
}

impl Host for Viewer {
    type Ask = Command;
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, command: Command) {
        match command {
            // The app decides: while recording, speed is capped at 2x.
            Command::Speed(speed) if self.recording && speed > 2.0 => {}
            Command::Speed(speed) => self.speed = speed,
            Command::ToggleRecording => self.recording = !self.recording,
            Command::SaveSnapshot => self.saved.push(self.speed),
        }
    }
}

fn controls(cx: &Cx<'_, Viewer>, theme: &Theme) -> Node<Intent<Viewer>> {
    let (app, ui) = (cx.app(), Ui::new(theme));
    let record = match app.recording {
        true => "Stop recording",
        false => "Record",
    };
    column([
        slider(Slider {
            theme,
            key: "speed",
            label: "Speed",
            value: app.speed, // always the app's value: a refused change snaps back
            range: 0.5..=4.0,
            step: Step::By(0.5),
            on_change: |speed| Intent::Ask(Command::Speed(speed)),
        }),
        ui.labeled_button(DISK, record)
            .on_press(Intent::Ask(Command::ToggleRecording))
            .into(),
        ui.labeled_button(DISK, "Save snapshot")
            .on_press(Intent::Ask(Command::SaveSnapshot))
            .into(),
    ])
}
// README:end

fn viewer() -> Viewer {
    Viewer {
        speed: 2.0,
        recording: false,
        saved: Vec::new(),
        now: Time::default(),
    }
}

type ViewerView = fn(&Cx<'_, Viewer>, &Theme) -> Node<Intent<Viewer>>;

fn arrow_right(h: &mut Handle<Viewer, ViewerView>) {
    h.press_key(Key::Named(NamedKey::ArrowRight));
}

/// The slider shows the app's value: a step the app refuses leaves it where it was, and the next step starts from
/// there. Mutant: a slider that keeps its own value (or an app that applies every `Speed`) moves to 2.5 here.
#[test]
fn a_refused_command_leaves_the_control_on_the_apps_value() {
    let mut h = Handle::new(
        viewer(),
        (320.0, 200.0),
        controls as fn(&Cx<'_, Viewer>, &Theme) -> Node<Intent<Viewer>>,
    );
    h.press_key(Key::Named(NamedKey::Tab)); // focus the slider; it keeps focus below
    h.tick(|viewer| viewer.recording = true);
    arrow_right(&mut h);
    assert_eq!(h.app().speed, 2.0, "recording caps speed at 2x");
    h.tick(|viewer| viewer.recording = false);
    arrow_right(&mut h);
    assert_eq!(
        h.app().speed,
        2.5,
        "one step from the app's value, not from a refused one"
    );
}

/// The record button's label is the app's state, and pressing it asks the app to change it.
#[test]
fn the_record_button_shows_and_toggles_the_apps_recording_state() {
    let mut h = Handle::new(
        viewer(),
        (320.0, 200.0),
        controls as fn(&Cx<'_, Viewer>, &Theme) -> Node<Intent<Viewer>>,
    );
    h.click(Role::Button, "Record");
    assert!(h.app().recording);
    assert!(h.shows(Role::Button, "Stop recording"));
}

/// A command reaches the app as the app's own value and reads the app's current state.
#[test]
fn save_snapshot_records_the_current_speed() {
    let mut h = Handle::new(
        viewer(),
        (320.0, 200.0),
        controls as fn(&Cx<'_, Viewer>, &Theme) -> Node<Intent<Viewer>>,
    );
    h.press_key(Key::Named(NamedKey::Tab));
    arrow_right(&mut h);
    h.click(Role::Button, "Save snapshot");
    assert_eq!(h.app().saved, vec![2.5]);
}
