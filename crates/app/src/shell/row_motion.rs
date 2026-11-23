use std::collections::{HashMap, HashSet};
use std::time::Duration;

use browser_core::tabs::{Section, TabId};
use gpui_ce_motion::{MotionValue, Transition};

use crate::motion;

const SLIDE: Duration = Duration::from_millis(260);
const APPEAR: Duration = Duration::from_millis(200);

/// A tab being dragged over the sidebar: which list and slot the gap is
/// open at, and where the pointer is in that list.
#[derive(Debug, Clone, Copy)]
pub(super) struct TabDrag {
    pub(super) id: TabId,
    pub(super) over: Option<(Section, usize)>,
    pub(super) pointer_y: f32,
}

#[derive(Debug)]
struct RowState {
    scope: u64,
    slot: f32,
    offset: MotionValue,
    appear: MotionValue,
    frame: u64,
}

/// Slides sidebar rows to their new place when the order changes, the way
/// FLIP layout animation works: each row remembers its slot, and when the
/// slot moves the row starts from where it was drawn and springs to 0.
#[derive(Debug, Default)]
pub(super) struct RowMotion {
    rows: HashMap<TabId, RowState>,
    frame: u64,
    scopes_before: HashSet<u64>,
    scopes_now: HashSet<u64>,
    glide: Option<(TabId, f32)>,
    animating: bool,
}

/// Where to draw a row this frame, relative to its layout position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RowFrame {
    pub(super) offset: f32,
    pub(super) opacity: f32,
}

impl RowMotion {
    pub(super) fn begin(&mut self) {
        self.frame += 1;
        self.animating = false;
    }

    pub(super) fn end(&mut self) {
        let frame = self.frame;
        self.rows.retain(|_, row| row.frame == frame);
        self.scopes_before = std::mem::take(&mut self.scopes_now);
        if self
            .glide
            .is_some_and(|(id, _)| !self.rows.contains_key(&id))
        {
            self.glide = None;
        }
    }

    pub(super) fn animating(&self) -> bool {
        self.animating
    }

    /// The next time `id` is placed it slides from `from_slot` instead of
    /// appearing in place: a dropped row gliding in from the pointer.
    pub(super) fn glide_from(&mut self, id: TabId, from_slot: f32) {
        self.glide = Some((id, from_slot));
    }

    /// Places row `id` at `slot` (its top, in pixels, within list `scope`).
    pub(super) fn place(&mut self, id: TabId, scope: u64, slot: f32) -> RowFrame {
        let now = motion::now();
        let spring = motion::spring(SLIDE);
        let glide = self
            .glide
            .filter(|(glide_id, _)| *glide_id == id)
            .map(|(_, from)| from);
        let known_scope = self.scopes_before.contains(&scope);
        self.scopes_now.insert(scope);
        let row = self.rows.entry(id).or_insert_with(|| RowState {
            scope,
            slot,
            offset: MotionValue::new(0.0),
            appear: MotionValue::new(1.0),
            frame: 0,
        });
        let fresh = row.frame == 0 || row.scope != scope;
        if let Some(from) = glide {
            self.glide = None;
            row.offset.jump(f64::from(from - slot), now);
            row.offset.animate(0.0, spring, now);
            row.appear.jump(1.0, now);
        } else if fresh {
            row.offset.jump(0.0, now);
            if known_scope {
                row.appear.jump(0.0, now);
                row.appear
                    .animate(1.0, Transition::Tween(APPEAR, motion::Easing::EaseOut), now);
            } else {
                row.appear.jump(1.0, now);
            }
        } else if (row.slot - slot).abs() > 0.5 {
            let drawn = row.slot + row.offset.get() as f32;
            row.offset.jump(f64::from(drawn - slot), now);
            row.offset.animate(0.0, spring, now);
        }
        row.scope = scope;
        row.slot = slot;
        row.frame = self.frame;
        let reduced = motion::reduced();
        let offset = row.offset.sample(now, reduced);
        let appear = row.appear.sample(now, reduced);
        self.animating |= offset.is_animating() || appear.is_animating();
        RowFrame {
            offset: offset.value() as f32,
            opacity: (appear.value() as f32).clamp(0.0, 1.0),
        }
    }
}

/// The list slot a pointer `y` pixels below a list's top falls in, for a list
/// of `count` rows `row` pixels tall (`count` is the slot past the end).
pub(super) fn slot_at(y: f32, row: f32, count: usize) -> usize {
    if y <= 0.0 || row <= 0.0 {
        return 0;
    }
    ((y / row) as usize).min(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(motion: &mut RowMotion, rows: &[(TabId, f32)]) -> Vec<RowFrame> {
        motion.begin();
        let frames = rows
            .iter()
            .map(|(id, slot)| motion.place(*id, 1, *slot))
            .collect();
        motion.end();
        frames
    }

    #[test]
    fn rows_start_where_they_were_drawn_when_the_order_changes() {
        let (a, b) = (TabId::next(), TabId::next());
        let mut motion = RowMotion::default();
        frame(&mut motion, &[(a, 0.0), (b, 32.0)]);
        let swapped = frame(&mut motion, &[(b, 0.0), (a, 32.0)]);
        assert!((swapped[0].offset - 32.0).abs() < 1.0);
        assert!((swapped[1].offset + 32.0).abs() < 1.0);
        assert!(motion.animating() || motion::reduced());
    }

    #[test]
    fn first_rows_appear_in_place_and_later_rows_fade_in() {
        let (a, b) = (TabId::next(), TabId::next());
        let mut motion = RowMotion::default();
        let first = frame(&mut motion, &[(a, 0.0)]);
        assert_eq!(
            first[0],
            RowFrame {
                offset: 0.0,
                opacity: 1.0
            }
        );
        let added = frame(&mut motion, &[(a, 0.0), (b, 32.0)]);
        assert!(added[1].opacity < 0.5 || motion::reduced());
    }

    #[test]
    fn a_dropped_row_glides_from_the_pointer() {
        let (a, b) = (TabId::next(), TabId::next());
        let mut motion = RowMotion::default();
        frame(&mut motion, &[(a, 0.0), (b, 32.0)]);
        motion.glide_from(b, 100.0);
        let dropped = frame(&mut motion, &[(b, 0.0), (a, 32.0)]);
        assert!((dropped[0].offset - 100.0).abs() < 1.0);
    }

    #[test]
    fn pointer_maps_to_slots() {
        assert_eq!(slot_at(-5.0, 32.0, 3), 0);
        assert_eq!(slot_at(40.0, 32.0, 3), 1);
        assert_eq!(slot_at(500.0, 32.0, 3), 3);
    }
}
