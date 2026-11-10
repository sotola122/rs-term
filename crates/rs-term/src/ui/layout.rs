//! Pane tree, a port of herdr's `TileLayout` (`src/layout.rs`).
//!
//! - A split replaces the target leaf with `Split { first: target, second:
//!   new }` at ratio 0.5 (`split_node`, `split_pane`). The caller focuses the
//!   new pane, as herdr's split keys do (`focus: true`).
//! - Closing the focused pane returns focus to the previously focused pane
//!   if it still exists, else the next pane in preorder, else the previous
//!   one (`close_focused`).
//! - Directional focus and swap pick the nearest pane by edge distance, then
//!   overlap, then center distance, then preorder (`find_in_direction`).
//!   They use the unzoomed rectangles, also while zoomed.
//! - Swap exchanges two ids and keeps the shape and ratios; focus stays on
//!   the moved pane (`swap_panes`).
//! - Resize moves the nearest divider on the requested edge, or on the
//!   opposite edge when there is none, by 0.05 (`resize_focused`; the API
//!   default amount is 0.05).
//! - Every ratio, from a key, a drag, or a file, is clamped to 0.1..=0.9
//!   (`valid_split_ratio`, `set_ratio_at`).

use std::cmp::Reverse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Side by side. herdr calls this a vertical split.
    SideBySide,
    /// Stacked. herdr calls this a horizontal split.
    Stacked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Leaf(u64),
    Split {
        axis: Axis,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneRect {
    pub id: u64,
    pub rect: Rect,
}

pub const RATIO_MIN: f32 = 0.1;
pub const RATIO_MAX: f32 = 0.9;
pub const RESIZE_STEP: f32 = 0.05;

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    root: Node,
    focus: u64,
    prev_focus: Option<u64>,
    next_id: u64,
    zoomed: bool,
}

impl Layout {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn single(id: u64) -> Self {
        Self {
            root: Node::Leaf(id),
            focus: id,
            prev_focus: None,
            next_id: id.saturating_add(1),
            zoomed: false,
        }
    }

    pub fn focus(&self) -> u64 {
        self.focus
    }

    pub fn zoomed(&self) -> bool {
        self.zoomed
    }

    pub fn panes(&self) -> Vec<u64> {
        let mut ids = Vec::new();
        collect(&self.root, &mut ids);
        ids
    }

    pub fn rects(&self, area: Rect) -> Vec<PaneRect> {
        let mut out = Vec::new();
        place(&self.root, area, &mut out);
        out
    }

