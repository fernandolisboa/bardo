//! The guided tour's mechanics (issue #105): where each [`TourAnchor`] is on
//! screen, and the spotlight that dims the window around one of them.
//!
//! The layouts and screens tag what they draw ([`Anchored::tour_anchor`],
//! [`crate::kit::anchor`]); each tag records its bounds while the window
//! prepaints, into a map rebuilt every frame. The spotlight is drawn
//! deferred, after everything else has prepainted, so it reads the bounds
//! of the same frame: resizing the window or switching the layout, the
//! theme or the language mid-tour only moves the light.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bardo_app::{Side, Spot, TourAnchor};
use gpui_kit::prelude::*;
use gpui_kit::{
    AlignItems, AnyElement, App, BorderStyle, Bounds, DispatchPhase, Display, Element, ElementId,
    Global, GlobalElementId, HitboxBehavior, Hsla, InspectorElementId, LayoutId, MouseDownEvent,
    Pixels, Point, ScrollHandle, Size, Style, Window, WindowId, canvas, fill, outline, point, px,
    relative,
};

/// Space between the lit component and the ring.
const PADDING: f32 = 4.;
/// Space between the ring and the card.
const GAP: f32 = 12.;
/// The least space between the card and the window's edge.
const MARGIN: f32 = 12.;
/// How long the light takes to move to the next component.
const MOVE: Duration = Duration::from_millis(150);

/// Where an anchor was drawn in a frame.
#[derive(Clone)]
pub struct Tagged {
    pub bounds: Bounds<Pixels>,
    /// The part of `bounds` that an ancestor's scroll or clip leaves
    /// visible.
    pub visible: Bounds<Pixels>,
    /// The side the layout leaves open around it ([`Side::Open`]).
    pub facing: Side,
    /// The scroll that holds it, if any: the spotlight scrolls it into view.
    scroll: Option<ScrollHandle>,
}

/// Each window's anchors: the frame being drawn, and the last whole one.
#[derive(Default)]
struct Anchors(HashMap<WindowId, Frames>);

#[derive(Default)]
struct Frames {
    current: HashMap<TourAnchor, Tagged>,
    last: HashMap<TourAnchor, Tagged>,
}

impl Global for Anchors {}

fn frames<'a>(window: &Window, cx: &'a mut App) -> &'a mut Frames {
    cx.default_global::<Anchors>()
        .0
        .entry(window.window_handle().window_id())
        .or_default()
}

/// Starts the window's map for a new frame. The window's root calls it
/// as it renders, before anything prepaints.
pub fn begin_frame(window: &Window, cx: &mut App) {
    let frames = frames(window, cx);
    frames.last = std::mem::take(&mut frames.current);
}

fn record(anchor: TourAnchor, tagged: Tagged, window: &Window, cx: &mut App) {
    frames(window, cx).current.insert(anchor, tagged);
}

/// Where `anchor` is in this frame, or the last one before it prepaints.
fn find(anchor: TourAnchor, window: &Window, cx: &mut App) -> Option<Tagged> {
    let frames = frames(window, cx);
    frames
        .current
        .get(&anchor)
        .or_else(|| frames.last.get(&anchor))
        .cloned()
}

/// An invisible child that records its parent's bounds under `anchor`.
fn probe(anchor: TourAnchor, facing: Side, scroll: Option<ScrollHandle>) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let visible = bounds.intersect(&window.content_mask().bounds);
            let tagged = Tagged {
                bounds,
                visible,
                facing,
                scroll,
            };
            record(anchor, tagged, window, cx);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Tags an element a layout draws with the anchor a tour points at.
/// Adds an invisible child that takes no space and no clicks.
pub trait Anchored: ParentElement + Sized {
    /// `facing` is the side its layout leaves open, where a step's card
    /// goes; `scroll`, the scroll that holds it, so the tour can bring it
    /// into view.
    fn tour_anchor(self, anchor: TourAnchor, facing: Side, scroll: Option<&ScrollHandle>) -> Self {
        self.child(probe(anchor, facing, scroll.cloned()))
    }
}

