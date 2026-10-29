//! Server authority: `cx.track` (what the server said about a command I sent) and `cx.read` (keep a resource loaded
//! while a view reads it), driven through the real router. The first program is the README's "Did the server accept
//! it?" snippet (the block between `README:begin` and `README:end`); change both together.
use kit_core::{MotionPref, Redraw, Time};
use kit_state::LateWhy;
use kit_test::Handle;
use kit_view::Role;
use kit_widgets::{Icon, Ink, PathEl};
use std::time::Duration;

const COIN: Icon = Icon::new(
    // Icons are drawn on a 32x32 canvas: a horizontal bar across the middle.
    &[PathEl::Move([8.0, 16.0]), PathEl::Line([24.0, 16.0])],
    Ink::Stroke(1.5),
);

// README:begin
use kit_core::Theme;
use kit_state::{Act, Command, Cx, Host, Ident, Intent, Life, Verdict};
use kit_view::{Node, column, row};
use kit_widgets::Ui;

struct Shop {
    items: Vec<u32>, // item ids, as the server last listed them
    sent: Vec<Buy>,  // what went out to the server
    now: Time,
}
#[derive(Clone, Debug, PartialEq)]
struct Buy(u32);
impl Host for Shop {
    type Ask = Buy;
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, buy: Buy) {
        self.sent.push(buy);
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Row(u32);
impl Ident<Shop> for Row {
    fn life(&self, shop: &Shop) -> Life {
        match shop.items.contains(&self.0) {
            true => Life::Live,
            false => Life::Over,
        }
    }
}

/// A kind of command whose answer the UI shows: why the server may refuse it, and how long to wait.
struct Buying;
#[derive(Clone, Debug, PartialEq)]
enum NoSale {
    NotEnoughGold,
}
impl Command for Buying {
    type Why = NoSale;
    const WITHIN: Duration = Duration::from_secs(5);
}

fn shop(cx: &Cx<'_, Shop>, theme: &Theme) -> Node<Intent<Shop>> {
    let ui = Ui::new(theme);
    column(cx.app().items.iter().map(|&id| {
        let key = Row(id);
        // The reducer says "this ask is a Buying"; the kit starts the clock.
        let buy = cx.state(
            &key,
            || (),
            |_: &mut (), key: &Row, (): (), _: &Shop| Act::ask_tracked::<Buying>(Buy(key.0)),
        );
        // The answer is whatever the host reported for this row's latest Buying, or Unknown after five seconds.
        let answer = match cx.track::<Buying>(&key) {
            Verdict::Unsent => "".to_string(),
            Verdict::Pending { .. } => "Buying...".to_string(),
            Verdict::Confirmed { .. } => "Bought".to_string(),
            Verdict::Refused {
                why: NoSale::NotEnoughGold,
                ..
            } => "Refused: not enough gold".to_string(),
            Verdict::Unknown { .. } => "No answer from the server".to_string(),
        };
        row([
            ui.labeled_button(COIN, format!("Buy {id}"))
                .on_press(buy.send(()))
                .into(),
            Node::from(ui.caption(answer)),
        ])
    }))
}
// README:end

fn store() -> Shop {
    Shop {
        items: vec![1, 2],
        sent: Vec::new(),
        now: Time::default(),
    }
}
fn secs(n: u64) -> Time {
    Time(Duration::from_secs(n))
}
fn has(h: &Handle<Shop, impl Fn(&Cx<'_, Shop>, &Theme) -> Node<Intent<Shop>>>, line: &str) -> bool {
    h.texts().iter().any(|t| t == line)
}
use kit_state::Outcome;

/// A sent command shows pending until the host reports the server's answer. Mutant: `Act::ask_tracked` sends
/// without recording (use `Act::ask`) and the row never leaves "no verdict".
#[test]
fn a_sent_command_is_pending_then_confirmed() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    assert!(!has(&h, "Buying..."));
    h.click(Role::Button, "Buy 1");
    assert_eq!(h.app().sent, vec![Buy(1)]);
    assert!(has(&h, "Buying..."));
    let answered = h.host(|app, store| store.settle::<Buying, _>(app, &Row(1), Outcome::Confirmed));
    assert_eq!(answered, Ok(kit_state::Settled::Shown));
    assert!(has(&h, "Bought"));
    assert!(!has(&h, "Buying..."));
}

/// A refusal carries the server's typed reason to the row that asked, and only that row.
#[test]
fn a_refusal_shows_its_reason_on_its_own_row() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.click(Role::Button, "Buy 2");
    h.host(|app, store| {
        store
            .settle::<Buying, _>(app, &Row(2), Outcome::Refused(NoSale::NotEnoughGold))
            .unwrap()
    });
    assert_eq!(
        h.texts()
            .iter()
            .filter(|t| *t == "Refused: not enough gold")
            .count(),
        1
    );
    assert!(!has(&h, "Buying..."));
}