    pub fn split(&mut self, axis: Axis) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        replace_leaf(&mut self.root, self.focus, &|old| Node::Split {
            axis,
            ratio: 0.5,
            first: Box::new(Node::Leaf(old)),
            second: Box::new(Node::Leaf(id)),
        });
        self.set_focus(id);
        self.zoomed = false;
        id
    }

    pub fn focus_in(&mut self, direction: Direction, area: Rect) -> bool {
        let panes = self.rects(area);
        let Some(focused) = panes.iter().find(|pane| pane.id == self.focus).copied() else {
            return false;
        };
        let Some(next) = nearest(focused, direction, &panes) else {
            return false;
        };
        self.set_focus(next);
        true
    }

    pub fn swap(&mut self, direction: Direction, area: Rect) -> bool {
        let panes = self.rects(area);
        let Some(focused) = panes.iter().find(|pane| pane.id == self.focus).copied() else {
            return false;
        };
        let Some(other) = nearest(focused, direction, &panes) else {
            return false;
        };
        swap_ids(&mut self.root, self.focus, other);
        true
    }

    pub fn cycle(&mut self, forward: bool) {
        let ids = self.panes();
        if ids.len() < 2 {
            return;
        }
        let pos = ids.iter().position(|id| *id == self.focus).unwrap_or(0);
        let next = if forward {
            ids[(pos + 1) % ids.len()]
        } else {
            ids[(pos + ids.len() - 1) % ids.len()]
        };
        self.set_focus(next);
    }

    /// Close the focused pane. The last pane stays, and the caller disconnects it.
    pub fn close_focused(&mut self) -> bool {
        let ids = self.panes();
        if ids.len() <= 1 {
            return false;
        }
        let target = self.focus;
        let pos = ids.iter().position(|id| *id == target).unwrap_or(0);
        let fallback = if pos + 1 < ids.len() {
            ids[pos + 1]
        } else {
            ids[pos - 1]
        };
        let new_focus = match self.prev_focus {
            Some(prev) if prev != target && ids.contains(&prev) => prev,
            _ => fallback,
        };
        if let Some(root) = remove(&self.root, target) {
            self.root = root;
            self.focus = new_focus;
            self.prev_focus = None;
            self.zoomed = false;
            true
        } else {
            false
        }
    }

    pub fn focus_id(&mut self, id: u64) {
        if self.panes().contains(&id) {
            self.set_focus(id);
        }
    }

    /// herdr: zoom of a single pane is a no-op (`apply_pane_zoom`).
    pub fn toggle_zoom(&mut self) -> bool {
        if self.panes().len() <= 1 {
            self.zoomed = false;
            return false;
        }
        self.zoomed = !self.zoomed;
        true
    }

    pub fn resize_focused(&mut self, direction: Direction, area: Rect) {
        let axis = match direction {
            Direction::Left | Direction::Right => Axis::SideBySide,
            Direction::Up | Direction::Down => Axis::Stacked,
        };
        let panes = self.rects(area);
        let Some(focused) = panes.iter().find(|pane| pane.id == self.focus) else {
            return;
        };
        let mut edges = Vec::new();
        walk_splits(&self.root, Vec::new(), area, &mut edges);
        let split = edges
            .iter()
            .filter(|split| split.axis == axis && edge_matches(split, focused.rect, direction))
            .min_by_key(|split| edge_distance(split, focused.rect, direction))
            .or_else(|| {
                let opposite = match direction {
                    Direction::Left => Direction::Right,
                    Direction::Right => Direction::Left,
                    Direction::Up => Direction::Down,
                    Direction::Down => Direction::Up,
                };
                edges
                    .iter()
                    .filter(|split| {
                        split.axis == axis && edge_matches(split, focused.rect, opposite)
                    })
                    .min_by_key(|split| edge_distance(split, focused.rect, opposite))
            });
        let Some(split) = split else {
            return;
        };
        // Herdr moves the divider with the key. Right and Down raise the
        // first-child ratio; Left and Up lower it. The outer edge therefore
        // shrinks the focused pane, and the inner edge grows it.
        let delta = match direction {
            Direction::Right | Direction::Down => RESIZE_STEP,
            Direction::Left | Direction::Up => -RESIZE_STEP,
        };
        let current = ratio_at(&self.root, &split.path).unwrap_or(0.5);
        let _ = set_ratio(&mut self.root, &split.path, clamp_live(current + delta));
    }

    /// Apply a drag ratio. Values outside 0.1..=0.9 are clamped.
    pub fn set_live_ratio(&mut self, path: &[bool], ratio: f32) {
        let _ = set_ratio(&mut self.root, path, clamp_live(ratio));
    }

    pub fn view(&self) -> ViewNode {
        view_node(&self.root, Vec::new())
    }

    // Used by the layout.toml restore (phase 3).
    #[allow(dead_code)]
    pub fn ratio_millis(&self, path: &[bool]) -> Option<u16> {
        ratio_at(&self.root, path).map(|ratio| (ratio * 1000.0).round() as u16)
    }

    fn set_focus(&mut self, id: u64) {
        if id != self.focus {
            self.prev_focus = Some(self.focus);
            self.focus = id;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewNode {
    Leaf(u64),
    Split {
        axis: Axis,
        ratio: f32,
        path: Vec<bool>,
        first: Box<ViewNode>,
        second: Box<ViewNode>,
    },
}

impl Layout {
    /// Rebuild from a saved tree. Duplicate leaf ids are refused, because
    /// they would make two panes share one session (review N29).
    // Used by the layout.toml restore (phase 3).
    #[allow(dead_code)]
    pub fn from_saved(node: ViewNode, focus: Option<u64>) -> Result<Self, String> {
        let mut ids = Vec::new();
        fn leaves(node: &ViewNode, out: &mut Vec<u64>) {
            match node {
                ViewNode::Leaf(id) => out.push(*id),
                ViewNode::Split { first, second, .. } => {
                    leaves(first, out);
                    leaves(second, out);
                }
            }
        }
        leaves(&node, &mut ids);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != ids.len() {
            return Err("ペイン ID が重複している".into());
        }
        if ids.contains(&0) {
            return Err("ペイン ID 0 は使えない".into());
        }
        let mut layout = Self::from_saved_unchecked(node);
        if let Some(focus) = focus.filter(|focus| ids.contains(focus)) {
            layout.focus = focus;
        }
        Ok(layout)
    }

    // Used by the layout.toml restore (phase 3).
    #[allow(dead_code)]
    fn from_saved_unchecked(node: ViewNode) -> Self {
        let mut max_id = 0u64;
        fn walk(node: &ViewNode, max_id: &mut u64) {
            match node {
                ViewNode::Leaf(id) => *max_id = (*max_id).max(*id),
                ViewNode::Split { first, second, .. } => {
                    walk(first, max_id);
                    walk(second, max_id);
                }
            }
        }
        walk(&node, &mut max_id);
        let focus = match &node {
            ViewNode::Leaf(id) => *id,
            ViewNode::Split { first, .. } => first_leaf(first),
        };
        Self {
            root: saved_node(node),
            focus,
            prev_focus: None,
            next_id: max_id.saturating_add(1).max(2),
            zoomed: false,
        }
    }
}

// Used by the layout.toml restore (phase 3).
#[allow(dead_code)]
fn first_leaf(node: &ViewNode) -> u64 {
    match node {
        ViewNode::Leaf(id) => *id,
        ViewNode::Split { first, .. } => first_leaf(first),
    }
}

// Used by the layout.toml restore (phase 3).
#[allow(dead_code)]
fn saved_node(node: ViewNode) -> Node {
    match node {
        ViewNode::Leaf(id) => Node::Leaf(id),
        ViewNode::Split {
            axis,
            ratio,
            first,
            second,
            ..
        } => Node::Split {
            axis,
            ratio: clamp_live(ratio),
            first: Box::new(saved_node(*first)),
            second: Box::new(saved_node(*second)),
        },
    }
}

fn view_node(node: &Node, path: Vec<bool>) -> ViewNode {
    match node {
        Node::Leaf(id) => ViewNode::Leaf(*id),
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let mut left = path.clone();
            left.push(false);
            let mut right = path.clone();
            right.push(true);
            ViewNode::Split {
                axis: *axis,
                ratio: *ratio,
                path,
                first: Box::new(view_node(first, left)),
                second: Box::new(view_node(second, right)),
            }
        }
    }
}

fn clamp_live(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(RATIO_MIN, RATIO_MAX)
    } else {
        0.5
    }
}