impl<E: ParentElement> Anchored for E {}

/// Scrolls `tagged` into its scroll's view. Returns whether it moved.
fn reveal(tagged: &Tagged) -> bool {
    let Some(scroll) = &tagged.scroll else {
        return false;
    };
    let viewport = scroll.bounds();
    let delta = point(
        into_view(
            tagged.bounds.left(),
            tagged.bounds.right(),
            viewport.left(),
            viewport.right(),
        ),
        into_view(
            tagged.bounds.top(),
            tagged.bounds.bottom(),
            viewport.top(),
            viewport.bottom(),
        ),
    );
    let max = scroll.max_offset();
    let current = scroll.offset();
    let offset = current + delta;
    let offset = point(
        offset.x.clamp(-max.x, px(0.)),
        offset.y.clamp(-max.y, px(0.)),
    );
    if offset == current {
        return false;
    }
    scroll.set_offset(offset);
    true
}

/// How far to scroll along one axis so `start..end` shows in
/// `view_start..view_end`. Something longer than the view shows its start,
/// so the scroll settles instead of swinging between the two ends.
fn into_view(start: Pixels, end: Pixels, view_start: Pixels, view_end: Pixels) -> Pixels {
    let padding = px(PADDING);
    if start < view_start || end - start + padding * 2. > view_end - view_start {
        view_start - start + padding
    } else if end > view_end {
        view_end - end - padding
    } else {
        px(0.)
    }
}

/// Where the light was and is going, so it slides between components.
#[derive(Default)]
pub struct Motion {
    from: Option<Bounds<Pixels>>,
    to: Option<Bounds<Pixels>>,
    started: Option<Instant>,
}

impl Motion {
    /// The light's bounds now, heading for `target`, and whether it is
    /// still moving.
    fn at(
        &mut self,
        target: Option<Bounds<Pixels>>,
        now: Instant,
    ) -> (Option<Bounds<Pixels>>, bool) {
        if target != self.to {
            let (current, _) = self.shown(now);
            self.from = current.or(target);
            self.to = target;
            self.started = Some(now);
        }
        self.shown(now)
    }

    fn shown(&self, now: Instant) -> (Option<Bounds<Pixels>>, bool) {
        let (Some(from), Some(to), Some(started)) = (self.from, self.to, self.started) else {
            return (self.to, false);
        };
        if from == to {
            return (Some(to), false);
        }
        let t = (now.duration_since(started).as_secs_f32() / MOVE.as_secs_f32()).min(1.);
        if t >= 1. {
            return (Some(to), false);
        }
        // Ease out: quick to leave, gentle to arrive.
        let eased = 1. - (1. - t).powi(3);
        let mix = |a: Pixels, b: Pixels| a + (b - a) * eased;
        let bounds = Bounds {
            origin: point(
                mix(from.origin.x, to.origin.x),
                mix(from.origin.y, to.origin.y),
            ),
            size: Size {
                width: mix(from.size.width, to.size.width),
                height: mix(from.size.height, to.size.height),
            },
        };
        (Some(bounds), true)
    }
}

/// The ring drawn around what is lit.
#[derive(Clone, Copy)]
pub struct Ring {
    pub color: Hsla,
    pub width: Pixels,
    pub radius: Pixels,
}

/// What the spotlight lights, given which anchors this frame drew.
pub type Resolve = Rc<dyn Fn(&dyn Fn(TourAnchor) -> bool, &App) -> Spot>;

/// What a click or a missing anchor runs.
pub type Callback = Rc<dyn Fn(&mut Window, &mut App)>;

/// A layer over the whole content area: the scrim around a lit component,
/// the ring, and a card placed beside the light (or in the middle).
pub struct Spotlight {
    resolve: Resolve,
    card: AnyElement,
    side: Side,
    scrim: Option<Hsla>,
    ring: Option<Ring>,
    /// Takes every click outside the card, so nothing changes by accident.
    block: bool,
    motion: Rc<RefCell<Motion>>,
    /// The lit anchor is missing and the step goes ([`Spot::Skip`]).
    on_skip: Option<Callback>,
    /// A click outside the card.
    on_outside: Option<Callback>,
}