/// No answer: the view wakes at the deadline (an idle app has no other reason to draw) and then shows Unknown.
/// A late answer still settles it. Mutant: drop `redraw_at` in `Cx::track` and the first assert fails; compute
/// Unknown against the wrong deadline and the second one does.
#[test]
fn no_answer_becomes_unknown_at_the_deadline_and_wakes_an_idle_app() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.tick(|app| app.now = secs(10));
    h.click(Role::Button, "Buy 1");
    assert_eq!(
        h.redraw_sources()[0],
        Redraw::At(secs(15)),
        "the view's own demand"
    );
    h.tick(|app| app.now = secs(14));
    assert!(has(&h, "Buying..."));
    h.tick(|app| app.now = secs(15));
    assert!(has(&h, "No answer from the server"));
    assert_eq!(h.redraw(), Redraw::Idle);
    h.host(|app, store| {
        store
            .settle::<Buying, _>(app, &Row(1), Outcome::Confirmed)
            .unwrap()
    });
    assert!(has(&h, "Bought"));
}

/// A frame loop in the starter's order (`examples_shared/starter_app.rs`): collect and fire timers, build the view,
/// lay out, route this frame's input against that layout, the router's frame step, paint.
struct Frames {
    store: kit_state::Store<Shop>,
    theme: Theme,
    viewport: kit_core::Viewport,
    text: kit_view::CosmicText,
    layout: kit_layout::TaffyLayout,
    router: kit_input::Router,
    memory: kit_input::UiMemory,
}
impl Frames {
    fn new() -> Self {
        let viewport = kit_core::Viewport::new(kit_core::kurbo::Size::new(420.0, 200.0), 1.0);
        Self {
            store: kit_state::Store::new(),
            theme: kit_test::theme(),
            viewport,
            text: kit_view::testing::text_system(),
            layout: kit_layout::TaffyLayout::new(),
            router: kit_input::Router::default(),
            memory: kit_input::UiMemory::new(viewport),
        }
    }
    /// One frame: the text it drew and its combined demand, which is all an event-driven host sleeps on.
    fn frame(
        &mut self,
        app: &mut Shop,
        events: Vec<kit_input::InputEvent>,
    ) -> (Vec<String>, Redraw) {
        self.store.collect(app);
        kit_state::wake_due(app, &mut self.store);
        let cx = Cx::routed(app, &self.store, &self.memory);
        let root = shop(&cx, &self.theme);
        let view_redraw = cx.redraw();
        let laid = self
            .layout
            .layout(root, self.viewport, &mut self.text, &kit_view::NoWorld)
            .unwrap();
        for event in events {
            for intent in self
                .router
                .route(&laid, &mut self.memory, event, app.now)
                .messages()
            {
                kit_state::dispatch(app, &mut self.store, intent);
            }
        }
        self.router.frame(&laid, &mut self.memory, app.now);
        self.memory.end_frame();
        let input_redraw = self.memory.redraw(&laid);
        let draws = kit_paint::paint(&laid, &self.memory, &self.theme, app.now);
        let demand = draws.frame_redraw(view_redraw, self.store.redraw(app), input_redraw);
        let mut texts = Vec::new();
        for node in laid.root.descendants() {
            if let kit_view::LaidKind::Text { layout, .. } = &node.kind {
                let lines = &self.text.buffer(layout.id).unwrap().lines;
                texts.extend(lines.iter().map(|line| line.text().to_string()));
            }
        }
        (texts, demand)
    }
    /// An event-driven host: draw again at once on `Now` (a 16 ms frame later), return on `At` or `Idle`. It never
    /// forces a tick: what happens next is up to the caller honouring the returned demand.
    fn settle(
        &mut self,
        app: &mut Shop,
        events: Vec<kit_input::InputEvent>,
    ) -> (Vec<String>, Redraw) {
        let mut drawn = self.frame(app, events);
        for _ in 0..120 {
            match drawn.1 {
                Redraw::Now => app.now = Time(app.now.0 + Duration::from_millis(16)),
                Redraw::At(_) | Redraw::Idle => return drawn,
            }
            drawn = self.frame(app, Vec::new());
        }
        panic!("the frame loop never settled");
    }
}