fn collect(node: &Node, ids: &mut Vec<u64>) {
    match node {
        Node::Leaf(id) => ids.push(*id),
        Node::Split { first, second, .. } => {
            collect(first, ids);
            collect(second, ids);
        }
    }
}

fn place(node: &Node, area: Rect, out: &mut Vec<PaneRect>) {
    match node {
        Node::Leaf(id) => out.push(PaneRect {
            id: *id,
            rect: area,
        }),
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (a, b) = split_rect(area, *axis, *ratio);
            place(first, a, out);
            place(second, b, out);
        }
    }
}

fn split_rect(area: Rect, axis: Axis, ratio: f32) -> (Rect, Rect) {
    match axis {
        Axis::SideBySide => {
            let first = ((area.width as f32) * ratio).round() as u32;
            let first = first.clamp(1, area.width.saturating_sub(1).max(1));
            let second = area.width.saturating_sub(first);
            (
                Rect {
                    width: first,
                    ..area
                },
                Rect {
                    x: area.x + first,
                    width: second,
                    ..area
                },
            )
        }
        Axis::Stacked => {
            let first = ((area.height as f32) * ratio).round() as u32;
            let first = first.clamp(1, area.height.saturating_sub(1).max(1));
            let second = area.height.saturating_sub(first);
            (
                Rect {
                    height: first,
                    ..area
                },
                Rect {
                    y: area.y + first,
                    height: second,
                    ..area
                },
            )
        }
    }
}

fn replace_leaf(node: &mut Node, target: u64, build: &dyn Fn(u64) -> Node) -> bool {
    match node {
        Node::Leaf(id) if *id == target => {
            *node = build(*id);
            true
        }
        Node::Leaf(_) => false,
        Node::Split { first, second, .. } => {
            replace_leaf(first, target, build) || replace_leaf(second, target, build)
        }
    }
}

fn remove(node: &Node, target: u64) -> Option<Node> {
    match node {
        Node::Leaf(id) if *id == target => None,
        Node::Leaf(id) => Some(Node::Leaf(*id)),
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => match (remove(first, target), remove(second, target)) {
            (None, Some(kept)) | (Some(kept), None) => Some(kept),
            (Some(first), Some(second)) => Some(Node::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(first),
                second: Box::new(second),
            }),
            (None, None) => None,
        },
    }
}