impl Spotlight {
    pub fn new(resolve: Resolve, card: impl IntoElement, motion: Rc<RefCell<Motion>>) -> Self {
        Self {
            resolve,
            card: card.into_any_element(),
            side: Side::Open,
            scrim: None,
            ring: None,
            block: false,
            motion,
            on_skip: None,
            on_outside: None,
        }
    }

    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    pub fn dim(mut self, scrim: Hsla) -> Self {
        self.scrim = Some(scrim);
        self
    }

    pub fn ring(mut self, ring: Ring) -> Self {
        self.ring = Some(ring);
        self
    }

    pub fn block(mut self) -> Self {
        self.block = true;
        self
    }

    pub fn on_skip(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_skip = Some(Rc::new(callback));
        self
    }

    pub fn on_outside(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_outside = Some(Rc::new(callback));
        self
    }
}

/// What prepaint found, for paint.
pub struct Placed {
    hole: Option<Bounds<Pixels>>,
    card: Bounds<Pixels>,
}

impl IntoElement for Spotlight {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Spotlight {
    type RequestLayoutState = LayoutId;
    type PrepaintState = Placed;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, LayoutId) {
        let card = self.card.request_layout(window, cx);
        let mut style = Style {
            display: Display::Flex,
            align_items: Some(AlignItems::FlexStart),
            ..Style::default()
        };
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [card], cx), card)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        card_layout: &mut LayoutId,
        window: &mut Window,
        cx: &mut App,
    ) -> Placed {
        // A component scrolled wholly out of view with no scroll to bring
        // it back (a screen's own control) counts as missing, so its
        // step falls back as it says instead of lighting nothing.
        let drawn: Vec<TourAnchor> = frames(window, cx)
            .current
            .iter()
            .filter(|(_, tagged)| tagged.scroll.is_some() || !tagged.visible.is_empty())
            .map(|(anchor, _)| *anchor)
            .collect();
        let spot = (self.resolve)(&|anchor| drawn.contains(&anchor), cx);
        let lit = match spot {
            Spot::Lit(anchor) => find(anchor, window, cx),
            Spot::Center => None,
            Spot::Skip => {
                if let Some(on_skip) = self.on_skip.clone() {
                    window.defer(cx, move |window, cx| on_skip(window, cx));
                }
                None
            }
        };
        if let Some(tagged) = &lit
            && tagged.visible != tagged.bounds
            && reveal(tagged)
        {
            window.request_animation_frame();
        }
        let target = lit.as_ref().and_then(|tagged| {
            let padded = tagged.visible.dilate(px(PADDING));
            let hole = padded.intersect(&bounds);
            (hole.size.width > px(0.) && hole.size.height > px(0.)).then_some(hole)
        });
        let (hole, moving) = self.motion.borrow_mut().at(target, Instant::now());
        if moving {
            window.request_animation_frame();
        }

        let natural = window.layout_bounds(*card_layout);
        let facing = lit.as_ref().map_or(Side::Right, |tagged| tagged.facing);
        let side = match self.side {
            Side::Open => facing,
            side => side,
        };
        let ring = self.ring.map_or(px(0.), |ring| ring.width);
        let around = hole.map(|hole| hole.dilate(ring));
        let origin = place(bounds, around, natural.size, side);
        if self.block {
            window.insert_hitbox(bounds, HitboxBehavior::BlockMouse);
        }
        let offset = origin - natural.origin;
        window.with_element_offset(point(offset.x.round(), offset.y.round()), |window| {
            self.card.prepaint(window, cx);
        });
        Placed {
            hole,
            card: Bounds {
                origin,
                size: natural.size,
            },
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _card_layout: &mut LayoutId,
        placed: &mut Placed,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(scrim) = self.scrim {
            for part in around_hole(bounds, placed.hole) {
                window.paint_quad(fill(part, scrim));
            }
        }
        if let (Some(ring), Some(hole)) = (self.ring, placed.hole) {
            window.paint_quad(
                outline(hole.dilate(ring.width), ring.color, BorderStyle::Solid)
                    .border_widths(ring.width)
                    .corner_radii(ring.radius + ring.width),
            );
        }
        self.card.paint(window, cx);
        if let Some(on_outside) = self.on_outside.clone() {
            let card = placed.card;
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && bounds.contains(&event.position)
                    && !card.contains(&event.position)
                {
                    on_outside(window, cx);
                }
            });
        }
    }
}

