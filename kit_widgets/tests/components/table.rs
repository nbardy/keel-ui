//! Table regressions through real routing, layout, state dispatch and Inter rasterization.
use crate::support::{Asked, Handle, TestApp};
use kit_core::kurbo::{Point, Vec2};
use kit_core::{Key, NamedKey, Theme};
use kit_input::{DeltaUnit, InputEvent};
use kit_state::{Act, Cx, Ident, Intent, Life};
use kit_view::{Node, Role, Toggle};
use kit_widgets::table::{Column, Selection, Sort, Sorting, Table, Width, table};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Id(&'static str);
impl Ident<TestApp> for Id {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}
#[derive(Clone, Debug, PartialEq)]
enum Ev {
    Sort(Sort),
    Pick(u64),
}
#[derive(Clone)]
struct Ship {
    id: u64,
    name: &'static str,
    gold: u32,
}
const SHIPS: [Ship; 3] = [
    Ship {
        id: 42,
        name: "Cutter",
        gold: 40,
    },
    Ship {
        id: 7,
        name: "Brig",
        gold: 70,
    },
    Ship {
        id: 99,
        name: "Sloop",
        gold: 40,
    },
];
fn view(cx: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let state = cx.state(
        &Id("model"),
        || (Sort::None, Selection::None),
        |s: &mut (Sort, Selection<u64>), _: &Id, ev: Ev, _: &TestApp| {
            match ev {
                Ev::Sort(sort) => s.0 = sort,
                Ev::Pick(id) => s.1 = Selection::Row(id),
            }
            Act::ask(Asked(format!("{ev:?}")))
        },
    );
    let columns = [
        Column {
            label: "Ship",
            width: Width::Fraction(1.0),
            sorting: Sorting::Sortable,
        },
        Column {
            label: "Gold",
            width: Width::Fixed(90.0),
            sorting: Sorting::Sortable,
        },
        Column {
            label: "Port",
            width: Width::Fraction(1.0),
            sorting: Sorting::Plain,
        },
    ];
    let cell = |ship: &Ship, col| match col {
        0 => ship.name.to_owned(),
        1 => ship.gold.to_string(),
        2 => "Harbour".to_owned(),
        other => panic!("invalid column {other}"),
    };
    let compare = |col, a: &Ship, b: &Ship| match col {
        0 => a.name.cmp(b.name),
        1 => a.gold.cmp(&b.gold),
        other => panic!("unsortable column {other}"),
    };
    table(
        cx,
        Table {
            theme,
            key: Id("table"),
            columns: &columns,
            rows: &SHIPS,
            row_id: &|r| r.id,
            row_label: &|r| r.name.to_owned(),
            cell: &cell,
            compare: &compare,
            sort: state.0,
            on_sort: &|s| state.send(Ev::Sort(s)),
            selected: state.1.clone(),
            on_select: &|id| state.send(Ev::Pick(id)),
            row_height: 32.0,
            height: 96.0,
        },
    )
    .pad(theme.spacing.sm)
}
fn handle() -> Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>> {
    Handle::new(TestApp::new(), (390.0, 150.0), view)
}
#[test]
fn golden_idle_ascending_selected_and_descending() {
    let mut h = handle();
    h.assert_golden("table_idle");
    h.click(Role::Option, "Brig");
    h.click(Role::Button, "Ship");
    h.assert_golden("table_ascending_selected");
    h.click(Role::Button, "Ship");
    h.assert_golden("table_descending_selected");
}
/// Sorting must change physical row order, restore input order at None, and keep selection on a data id.
#[test]
fn sort_cycles_ascending_descending_none_and_selection_follows_data_id() {
    let mut h = handle();
    h.click(Role::Option, "Brig");
    for (expected, first) in [
        (Sort::Ascending(0), 7),
        (Sort::Descending(0), 99),
        (Sort::None, 42),
    ] {
        h.click(Role::Button, "Ship");
        assert_eq!(
            h.app().asked.last(),
            Some(&Asked(format!("Sort({expected:?})")))
        );
        assert_eq!(h.toggle_of(Role::Option, "Brig"), Toggle::On);
        assert_eq!(h.toggle_of(Role::Option, "Cutter"), Toggle::Off);
        assert_eq!(h.toggle_of(Role::Option, "Sloop"), Toggle::Off);
        h.click_at(Point::new(45.0, 52.0)); // First body row, after padding + header.
        assert_eq!(h.app().asked.last(), Some(&Asked(format!("Pick({first})"))));
        h.click(Role::Option, "Brig");
    }
    h.click(Role::Button, "Ship");
    h.click(Role::Button, "Gold");
    assert_eq!(
        h.app().asked.last(),
        Some(&Asked("Sort(Ascending(1))".into()))
    );
    h.click_at(Point::new(45.0, 52.0));
    assert_eq!(
        h.app().asked.last(),
        Some(&Asked("Pick(42)".into())),
        "equal gold values preserve input order"
    );
    assert!(!h.shows(Role::Button, "Port"), "plain headers cannot sort");
}
#[test]
fn space_sorts_only_the_focused_header() {
    let mut h = handle();
    h.press_key(Key::Named(NamedKey::Space));
    assert!(h.app().asked.is_empty());
    h.press_key(Key::Named(NamedKey::Tab));
    h.press_key(Key::Named(NamedKey::Space));
    assert_eq!(h.app().asked, [Asked("Sort(Ascending(0))".into())]);
}
#[test]
fn virtual_rows_build_a_bounded_window_and_header_stays_put() {
    let rows: Vec<_> = (0..10_000_u64).collect();
    let render = |cx: &Cx<'_, TestApp>, theme: &Theme| {
        let column = [Column {
            label: "Id",
            width: Width::Fraction(1.0),
            sorting: Sorting::Sortable,
        }];
        table(
            cx,
            Table {
                theme,
                key: Id("many"),
                columns: &column,
                rows: &rows,
                row_id: &|r| *r,
                row_label: &|r| format!("Row {r}"),
                cell: &|r, _| r.to_string(),
                compare: &|_, a, b| a.cmp(b),
                sort: Sort::None,
                selected: Selection::None,
                on_sort: &|_| Intent::All(vec![]),
                on_select: &|_| Intent::All(vec![]),
                row_height: 32.0,
                height: 96.0,
            },
        )
    };
    let mut h = Handle::new(TestApp::new(), (200.0, 140.0), render);
    h.click_at(Point::new(190.0, 135.0)); // Second frame measures the initial virtual window.
    assert!(h.shows(Role::Option, "Row 0"));
    assert!(!h.shows(Role::Option, "Row 100"));
    h.input(InputEvent::Wheel {
        at: Point::new(50.0, 70.0),
        delta: Vec2::new(0.0, 3200.0),
        unit: DeltaUnit::Pixels,
    });
    assert!(h.shows(Role::Button, "Id"));
    assert!(h.shows(Role::Option, "Row 100"));
    assert!(!h.shows(Role::Option, "Row 0"));
}
