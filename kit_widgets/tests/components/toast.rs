//! Toast deadlines, presence, lease collection, queue limits and pointer dismissal.
use core::time::Duration;
use kit_core::{MotionPref, Theme, Time, WidgetId};
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_test::Handle;
use kit_view::{A11y, Bindings, Node, Role, Style, Toggle, column, interactive, row, text};
use kit_widgets::toast::{Toast, ToastStack, toast};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Id(&'static str);
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
#[derive(Clone, Debug)]
enum Ask {
    Tick,
    Add,
}
struct App {
    now: Time,
    added: bool,
    reduced: bool,
}
impl Host for App {
    type Ask = Ask;
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        if self.reduced {
            MotionPref::Reduced
        } else {
            MotionPref::Full
        }
    }
    fn ask(&mut self, ask: Ask) {
        match ask {
            Ask::Tick => self.now.0 += Duration::from_millis(60),
            Ask::Add => self.added = true,
        }
    }
}
fn at(ms: u64) -> Time {
    Time(Duration::from_millis(ms))
}
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    let theme = Theme::new(theme.vision, cx.app().motion());
    let notice = |label, born, expires| Toast {
        key: Id(label),
        label,
        born: at(born),
        expires: at(expires),
    };
    let mut items = vec![notice("Cargo loaded", 0, 960)];
    if cx.app().added {
        items.push(notice("Crew ready", 480, 1440));
        items.push(notice("Wind changed", 480, 2400));
    }
    let button = |label: &'static str, ask| {
        let face = text(label, kit_view::TextStyle::new(14.0, theme.colors.text)).pad(8.0);
        let a11y = A11y::Labeled {
            role: Role::Button,
            label: label.into(),
            toggle: Toggle::NotToggle,
        };
        interactive(
            WidgetId::of(&Id(label)),
            Bindings::activate(Intent::Ask(ask)),
            a11y,
            face,
        )
    };
    let controls = row([button("Tick", Ask::Tick), button("Add", Ask::Add)]);
    let notices = toast(
        cx,
        ToastStack {
            theme: &theme,
            items: &items,
            max_visible: 2,
        },
    );
    column([controls, notices]).styled(Style::column().padded(8.0))
}
fn handle(reduced: bool) -> Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>> {
    Handle::new(
        App {
            now: at(0),
            added: false,
            reduced,
        },
        (320.0, 180.0),
        view,
    )
}
fn tick(h: &mut Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>>, count: usize) {
    for _ in 0..count {
        h.click(Role::Button, "Tick");
    }
}

#[test]
fn golden_enter_shown_and_exit() {
    let mut h = handle(false);
    tick(&mut h, 1);
    h.assert_golden("toast_enter");
    tick(&mut h, 2);
    h.assert_golden("toast_shown");
    tick(&mut h, 5);
    h.click(Role::Button, "Add");
    tick(&mut h, 9);
    h.assert_golden("toast_exit");
    tick(&mut h, 2);
    h.assert_golden("toast_queued_enter");
}

#[test]
fn expired_toast_exits_before_collection_and_new_toasts_keep_old_deadlines() {
    let mut h = handle(false);
    tick(&mut h, 8);
    h.click(Role::Button, "Add");
    assert!(h.shows(Role::Button, "Cargo loaded"));
    assert!(h.shows(Role::Button, "Crew ready"));
    assert!(
        !h.shows(Role::Button, "Wind changed"),
        "only two toasts are visible"
    );
    tick(&mut h, 8);
    assert_eq!(h.app().now, at(960));
    assert!(
        h.shows(Role::Button, "Cargo loaded"),
        "expiry starts the exit, not immediate removal"
    );
    assert_eq!(h.store().len(), 2, "the exiting lease retains its state");
    tick(&mut h, 1);
    assert!(
        h.shows(Role::Button, "Cargo loaded"),
        "the exit lasts the full theme duration"
    );
    tick(&mut h, 1);
    assert!(
        !h.shows(Role::Button, "Cargo loaded"),
        "adding siblings must not restart the old timer"
    );
    assert!(
        h.shows(Role::Button, "Crew ready"),
        "the younger toast has its own deadline"
    );
    assert!(
        h.shows(Role::Button, "Wind changed"),
        "the next queued toast fills the vacancy"
    );
    assert_eq!(
        h.store().len(),
        2,
        "old lease collected before the queued toast enters"
    );
    tick(&mut h, 8);
    assert!(!h.shows(Role::Button, "Crew ready"));
    assert_eq!(h.store().len(), 1);
    tick(&mut h, 16);
    assert!(!h.shows(Role::Button, "Wind changed"));
    assert!(
        h.store().is_empty(),
        "retaining expired caller items never recreates their state"
    );
}

#[test]
fn clicking_dismisses_with_an_exit_and_never_resurrects_a_retained_item() {
    let mut h = handle(false);
    tick(&mut h, 3);
    h.click(Role::Button, "Cargo loaded");
    tick(&mut h, 1);
    assert!(h.shows(Role::Button, "Cargo loaded"));
    tick(&mut h, 1);
    assert!(!h.shows(Role::Button, "Cargo loaded"));
    tick(&mut h, 4);
    assert!(!h.shows(Role::Button, "Cargo loaded"));
    assert_eq!(
        h.store().len(),
        1,
        "dismissal stays remembered through the original lease"
    );
    tick(&mut h, 9);
    assert!(h.store().is_empty());
}

#[test]
fn reduced_motion_collects_at_expiry_without_an_exit_delay() {
    let mut h = handle(true);
    assert!(h.shows(Role::Button, "Cargo loaded"));
    tick(&mut h, 16);
    assert!(!h.shows(Role::Button, "Cargo loaded"));
    assert!(h.store().is_empty());
}