/// The layer around `hole`, as up to four rectangles; all of it without one.
fn around_hole(layer: Bounds<Pixels>, hole: Option<Bounds<Pixels>>) -> Vec<Bounds<Pixels>> {
    let Some(hole) = hole else {
        return vec![layer];
    };
    let rect = |left: Pixels, top: Pixels, right: Pixels, bottom: Pixels| Bounds {
        origin: point(left, top),
        size: Size {
            width: (right - left).max(px(0.)),
            height: (bottom - top).max(px(0.)),
        },
    };
    vec![
        rect(layer.left(), layer.top(), layer.right(), hole.top()),
        rect(layer.left(), hole.bottom(), layer.right(), layer.bottom()),
        rect(layer.left(), hole.top(), hole.left(), hole.bottom()),
        rect(hole.right(), hole.top(), layer.right(), hole.bottom()),
    ]
}

fn opposite(side: Side) -> Side {
    match side {
        Side::Right | Side::Open => Side::Left,
        Side::Left => Side::Right,
        Side::Below => Side::Above,
        Side::Above => Side::Below,
    }
}

/// Where a card of `size` goes: on `side` of `lit`, flipped to the other
/// side when it has no room, else on the other axis; in the middle of
/// `layer` with nothing lit. Always inside `layer`, `MARGIN` from its edge.
fn place(
    layer: Bounds<Pixels>,
    lit: Option<Bounds<Pixels>>,
    size: Size<Pixels>,
    side: Side,
) -> Point<Pixels> {
    let margin = px(MARGIN);
    let clamp = |origin: Point<Pixels>| {
        let max_x = (layer.right() - size.width - margin).max(layer.left() + margin);
        let max_y = (layer.bottom() - size.height - margin).max(layer.top() + margin);
        point(
            origin.x.clamp(layer.left() + margin, max_x),
            origin.y.clamp(layer.top() + margin, max_y),
        )
    };
    let Some(lit) = lit else {
        return clamp(point(
            layer.left() + (layer.size.width - size.width) / 2.,
            layer.top() + (layer.size.height - size.height) / 2.,
        ));
    };
    let gap = px(GAP);
    let at = |side: Side| match side {
        Side::Right | Side::Open => point(lit.right() + gap, lit.top()),
        Side::Left => point(lit.left() - gap - size.width, lit.top()),
        Side::Below => point(lit.left(), lit.bottom() + gap),
        Side::Above => point(lit.left(), lit.top() - gap - size.height),
    };
    let fits = |side: Side| {
        let origin = at(side);
        match side {
            Side::Right | Side::Open | Side::Left => {
                origin.x >= layer.left() + margin && origin.x + size.width <= layer.right() - margin
            }
            Side::Below | Side::Above => {
                origin.y >= layer.top() + margin
                    && origin.y + size.height <= layer.bottom() - margin
            }
        }
    };
    let across = match side {
        Side::Right | Side::Open | Side::Left => [Side::Below, Side::Above],
        Side::Below | Side::Above => [Side::Right, Side::Left],
    };
    let order = [side, opposite(side), across[0], across[1]];
    let chosen = order.into_iter().find(|side| fits(*side)).unwrap_or(side);
    clamp(at(chosen))
}

