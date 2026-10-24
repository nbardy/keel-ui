//! The size of a text field component. The whole component below is the one function `name_field`: keyed state,
//! the reducer, the node. Everything else in this file is the frame loop a host runs.
use kit_core::kurbo::{Point, Rect, Size};
use kit_core::peniko::Color;
use kit_core::{MotionPref, Time, Viewport, WidgetId};
use kit_input::*;
use kit_state::{Act, Cx, Host, Ident, Intent, Life, Store, dispatch};
use kit_view::testing::laid_out;
use kit_view::*;

struct App;
impl Host for App {
    type Ask = ();
    fn now(&self) -> Time {
        Time::default()
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, (): ()) {}
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Name;
impl Ident<App> for Name {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}

/// A single-line text field whose text lives in the kit's keyed state.
fn name_field(cx: &Cx<'_, App>) -> Node<Intent<App>> {
    let field = cx.state(
        &Name,
        || EditorState::single_line(""),
        |state: &mut EditorState, _: &Name, edit: Edit, _: &App| {
            state.apply(edit);
            Act::none()
        },
    );
    let send = field.sender();
    text_field(
        WidgetId::of(&Name),
        "Name",
        &field,
        TextStyle::new(16.0, Color::BLACK),
        move |edit| send.send(edit),
    )
}

#[test]
fn a_field_component_types_selects_and_undoes_through_cx_state() {
    let (mut app, mut store) = (App, Store::<App>::new());
    let mut router = Router::default();
    router.config.platform = Platform::Other;
    let mut memory = UiMemory::new(Viewport::new(Size::new(400.0, 100.0), 1.0));
    let mut frame = |event: InputEvent, app: &mut App, store: &mut Store<App>| {
        let cx = Cx::new(app, store);
        let laid = laid_out(
            column(vec![name_field(&cx)]),
            &[
                Rect::new(0.0, 0.0, 400.0, 100.0),
                Rect::new(0.0, 0.0, 200.0, 30.0),
            ],
        );
        let routed = router.route(&laid, &mut memory, event, Time::default());
        for intent in routed.messages() {
            dispatch(app, store, intent);
        }
        let cx = Cx::new(app, store);
        let laid = laid_out(
            column(vec![name_field(&cx)]),
            &[
                Rect::new(0.0, 0.0, 400.0, 100.0),
                Rect::new(0.0, 0.0, 200.0, 30.0),
            ],
        );
        match &laid.find(WidgetId::of(&Name)).expect("field").kind {
            LaidKind::TextField { view, .. } => (view.text.clone(), view.selection),
            _ => unreachable!("the field is a text field"),
        }
    };
    let press = |phase| {
        InputEvent::Pointer(PointerEvent {
            id: PointerId(0),
            kind: PointerKind::Mouse,
            phase,
            at: Point::new(5.0, 5.0),
            pressure: 1.0,
        })
    };
    frame(press(Phase::Down), &mut app, &mut store);
    frame(press(Phase::Up), &mut app, &mut store);
    frame(InputEvent::Text("hello".into()), &mut app, &mut store);
    let (text, _) = frame(InputEvent::Text(" world".into()), &mut app, &mut store);
    assert_eq!(text, "hello world");
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    let key = |c| InputEvent::Key {
        key: Key::character(c),
        state: KeyState::Down,
        mods: ctrl,
    };
    let (_, selection) = frame(key('a'), &mut app, &mut store);
    assert_eq!(selection.range(), 0..11);
    let (text, _) = frame(InputEvent::Text("bye".into()), &mut app, &mut store);
    assert_eq!(text, "bye");
    let (text, _) = frame(key('z'), &mut app, &mut store);
    assert_eq!(text, "hello world");
}
