//! `virtual_items` through the real router: a 10,000-item list lays out about a screenful of rows, wheel scrolling
//! shows the right items, and a row's pin follows its item (not its position) across scrolling and inserts above it.
use kit_core::kurbo::{Point, Vec2};
use kit_core::{MotionPref, ScrollView, Theme, Time};
use kit_input::{DeltaUnit, InputEvent};
use kit_state::{Act, Cx, Host, Ident, Intent, Life};
use kit_test::Handle;
use kit_view::{Node, Role, Toggle, column, row};
use kit_widgets::{Press, Switch, VirtualList, switch, virtual_items};
use std::cell::Cell;

const ROW: f64 = 28.0;
const VIEW: f64 = 140.0; // five rows
const N: u32 = 10_000;

struct App {
    now: Time,
    items: Vec<u32>,
    scroll: Cell<ScrollView>,
}
impl Host for App {
    type Ask = ();
    fn now(&self) -> Time {
        self.now
    }
    fn motion(&self) -> MotionPref {
        MotionPref::Full
    }
    fn ask(&mut self, (): ()) {}
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Id;
impl Ident<App> for Id {
    fn life(&self, _: &App) -> Life {
        Life::Live
    }
}
/// A row's identity is the item id; it lives while the app lists the item (a hash set would be faster, the scan is
/// fine because the store only asks about keys it has seen, i.e. rows that were built).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Item(u32);
impl Ident<App> for Item {
    fn life(&self, app: &App) -> Life {
        match app.items.contains(&self.0) {
            true => Life::Live,
            false => Life::Over,
        }
    }
}

fn label(id: u32) -> String {
    format!("Pin {id}")
}
fn view(cx: &Cx<'_, App>, theme: &Theme) -> Node<Intent<App>> {
    let app = cx.app();
    app.scroll.set(cx.scroll(&Id));
    let render = |id: &u32| {
        let key = Item(*id);
        let pinned = cx.state(
            &key,
            || false,
            |pinned: &mut bool, _: &Item, (): (), _: &App| {
                *pinned = !*pinned;
                Act::none()
            },
        );
        let name = label(*id);
        row([switch(
            cx,
            Switch {
                theme,
                key,
                label: &name,
                on: *pinned,
                press: Press::Sends(pinned.send(())),
            },
        )])
    };
    column([virtual_items(
        cx,
        VirtualList {
            key: Id,
            items: &app.items,
            row_height: ROW,
            height: VIEW,
            row: &render,
        },
    )])
}
fn app(items: impl IntoIterator<Item = u32>) -> App {
    App {
        now: Time::default(),
        items: items.into_iter().collect(),
        scroll: Cell::new(ScrollView::Unseen),
    }
}
fn handle(
    items: impl IntoIterator<Item = u32>,
) -> Handle<App, impl Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>> {
    let mut h = Handle::new(app(items), (240.0, 200.0), view);
    h.scroll_by(0.0); // the first frame builds a stub window; the second has the measured viewport
    h
}
/// Test-local helpers on the shared harness (`kit_test::Handle` is another crate's type, so they are a trait).
trait ListHandle {
    fn scroll_by(&mut self, dy: f64);
    fn click_pin(&mut self, id: u32);
    fn offset(&self) -> f64;
    fn built(&self, ids: impl IntoIterator<Item = u32>) -> Vec<u32>;
}
impl<V: Fn(&Cx<'_, App>, &Theme) -> Node<Intent<App>>> ListHandle for Handle<App, V> {
    fn scroll_by(&mut self, dy: f64) {
        self.input(InputEvent::Wheel {
            at: Point::new(60.0, 60.0),
            delta: Vec2::new(0.0, dy),
            unit: DeltaUnit::Pixels,
        });
    }
    /// `Handle::click` aims at the laid rect, which is in content space: inside a scrolled area subtract the offset.
    fn click_pin(&mut self, id: u32) {
        let at = self.center(Role::CheckBox, &label(id));
        self.click_at(Point::new(at.x, at.y - self.offset()));
    }
    fn offset(&self) -> f64 {
        match self.app().scroll.get() {
            ScrollView::Seen { offset, .. } => offset.y,
            ScrollView::Unseen => panic!("list never measured"),
        }
    }
    fn built(&self, ids: impl IntoIterator<Item = u32>) -> Vec<u32> {
        ids.into_iter()
            .filter(|id| self.shows(Role::CheckBox, &label(*id)))
            .collect()
    }
}

#[test]
fn ten_thousand_items_lay_out_one_screenful() {
    let h = handle(0..N);
    // 5 rows in view plus 3 of overscan below; the top has none above it.
    assert_eq!(h.built(0..N), (0..8).collect::<Vec<_>>());
    // Everything painted comes from laid nodes: the same rows from a short list paint (about) the same.
    let eight = handle(0..8).draw().items.len();
    let many = h.draw().items.len();
    assert!(
        many <= eight + 4,
        "{many} draws for 10,000 items vs {eight} for 8 items"
    );
}

#[test]
fn the_scroll_extent_is_every_row() {
    let mut h = handle(0..N);
    match h.app().scroll.get() {
        ScrollView::Seen { content, .. } => {
            assert_eq!(
                content.height,
                f64::from(N) * ROW,
                "content height is N rows"
            )
        }
        ScrollView::Unseen => panic!("list never measured"),
    }
    h.scroll_by(1.0e9); // far past the end: clamps to the last row
    assert!(h.shows(Role::CheckBox, &label(N - 1)));
    assert!(!h.shows(Role::CheckBox, &label(N - 100)));
}

#[test]
fn wheel_scrolling_shows_the_items_at_that_offset() {
    let mut h = handle(0..N);
    h.scroll_by(ROW * 4000.0 + 10.0);
    assert_eq!(h.offset(), ROW * 4000.0 + 10.0);
    // Rows 4000..4005 intersect the viewport (offset 10 px into row 4000); overscan 3 each side.
    assert_eq!(h.built(0..N), (3997..4009).collect::<Vec<_>>());
    assert!(h.shows(Role::CheckBox, &label(4002)));
}

#[test]
fn a_pin_stays_with_its_item_across_scrolling_and_inserts_above() {
    let mut h = handle(0..N);
    h.scroll_by(ROW * 2000.0);
    h.click_pin(2001);
    assert_eq!(h.toggle_of(Role::CheckBox, &label(2001)), Toggle::On);

    h.scroll_by(ROW * 5000.0); // far away: item 2001 is not built, its state is kept
    assert!(!h.shows(Role::CheckBox, &label(2001)));
    h.scroll_by(-ROW * 5000.0);
    assert_eq!(h.toggle_of(Role::CheckBox, &label(2001)), Toggle::On);
    assert_eq!(h.toggle_of(Role::CheckBox, &label(2002)), Toggle::Off);

    // Two new items above: every row below shifts down two positions, the pin goes with its item.
    h.tick(|app| {
        app.items.splice(0..0, [N + 1, N + 2]);
    });
    h.scroll_by(ROW * 2.0);
    assert_eq!(h.toggle_of(Role::CheckBox, &label(2001)), Toggle::On);
    assert_eq!(h.toggle_of(Role::CheckBox, &label(2002)), Toggle::Off);
    assert_eq!(h.toggle_of(Role::CheckBox, &label(2000)), Toggle::Off);
}

#[test]
fn an_item_removed_while_scrolled_away_loses_its_state() {
    let mut h = handle(0..N);
    h.scroll_by(ROW * 2000.0);
    h.click_pin(2001);
    h.scroll_by(ROW * 5000.0);
    h.tick(|app| app.items.retain(|id| *id != 2001));
    h.tick(|app| app.items.insert(2001, 2001));
    h.scroll_by(-ROW * 5000.0);
    assert_eq!(
        h.toggle_of(Role::CheckBox, &label(2001)),
        Toggle::Off,
        "a re-added item starts fresh"
    );
}

#[test]
fn golden_idle() {
    let mut h = handle(0..N);
    h.assert_golden("virtual_list_idle");
}