fn swap_ids(node: &mut Node, a: u64, b: u64) {
    match node {
        Node::Leaf(id) if *id == a => *id = b,
        Node::Leaf(id) if *id == b => *id = a,
        Node::Leaf(_) => {}
        Node::Split { first, second, .. } => {
            swap_ids(first, a, b);
            swap_ids(second, a, b);
        }
    }
}

fn nearest(focused: PaneRect, direction: Direction, panes: &[PaneRect]) -> Option<u64> {
    let fr = focused.rect;
    panes
        .iter()
        .enumerate()
        .filter(|(_, pane)| pane.id != focused.id)
        .filter(|(_, pane)| {
            let r = pane.rect;
            match direction {
                Direction::Left => r.x + r.width <= fr.x && overlap(r.y, r.height, fr.y, fr.height),
                Direction::Right => {
                    r.x >= fr.x + fr.width && overlap(r.y, r.height, fr.y, fr.height)
                }
                Direction::Up => r.y + r.height <= fr.y && overlap(r.x, r.width, fr.x, fr.width),
                Direction::Down => r.y >= fr.y + fr.height && overlap(r.x, r.width, fr.x, fr.width),
            }
        })
        .min_by_key(|(index, pane)| {
            let r = pane.rect;
            let edge = match direction {
                Direction::Left => fr.x.saturating_sub(r.x + r.width),
                Direction::Right => r.x.saturating_sub(fr.x + fr.width),
                Direction::Up => fr.y.saturating_sub(r.y + r.height),
                Direction::Down => r.y.saturating_sub(fr.y + fr.height),
            };
            let overlap = match direction {
                Direction::Left | Direction::Right => {
                    overlap_amount(r.y, r.height, fr.y, fr.height)
                }
                Direction::Up | Direction::Down => overlap_amount(r.x, r.width, fr.x, fr.width),
            };
            let center = match direction {
                Direction::Left | Direction::Right => {
                    center_distance(r.y, r.height, fr.y, fr.height)
                }
                Direction::Up | Direction::Down => center_distance(r.x, r.width, fr.x, fr.width),
            };
            (edge, Reverse(overlap), center, *index)
        })
        .map(|(_, pane)| pane.id)
}

fn overlap(a: u32, a_len: u32, b: u32, b_len: u32) -> bool {
    a < b + b_len && a + a_len > b
}

fn overlap_amount(a: u32, a_len: u32, b: u32, b_len: u32) -> u32 {
    let a_end = a + a_len;
    let b_end = b + b_len;
    a_end.min(b_end).saturating_sub(a.max(b))
}

fn center_distance(a: u32, a_len: u32, b: u32, b_len: u32) -> u32 {
    let a_center = a.saturating_mul(2).saturating_add(a_len);
    let b_center = b.saturating_mul(2).saturating_add(b_len);
    a_center.abs_diff(b_center)
}

struct SplitEdge {
    path: Vec<bool>,
    axis: Axis,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    pos: u32,
}

fn walk_splits(node: &Node, path: Vec<bool>, area: Rect, out: &mut Vec<SplitEdge>) {
    let Node::Split {
        axis,
        ratio,
        first,
        second,
    } = node
    else {
        return;
    };
    let (a, b) = split_rect(area, *axis, *ratio);
    let pos = match axis {
        Axis::SideBySide => a.x + a.width,
        Axis::Stacked => a.y + a.height,
    };
    out.push(SplitEdge {
        path,
        axis: *axis,
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
        pos,
    });
    let mut left = out.last().unwrap().path.clone();
    left.push(false);
    let mut right = out.last().unwrap().path.clone();
    right.push(true);
    walk_splits(first, left, a, out);
    walk_splits(second, right, b, out);
}

fn edge_matches(split: &SplitEdge, focused: Rect, direction: Direction) -> bool {
    let overlaps = match direction {
        Direction::Left | Direction::Right => {
            overlap(split.y, split.height, focused.y, focused.height)
        }
        Direction::Up | Direction::Down => overlap(split.x, split.width, focused.x, focused.width),
    };
    overlaps && edge_distance(split, focused, direction) <= 1
}

fn edge_distance(split: &SplitEdge, focused: Rect, direction: Direction) -> u32 {
    match direction {
        Direction::Left => split.pos.abs_diff(focused.x),
        Direction::Right => split.pos.abs_diff(focused.x + focused.width),
        Direction::Up => split.pos.abs_diff(focused.y),
        Direction::Down => split.pos.abs_diff(focused.y + focused.height),
    }
}