/// The starter builds before routing input, so the frame whose Enter sends a command drew the row as it was (Unsent):
/// that view asked for no deadline, and a keyboard press on an already focused button changes nothing else the router
/// or painter would wake for. Only the store's demand wakes the idle host to draw `Unknown`.
/// Mutant: omit tracked deadlines in Store::redraw; the frame's demand is Idle and the loop sleeps forever.
#[test]
fn store_demand_wakes_the_app_to_draw_a_tracked_timeout() {
    use kit_input::{InputEvent, NavAction};
    let mut app = store();
    app.now = secs(10);
    let mut frames = Frames::new();
    let focused = frames.settle(&mut app, vec![InputEvent::Nav(NavAction::Next)]);
    assert_eq!(
        focused.1,
        Redraw::Idle,
        "a focused, settled shop asks for nothing"
    );

    let sent_at = app.now;
    let (drawn, demand) = frames.frame(&mut app, vec![InputEvent::Nav(NavAction::Activate)]);
    assert_eq!(app.sent, [Buy(1)]);
    assert!(
        !drawn.iter().any(|t| t == "Buying..."),
        "the send's frame drew the pre-send view"
    );
    match demand {
        Redraw::At(deadline) => app.now = deadline,
        Redraw::Idle => {}
        Redraw::Now => panic!("a pending command must sleep until its deadline, not spin"),
    }
    assert_eq!(
        app.now,
        Time(sent_at.0 + Buying::WITHIN),
        "the store woke the host at the deadline"
    );

    let (drawn, demand) = frames.settle(&mut app, Vec::new());
    assert!(
        drawn.iter().any(|t| t == "No answer from the server"),
        "{drawn:?}"
    );
    assert_eq!(
        demand,
        Redraw::Idle,
        "an expired command must not keep the host awake"
    );
}

/// Replacement, superseded replies, settlement and collection must all change the same canonical deadline.
#[test]
fn store_tracks_the_earliest_unanswered_send_until_it_is_answered_or_collected() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.click(Role::Button, "Buy 1");
    h.tick(|app| app.now = secs(1));
    h.click(Role::Button, "Buy 2");
    h.tick(|app| app.now = secs(2));
    h.click(Role::Button, "Buy 1");
    assert_eq!(h.store().redraw(h.app()), Redraw::At(secs(6)));
    let superseded =
        h.host(|app, store| store.settle::<Buying, _>(app, &Row(1), Outcome::Confirmed));
    assert_eq!(superseded, Ok(kit_state::Settled::Superseded));
    h.host(|app, store| {
        store
            .settle::<Buying, _>(app, &Row(2), Outcome::Refused(NoSale::NotEnoughGold))
            .unwrap()
    });
    assert_eq!(h.store().redraw(h.app()), Redraw::At(secs(7)));
    h.tick(|app| app.items.retain(|&id| id != 1));
    assert_eq!(h.store().redraw(h.app()), Redraw::Idle);
    h.click(Role::Button, "Buy 2");
    h.host(|app, store| {
        store
            .settle::<Buying, _>(app, &Row(2), Outcome::Confirmed)
            .unwrap()
    });
    assert_eq!(h.store().redraw(h.app()), Redraw::Idle);
}

/// An answer for a row the server has since dropped is refused and counted, and the record went with the key: the
/// item coming back shows no stale verdict. Mutant: `settle` skips the life check and the answer lands (or panics) on
/// a collected slot; collect nothing and "Bought" greets the returning item.
#[test]
fn an_answer_after_its_key_ended_is_refused_and_counted() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.click(Role::Button, "Buy 1");
    h.tick(|app| app.items.retain(|&id| id != 1));
    let answered = h.host(|app, store| store.settle::<Buying, _>(app, &Row(1), Outcome::Confirmed));
    assert_eq!(answered, Err(LateWhy::Gone));
    assert_eq!(h.store().late_total(), 1);
    h.tick(|app| app.items.push(1));
    assert!(!has(&h, "Buying..."));
    assert!(!has(&h, "Bought"));
}

/// An answer to a command that was never sent is a typed error and a counted refusal, not a state change.
#[test]
fn an_answer_to_nothing_is_refused_and_counted() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.click(Role::Button, "Buy 1");
    h.host(|app, store| {
        store
            .settle::<Buying, _>(app, &Row(1), Outcome::Confirmed)
            .unwrap()
    });
    let stray = h.host(|app, store| {
        store.settle::<Buying, _>(app, &Row(1), Outcome::Refused(NoSale::NotEnoughGold))
    });
    assert_eq!(stray, Err(LateWhy::Unsent));
    let never = h.host(|app, store| store.settle::<Buying, _>(app, &Row(2), Outcome::Confirmed));
    assert_eq!(never, Err(LateWhy::Unsent));
    assert_eq!(h.store().late_total(), 2);
    assert!(
        has(&h, "Bought"),
        "the stray refusal must not overwrite the confirmed verdict"
    );
}