#[cfg(test)]
mod tests {
    use bardo_app::bardo_domain::LayoutId;
    use bardo_app::{Destination, Tour, WhenMissing};

    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: Size {
                width: px(w),
                height: px(h),
            },
        }
    }

    const CARD: Size<Pixels> = Size {
        width: px(360.),
        height: px(200.),
    };

    #[test]
    fn the_card_sits_on_the_preferred_side() {
        let layer = rect(0., 0., 1200., 800.);
        let sidebar_group = rect(0., 100., 216., 120.);
        assert_eq!(
            place(layer, Some(sidebar_group), CARD, Side::Right),
            point(px(228.), px(100.))
        );
        let top_tab = rect(300., 0., 120., 44.);
        assert_eq!(
            place(layer, Some(top_tab), CARD, Side::Below),
            point(px(300.), px(56.))
        );
    }

    #[test]
    fn the_card_flips_when_its_side_has_no_room() {
        let layer = rect(0., 0., 1200., 800.);
        let right_edge = rect(1000., 300., 190., 40.);
        let origin = place(layer, Some(right_edge), CARD, Side::Right);
        assert_eq!(origin, point(px(628.), px(300.)), "to the left");
        let bottom = rect(100., 700., 200., 60.);
        let origin = place(layer, Some(bottom), CARD, Side::Below);
        assert_eq!(origin, point(px(100.), px(488.)), "above");
    }

    #[test]
    fn the_card_stays_in_the_window() {
        let layer = rect(0., 0., 1200., 800.);
        let low = rect(0., 700., 216., 90.);
        let origin = place(layer, Some(low), CARD, Side::Right);
        assert_eq!(origin, point(px(228.), px(588.)), "pulled up");
        assert_eq!(
            place(layer, None, CARD, Side::Right),
            point(px(420.), px(300.)),
            "centered"
        );
    }

    #[test]
    fn the_scrim_leaves_the_hole_lit() {
        let layer = rect(0., 0., 100., 100.);
        let parts = around_hole(layer, Some(rect(20., 30., 40., 10.)));
        let area: f32 = parts
            .iter()
            .map(|part| f32::from(part.size.width) * f32::from(part.size.height))
            .sum();
        assert_eq!(area, 100. * 100. - 40. * 10.);
        for part in &parts {
            let overlap = part.intersect(&rect(20., 30., 40., 10.));
            assert!(
                overlap.size.width <= px(0.) || overlap.size.height <= px(0.),
                "{part:?} covers the hole"
            );
        }
        assert_eq!(around_hole(layer, None), vec![layer]);
    }

    #[test]
    fn scrolling_into_view_settles() {
        let view = (px(0.), px(200.));
        assert_eq!(into_view(px(50.), px(100.), view.0, view.1), px(0.));
        assert_eq!(into_view(px(-30.), px(20.), view.0, view.1), px(34.));
        assert_eq!(into_view(px(180.), px(240.), view.0, view.1), px(-44.));
        // Longer than the view: its start shows, wherever it is.
        let long = into_view(px(-30.), px(300.), view.0, view.1);
        assert_eq!(long, px(34.));
        assert_eq!(
            into_view(px(-30.) + long, px(300.) + long, view.0, view.1),
            px(0.)
        );
    }

    #[test]
    fn the_light_slides_then_settles() {
        let mut motion = Motion::default();
        let start = Instant::now();
        let first = rect(0., 0., 100., 50.);
        assert_eq!(motion.at(Some(first), start), (Some(first), false));
        let second = rect(0., 200., 100., 50.);
        assert_eq!(motion.at(Some(second), start), (Some(first), true));
        let (halfway, moving) = motion.at(Some(second), start + MOVE / 2);
        assert!(moving);
        let y = halfway.unwrap().origin.y;
        assert!(y > px(0.) && y < px(200.), "{y:?}");
        assert_eq!(
            motion.at(Some(second), start + MOVE * 2),
            (Some(second), false)
        );
    }

    /// Each layout's source, by the layout it arranges.
    fn layout_source(layout: LayoutId) -> &'static str {
        match layout {
            LayoutId::Workspace => include_str!("layout/workspace.rs"),
            LayoutId::Studio => include_str!("layout/studio.rs"),
        }
    }

    /// The screens that tag their own controls, in every layout alike.
    const SCREENS: [&str; 17] = [
        include_str!("research.rs"),
        include_str!("themes.rs"),
        include_str!("performance.rs"),
        include_str!("projects.rs"),
        include_str!("projects/scenes.rs"),
        include_str!("projects/render.rs"),
        include_str!("editor.rs"),
        include_str!("editor/timeline.rs"),
        include_str!("editor/suggestions.rs"),
        include_str!("personas.rs"),
        include_str!("templates.rs"),
        include_str!("channels.rs"),
        include_str!("network_accounts.rs"),
        include_str!("settings.rs"),
        include_str!("missed.rs"),
        include_str!("projects/export.rs"),
        include_str!("projects/upload.rs"),
    ];

    /// How a layout tags `anchor`: a pillar's group and each place come
    /// from the navigation it is handed, which holds every one.
    fn tag(anchor: TourAnchor) -> String {
        match anchor {
            TourAnchor::NavGroup(_) => "TourAnchor::NavGroup(group.pillar)".to_owned(),
            TourAnchor::NavPlace(place) => {
                let pinned = Destination::PINNED.contains(&place);
                if pinned {
                    "TourAnchor::NavPlace(".to_owned()
                } else {
                    "TourAnchor::NavPlace(item.place)".to_owned()
                }
            }
            part => format!("TourAnchor::{part:?}"),
        }
    }

    #[test]
    fn every_anchor_a_tour_uses_is_tagged_in_every_layout() {
        for tour in Tour::ALL {
            for step in tour.steps {
                let fallback = match step.when_missing {
                    WhenMissing::LightPart(part) => Some(part),
                    WhenMissing::Skip | WhenMissing::Center => None,
                };
                for anchor in step.anchor.into_iter().chain(fallback) {
                    if let TourAnchor::Control(control) = anchor {
                        let tag = format!("TourAnchor::Control(Control::{control:?})");
                        assert!(
                            SCREENS.iter().any(|source| source.contains(&tag)),
                            "no screen tags {control:?} ({:?} step {})",
                            tour.id,
                            step.key
                        );
                        continue;
                    }
                    for layout in LayoutId::ALL {
                        assert!(
                            layout_source(layout).contains(&tag(anchor)),
                            "{layout:?} does not tag {anchor:?} ({:?} step {})",
                            tour.id,
                            step.key
                        );
                    }
                }
            }
        }
    }

    /// The arms of the `match` a layout draws its pinned places with, as
    /// (the place an arm names, or `None` for `_`, and the arm's code).
    fn pinned_arms(source: &str) -> Vec<(Option<&str>, String)> {
        let start = ["nav.pinned.iter().map(", "for item in &nav.pinned {"]
            .iter()
            .find_map(|marker| source.find(marker))
            .expect("the layout draws the pinned places");
        let mut arms: Vec<(Option<&str>, String)> = Vec::new();
        // Arms start at the first arm's indent; a `match` inside one is its
        // code.
        let mut indent = None;
        for line in source[start..].lines().skip(1) {
            if line.starts_with("    }") {
                break;
            }
            let code = line.trim_start();
            let depth = line.len() - code.len();
            let arm = code.starts_with("Destination::") || code.starts_with("_ =>");
            if arm && indent.is_none() {
                indent = Some(depth);
            }
            if arm && indent == Some(depth) {
                let name = code
                    .strip_prefix("Destination::")
                    .and_then(|rest| rest.split_once(" =>"))
                    .map(|(name, _)| name);
                arms.push((name, String::new()));
            }
            if let Some((_, body)) = arms.last_mut() {
                body.push_str(line);
                body.push('\n');
            }
        }
        arms
    }

    #[test]
    fn every_pinned_place_is_tagged_in_every_layout() {
        // Each layout draws the pinned places its own way, one arm each.
        for layout in LayoutId::ALL {
            let arms = pinned_arms(layout_source(layout));
            for place in Destination::PINNED {
                let name = format!("{place:?}");
                let arm = arms
                    .iter()
                    .find(|(named, _)| *named == Some(name.as_str()))
                    .or_else(|| arms.iter().find(|(named, _)| named.is_none()))
                    .unwrap_or_else(|| panic!("{layout:?} does not draw {name}"));
                assert!(
                    arm.1.contains("TourAnchor::NavPlace("),
                    "{layout:?} does not tag {name}"
                );
            }
        }
    }
}