fn ratio_at(node: &Node, path: &[bool]) -> Option<f32> {
    match node {
        Node::Split {
            ratio,
            first,
            second,
            ..
        } => {
            if path.is_empty() {
                Some(*ratio)
            } else if path[0] {
                ratio_at(second, &path[1..])
            } else {
                ratio_at(first, &path[1..])
            }
        }
        Node::Leaf(_) => None,
    }
}

fn set_ratio(node: &mut Node, path: &[bool], ratio: f32) -> bool {
    match node {
        Node::Split {
            ratio: slot,
            first,
            second,
            ..
        } => {
            if path.is_empty() {
                *slot = ratio;
                true
            } else if path[0] {
                set_ratio(second, &path[1..], ratio)
            } else {
                set_ratio(first, &path[1..], ratio)
            }
        }
        Node::Leaf(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        }
    }

    #[test]
    fn side_by_side_then_stack_makes_a_grid() {
        let mut layout = Layout::single(1);
        layout.split(Axis::SideBySide);
        layout.focus_in(Direction::Left, area());
        layout.split(Axis::Stacked);
        assert_eq!(layout.panes().len(), 3);
        let rects = layout.rects(area());
        assert!(rects.iter().any(|pane| pane.rect.width < 1000));
        assert!(rects.iter().any(|pane| pane.rect.height < 800));
    }

    #[test]
    fn focus_moves_across_a_side_by_side_split() {
        let mut layout = Layout::single(1);
        let created = layout.split(Axis::SideBySide);
        assert_eq!(layout.focus(), created);
        assert!(layout.focus_in(Direction::Left, area()));
        assert_eq!(layout.focus(), 1);
        assert!(layout.focus_in(Direction::Right, area()));
        assert_eq!(layout.focus(), created);
    }

    #[test]
    fn swap_exchanges_ids_and_keeps_the_rects() {
        let mut layout = Layout::single(1);
        layout.split(Axis::SideBySide);
        let before = layout.rects(area());
        assert!(layout.swap(Direction::Left, area()));
        let after = layout.rects(area());
        assert_eq!(before[0].rect, after[0].rect);
        assert_eq!(before[0].id, after[1].id);
        assert_eq!(before[1].id, after[0].id);
    }

    #[test]
    fn close_keeps_the_last_pane_and_returns_to_the_previous_focus() {
        let mut layout = Layout::single(1);
        layout.split(Axis::SideBySide);
        assert!(layout.close_focused());
        assert_eq!(layout.panes(), vec![1]);
        assert_eq!(layout.focus(), 1);
        assert!(!layout.close_focused());
        assert_eq!(layout.panes(), vec![1]);
    }

    #[test]
    fn zoom_hides_nothing_in_the_tree_and_is_a_noop_for_one_pane() {
        let mut layout = Layout::single(1);
        layout.toggle_zoom();
        assert!(!layout.zoomed());
        layout.split(Axis::Stacked);
        layout.toggle_zoom();
        assert!(layout.zoomed());
        assert_eq!(layout.panes().len(), 2);
        assert!(layout.focus_in(Direction::Up, area()) || layout.focus_in(Direction::Down, area()));
    }

    #[test]
    fn resize_moves_the_divider_and_clamps() {
        let mut layout = Layout::single(1);
        let right = layout.split(Axis::SideBySide);
        layout.resize_focused(Direction::Left, area());
        assert_eq!(layout.ratio_millis(&[]), Some(450));
        layout.focus_id(1);
        layout.resize_focused(Direction::Left, area());
        assert_eq!(layout.ratio_millis(&[]), Some(400));
        layout.focus_id(right);
        layout.resize_focused(Direction::Right, area());
        assert_eq!(layout.ratio_millis(&[]), Some(450));
        for _ in 0..30 {
            layout.resize_focused(Direction::Right, area());
        }
        assert_eq!(layout.ratio_millis(&[]), Some(900));
    }

    fn grid() -> Layout {
        // herdr's 2x2: split right, split the left down, split the right down.
        let mut layout = Layout::single(1);
        let right = layout.split(Axis::SideBySide);
        assert_eq!(right, 2);
        layout.focus_id(1);
        assert_eq!(layout.split(Axis::Stacked), 3);
        layout.focus_id(2);
        assert_eq!(layout.split(Axis::Stacked), 4);
        layout
    }

    #[test]
    fn split_puts_the_new_pane_second_and_focuses_it() {
        let mut layout = Layout::single(1);
        let new = layout.split(Axis::SideBySide);
        assert_eq!(layout.focus(), new);
        let rects = layout.rects(area());
        // Right of the target, half the width (herdr split_vertical).
        assert_eq!(rects[0].id, 1);
        assert_eq!(rects[0].rect.width, 500);
        assert_eq!(rects[1].id, new);
        assert_eq!(rects[1].rect.x, 500);
        let below = layout.split(Axis::Stacked);
        let rects = layout.rects(area());
        let lower = rects.iter().find(|pane| pane.id == below).unwrap();
        // Below the target (herdr split_horizontal).
        assert_eq!(lower.rect.y, 400);
        assert_eq!(lower.rect.x, 500);
    }

    #[test]
    fn grid_focus_moves_by_direction_and_stops_at_the_edge() {
        let mut layout = grid();
        // 1 top-left, 3 bottom-left, 2 top-right, 4 bottom-right.
        layout.focus_id(1);
        assert!(layout.focus_in(Direction::Right, area()));
        assert_eq!(layout.focus(), 2);
        assert!(layout.focus_in(Direction::Down, area()));
        assert_eq!(layout.focus(), 4);
        assert!(layout.focus_in(Direction::Left, area()));
        assert_eq!(layout.focus(), 3);
        // No neighbor: no wrap, focus stays (herdr NoNeighbor).
        assert!(!layout.focus_in(Direction::Down, area()));
        assert!(!layout.focus_in(Direction::Left, area()));
        assert_eq!(layout.focus(), 3);
    }

    #[test]
    fn cycle_walks_preorder_and_wraps() {
        let mut layout = grid();
        assert_eq!(layout.panes(), vec![1, 3, 2, 4]);
        layout.focus_id(4);
        layout.cycle(true);
        assert_eq!(layout.focus(), 1);
        layout.cycle(false);
        assert_eq!(layout.focus(), 4);
        layout.cycle(false);
        assert_eq!(layout.focus(), 2);
    }

    #[test]
    fn close_without_history_takes_the_next_pane_in_preorder() {
        let mut layout = grid();
        let saved = layout.view();
        let mut fresh = Layout::from_saved(saved, Some(3)).unwrap();
        // No previous focus after a restore: next in preorder (1,3,2,4).
        assert!(fresh.close_focused());
        assert_eq!(fresh.focus(), 2);
        // Last in preorder: the previous one.
        fresh.focus_id(4);
        layout = fresh;
        layout.prev_focus = None;
        assert!(layout.close_focused());
        assert_eq!(layout.focus(), 2);
    }

    #[test]
    fn close_and_split_end_zoom() {
        let mut layout = grid();
        assert!(layout.toggle_zoom());
        assert!(layout.zoomed());
        layout.split(Axis::SideBySide);
        assert!(!layout.zoomed());
        layout.toggle_zoom();
        assert!(layout.close_focused());
        assert!(!layout.zoomed());
    }

    #[test]
    fn swap_keeps_focus_on_the_moved_pane() {
        let mut layout = grid();
        layout.focus_id(1);
        assert!(layout.swap(Direction::Right, area()));
        assert_eq!(layout.focus(), 1);
        let rects = layout.rects(area());
        assert_eq!(rects.iter().find(|pane| pane.id == 1).unwrap().rect.x, 500);
        // No neighbor: nothing changes.
        assert!(!layout.swap(Direction::Right, area()));
    }

    #[test]
    fn saved_ratios_are_clamped_and_duplicates_refused() {
        let tree = ViewNode::Split {
            axis: Axis::SideBySide,
            ratio: 0.02,
            path: Vec::new(),
            first: Box::new(ViewNode::Leaf(1)),
            second: Box::new(ViewNode::Leaf(2)),
        };
        let layout = Layout::from_saved(tree, Some(2)).unwrap();
        assert_eq!(layout.ratio_millis(&[]), Some(100));
        assert_eq!(layout.focus(), 2);
        let dup = ViewNode::Split {
            axis: Axis::Stacked,
            ratio: 0.5,
            path: Vec::new(),
            first: Box::new(ViewNode::Leaf(1)),
            second: Box::new(ViewNode::Leaf(1)),
        };
        assert!(Layout::from_saved(dup, None).is_err());
    }
}