/// Two sends before any reply: replies are matched to sends in order, so the first reply (to the older send) leaves
/// the row waiting and only the second decides. Mutant: let every reply settle and the row shows "Bought" while the
/// newest purchase is still in flight.
#[test]
fn a_reply_to_an_older_send_does_not_settle_the_newest() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.click(Role::Button, "Buy 1");
    h.click(Role::Button, "Buy 1");
    let first = h.host(|app, store| store.settle::<Buying, _>(app, &Row(1), Outcome::Confirmed));
    assert_eq!(first, Ok(kit_state::Settled::Superseded));
    assert!(has(&h, "Buying..."));
    h.host(|app, store| {
        store
            .settle::<Buying, _>(app, &Row(1), Outcome::Refused(NoSale::NotEnoughGold))
            .unwrap()
    });
    assert!(has(&h, "Refused: not enough gold"));
}

mod reading {
    use super::*;
    use kit_state::{Loaded, Resource};
    use std::collections::HashMap;

    /// The wire: what the app is told about resources, and the panel's open switch.
    #[derive(Clone, Debug, PartialEq)]
    enum Wire {
        Want(&'static str),
        Release(&'static str),
        TogglePanel,
    }
    #[derive(Default)]
    struct Console {
        open: bool,
        held: HashMap<&'static str, Loaded<Vec<String>, String>>,
        wire: Vec<Wire>,
    }
    impl Host for Console {
        type Ask = Wire;
        fn now(&self) -> Time {
            Time::default()
        }
        fn motion(&self) -> MotionPref {
            MotionPref::Full
        }
        fn ask(&mut self, wire: Wire) {
            match wire {
                Wire::TogglePanel => self.open = !self.open,
                Wire::Want(_) | Wire::Release(_) => {}
            }
            self.wire.push(wire);
        }
    }
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    struct Feed(&'static str);
    impl Ident<Console> for Feed {
        fn life(&self, _: &Console) -> Life {
            Life::Live
        }
    }
    impl Resource<Console> for Feed {
        type Value = Vec<String>;
        type Error = String;
        fn state(&self, app: &Console) -> Loaded<Vec<String>, String> {
            app.held.get(self.0).cloned().unwrap_or(Loaded::Loading)
        }
        fn want(&self) -> Wire {
            Wire::Want(self.0)
        }
        fn release(&self) -> Wire {
            Wire::Release(self.0)
        }
    }

    /// The panel reads the feed only while open, and twice (header and body): two reads are one subscription.
    fn console(cx: &Cx<'_, Console>, theme: &Theme) -> Node<Intent<Console>> {
        let ui = Ui::new(theme);
        let toggle: Node<Intent<Console>> = ui
            .labeled_button(COIN, "Chat")
            .on_press(Intent::<Console>::Ask(Wire::TogglePanel))
            .into();
        match cx.app().open {
            false => column([toggle]),
            true => {
                let feed = Feed("lobby");
                let _header = cx.read(&feed);
                let body = match cx.read(&feed) {
                    Loaded::Loading => "Loading...".to_string(),
                    Loaded::Ready(lines) => lines.join(" / "),
                    Loaded::Failed(why) => format!("Failed: {why}"),
                };
                column([toggle, Node::from(ui.caption(body))])
            }
        }
    }

    /// Opening the panel subscribes (one `Want`), the app's data shows through, and closing it releases with no
    /// code in the view. Mutants: never emit `Want` and the first assert fails; skip the unread check in
    /// `sync_reads` and the release never comes; emit `Want` per read and it is asked twice.
    #[test]
    fn reading_subscribes_and_closing_the_reader_releases() {
        let mut h = Handle::new(Console::default(), (420.0, 200.0), console);
        assert_eq!(h.store().subscribed(), 0);
        h.click(Role::Button, "Chat");
        assert_eq!(h.app().wire, [Wire::TogglePanel, Wire::Want("lobby")]);
        assert!(has_line(&h, "Loading..."));

        h.host(|app, _| {
            app.held
                .insert("lobby", Loaded::Ready(vec!["hi".into(), "yo".into()]));
        });
        assert!(has_line(&h, "hi / yo"));
        h.host(|app, _| {
            app.held.insert("lobby", Loaded::Failed("offline".into()));
        });
        assert!(has_line(&h, "Failed: offline"));
        assert_eq!(h.app().wire.len(), 2, "staying open asks for nothing more");

        h.click(Role::Button, "Chat");
        assert_eq!(
            h.app().wire,
            [
                Wire::TogglePanel,
                Wire::Want("lobby"),
                Wire::TogglePanel,
                Wire::Release("lobby")
            ]
        );
        assert_eq!(h.store().subscribed(), 0);
        h.tick(|_| {});
        assert_eq!(
            h.app().wire.len(),
            4,
            "a released resource is released once"
        );
    }

    fn has_line(
        h: &Handle<Console, impl Fn(&Cx<'_, Console>, &Theme) -> Node<Intent<Console>>>,
        line: &str,
    ) -> bool {
        h.texts().iter().any(|t| t == line)
    }
}
