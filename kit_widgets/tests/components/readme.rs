//! The example at the top of `keel/crates/kit/README.md`, compiled and driven by real clicks. If you change the
//! README's code, change it here too (the block between `README:begin` and `README:end` is the published snippet).
//!
//! It guards the two claims the README leads with: local state declared inside a loop follows its DATA key when the
//! list reorders (a hook-order or position-keyed store would move the pin to the wrong row), and a key's `life`
//! collects its state when the server stops listing it.
use crate::support::Handle;
use kit_core::{MotionPref, Time};
use kit_view::{Role, Toggle};
use kit_widgets::{Icon, Ink, PathEl};

const COIN: Icon = Icon::new(
    // Icons are drawn on a 32x32 canvas: a horizontal bar across the middle.
    &[PathEl::Move([8.0, 16.0]), PathEl::Line([24.0, 16.0])],
    Ink::Stroke(1.5),
);

// README:begin
use kit_core::Theme;
use kit_state::{Act, Cx, Host, Ident, Intent, Life};
use kit_view::{Node, column, row};
use kit_widgets::{Press, Switch, Ui, switch};

/// Your app model, server data included. The kit never owns it.
struct Shop {
    items: Vec<Item>, // as the server last sent them
    sent: Vec<Buy>,   // what the UI asked the server for
    now: Time,
}
struct Item {
    id: u32,
    name: &'static str,
}
#[derive(Clone, Debug, PartialEq)]
struct Buy(u32);

impl Host for Shop {
    type Ask = Buy; // what reducers may ask the app to do (here: send to the server)
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

/// A row's identity is the item's id, and it lives exactly as long as the server lists the item.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Row(u32);
impl Ident<Shop> for Row {
    fn life(&self, shop: &Shop) -> Life {
        match shop.items.iter().any(|item| item.id == self.0) {
            true => Life::Live,
            false => Life::Over,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum RowEvent {
    Pin,
    Buy,
}

/// A component is a plain function: app data and props in, a value tree out. It re-runs every frame.
fn shop(cx: &Cx<'_, Shop>, theme: &Theme) -> Node<Intent<Shop>> {
    let ui = Ui::new(theme);
    column(cx.app().items.iter().map(|item| {
        let key = Row(item.id);
        // Local state inside a loop, keyed by data (not call order). The reducer cannot capture anything.
        let pinned = cx.state(
            &key,
            || false,
            |pinned: &mut bool, key: &Row, event: RowEvent, _: &Shop| match event {
                RowEvent::Pin => {
                    *pinned = !*pinned;
                    Act::none()
                }
                RowEvent::Buy => Act::ask(Buy(key.0)),
            },
        );
        row([
            switch(
                cx,
                Switch {
                    theme,
                    key: key.clone(),
                    label: item.name,
                    on: *pinned,
                    press: Press::Sends(pinned.send(RowEvent::Pin)),
                },
            ),
            ui.labeled_button(COIN, format!("Buy {}", item.name))
                .on_press(pinned.send(RowEvent::Buy)) // a press is a value: testable, replayable
                .into(),
        ])
    }))
}
// README:end

fn store() -> Shop {
    Shop {
        items: vec![
            Item {
                id: 1,
                name: "Cannon",
            },
            Item {
                id: 2,
                name: "Sail",
            },
        ],
        sent: Vec::new(),
        now: Time::default(),
    }
}

/// The README's headline: state declared in a loop stays with its item when the list changes order. Mutant: key the
/// state by position (`Row(index)`) and the pin moves to "Hull" here.
#[test]
fn a_pin_follows_its_item_when_the_list_reorders() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.click(Role::CheckBox, "Sail");
    h.tick(|shop| {
        shop.items.insert(
            0,
            Item {
                id: 3,
                name: "Hull",
            },
        )
    });
    assert_eq!(h.toggle_of(Role::CheckBox, "Sail"), Toggle::On);
    assert_eq!(h.toggle_of(Role::CheckBox, "Hull"), Toggle::Off);
    assert_eq!(h.toggle_of(Role::CheckBox, "Cannon"), Toggle::Off);
}

/// A key's life ends with the server's listing: an item that leaves and comes back starts fresh. Mutant: `life`
/// always `Live` and the old pin survives.
#[test]
fn an_items_state_is_collected_when_the_server_stops_listing_it() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.click(Role::CheckBox, "Cannon");
    h.tick(|shop| shop.items.retain(|item| item.id != 1));
    h.tick(|shop| {
        shop.items.push(Item {
            id: 1,
            name: "Cannon",
        })
    });
    assert_eq!(h.toggle_of(Role::CheckBox, "Cannon"), Toggle::Off);
}

/// The press reaches the host as the app's own ask, naming the item the player saw.
#[test]
fn buy_asks_the_host_for_the_item_pressed() {
    let mut h = Handle::new(store(), (420.0, 200.0), shop);
    h.click(Role::Button, "Buy Sail");
    assert_eq!(h.app().sent, vec![Buy(2)]);
}
