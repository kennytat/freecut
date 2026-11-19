//! Rebuilds a guillotine cutting guide from a finished sheet layout.
//!
//! The incremental slicing tree in `GuillotineSheet` can only describe placements into its own
//! free leaves. Merged free rects allow placements it cannot describe, so for those sheets the
//! guide is derived from the final piece positions instead: find a full edge-to-edge cut that no
//! piece crosses (with a kerf-wide gap), split, and recurse. The same search decides whether a
//! layout can be cut at all.
//!
//! Greedy splitting is sufficient: restricting a guillotine pattern to either side of a cut that
//! crosses no piece leaves a guillotine pattern, so the first valid cut never closes off a
//! solution.

use crate::render::{
    Cut as GuideCut, CutOrientation, LeafKind as GuideLeafKind, PlacedPiece, Rect,
    SliceNode as GuideSliceNode,
};

/// Returns a cutting guide for `pieces` inside `region`, or `None` if they cannot be separated
/// by edge-to-edge cuts that leave `kerf_width` between neighbouring pieces.
pub(super) fn guillotine_guide(
    region: Rect,
    pieces: &[&PlacedPiece],
    kerf_width: u32,
) -> Option<GuideSliceNode> {
    match pieces {
        [] => return Some(GuideSliceNode::leaf(region, GuideLeafKind::Waste)),
        [piece] if piece.rect == region => return Some(piece_leaf(piece)),
        _ => {}
    }

    for orientation in [CutOrientation::Horizontal, CutOrientation::Vertical] {
        if let Some(node) = split_after_pieces(region, pieces, kerf_width, orientation) {
            return node;
        }
    }

    for orientation in [CutOrientation::Horizontal, CutOrientation::Vertical] {
        if let Some(node) = trim_leading_waste(region, pieces, kerf_width, orientation) {
            return node;
        }
    }

    // A single piece whose remaining slivers are no wider than the kerf needs no further cut.
    match pieces {
        [piece] => Some(piece_leaf(piece)),
        _ => None,
    }
}

/// Finds the first edge-to-edge cut along `orientation`: either a kerf-wide gap that separates
/// the pieces into two groups, or a trim cut after the last piece. The outer `Option` is "this
/// orientation produced a cut"; the inner one carries the recursive result.
#[allow(clippy::option_option)]
fn split_after_pieces(
    region: Rect,
    pieces: &[&PlacedPiece],
    kerf_width: u32,
    orientation: CutOrientation,
) -> Option<Option<GuideSliceNode>> {
    let region_start = start(region, orientation);
    let mut sorted = pieces.to_vec();
    sorted.sort_by_key(|piece| (start(piece.rect, orientation), end(piece.rect, orientation)));

    // Sweep by start: the first piece starting at least a kerf after every earlier piece ended
    // marks a cut that crosses no piece.
    let mut furthest_end = end(sorted[0].rect, orientation);
    let mut split_at = None;
    for (index, piece) in sorted.iter().enumerate().skip(1) {
        if start(piece.rect, orientation) >= furthest_end + kerf_width {
            split_at = Some(index);
            break;
        }
        furthest_end = furthest_end.max(end(piece.rect, orientation));
    }

    let offset = furthest_end - region_start;
    let cut = GuideCut::new(region, orientation, offset, kerf_width)?;
    let (first, second) = sorted.split_at(split_at.unwrap_or(sorted.len()));
    let (first_rect, second_rect) = split_region(region, orientation, offset, kerf_width);

    Some(
        guillotine_guide(first_rect, first, kerf_width).and_then(|first_node| {
            guillotine_guide(second_rect, second, kerf_width)
                .map(|second_node| GuideSliceNode::cut(cut, first_node, second_node))
        }),
    )
}

/// Cuts off waste in front of every piece, e.g. a region whose pieces start after a gap.
#[allow(clippy::option_option)]
fn trim_leading_waste(
    region: Rect,
    pieces: &[&PlacedPiece],
    kerf_width: u32,
    orientation: CutOrientation,
) -> Option<Option<GuideSliceNode>> {
    let region_start = start(region, orientation);
    let first_piece_start = pieces
        .iter()
        .map(|piece| start(piece.rect, orientation))
        .min()?;
    let offset = first_piece_start
        .checked_sub(region_start)?
        .checked_sub(kerf_width)?;
    let cut = GuideCut::new(region, orientation, offset, kerf_width)?;
    let (waste_rect, rest_rect) = split_region(region, orientation, offset, kerf_width);

    Some(guillotine_guide(rest_rect, pieces, kerf_width).map(|rest| {
        GuideSliceNode::cut(
            cut,
            GuideSliceNode::leaf(waste_rect, GuideLeafKind::Waste),
            rest,
        )
    }))
}

fn piece_leaf(piece: &PlacedPiece) -> GuideSliceNode {
    GuideSliceNode::leaf(
        piece.rect,
        GuideLeafKind::CutPiece {
            cut_id: piece.cut_id,
            instance: piece.instance,
        },
    )
}

fn start(rect: Rect, orientation: CutOrientation) -> u32 {
    match orientation {
        CutOrientation::Horizontal => rect.y,
        CutOrientation::Vertical => rect.x,
    }
}

fn end(rect: Rect, orientation: CutOrientation) -> u32 {
    match orientation {
        CutOrientation::Horizontal => rect.y + rect.length,
        CutOrientation::Vertical => rect.x + rect.width,
    }
}

/// Splits `region` at `offset` along `orientation`, leaving a kerf-wide gap between the parts.
fn split_region(
    region: Rect,
    orientation: CutOrientation,
    offset: u32,
    kerf_width: u32,
) -> (Rect, Rect) {
    match orientation {
        CutOrientation::Horizontal => (
            Rect {
                length: offset,
                ..region
            },
            Rect {
                y: region.y + offset + kerf_width,
                length: region.length - offset - kerf_width,
                ..region
            },
        ),
        CutOrientation::Vertical => (
            Rect {
                width: offset,
                ..region
            },
            Rect {
                x: region.x + offset + kerf_width,
                width: region.width - offset - kerf_width,
                ..region
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PatternDirection, PieceId};

    fn piece(id: u64, x: u32, y: u32, width: u32, length: u32) -> PlacedPiece {
        PlacedPiece {
            cut_id: PieceId(id),
            instance: 0,
            rect: Rect {
                x,
                y,
                width,
                length,
            },
            pattern: PatternDirection::None,
            rotated: false,
        }
    }

    fn sheet(width: u32, length: u32) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width,
            length,
        }
    }

    fn piece_leaf_count(node: &GuideSliceNode) -> usize {
        match node {
            GuideSliceNode::Cut { first, second, .. } => {
                piece_leaf_count(first) + piece_leaf_count(second)
            }
            GuideSliceNode::Leaf {
                kind: GuideLeafKind::CutPiece { .. },
                ..
            } => 1,
            GuideSliceNode::Leaf { .. } => 0,
        }
    }

    #[test]
    fn builds_guide_with_every_piece_and_kerf_gaps() {
        let pieces = [
            piece(1, 0, 0, 50, 30),
            piece(2, 0, 32, 50, 68),
            piece(3, 52, 0, 48, 100),
        ];
        let refs = pieces.iter().collect::<Vec<_>>();

        let guide = guillotine_guide(sheet(100, 100), &refs, 2).expect("layout is cuttable");

        assert_eq!(piece_leaf_count(&guide), 3);
        let GuideSliceNode::Cut { cut, .. } = &guide else {
            panic!("first node should be a cut");
        };
        assert_eq!(cut.orientation(), CutOrientation::Vertical);
        assert_eq!((cut.offset(), cut.kerf_width()), (50, 2));
    }

    #[test]
    fn rejects_pinwheel_layout() {
        // Four pieces around a centre square: every straight cut crosses a piece.
        let pieces = [
            piece(1, 0, 0, 60, 40),
            piece(2, 60, 0, 40, 60),
            piece(3, 40, 60, 60, 40),
            piece(4, 0, 40, 40, 60),
            piece(5, 40, 40, 20, 20),
        ];
        let refs = pieces.iter().collect::<Vec<_>>();

        assert!(guillotine_guide(sheet(100, 100), &refs, 0).is_none());
    }

    #[test]
    fn rejects_neighbours_closer_than_the_kerf() {
        let pieces = [piece(1, 0, 0, 50, 100), piece(2, 51, 0, 49, 100)];
        let refs = pieces.iter().collect::<Vec<_>>();

        assert!(guillotine_guide(sheet(100, 100), &refs, 1).is_some());
        assert!(guillotine_guide(sheet(100, 100), &refs, 2).is_none());
    }

    #[test]
    fn trims_waste_around_a_single_piece() {
        let pieces = [piece(1, 10, 10, 30, 30)];
        let refs = pieces.iter().collect::<Vec<_>>();

        let guide = guillotine_guide(sheet(100, 100), &refs, 1).expect("one piece is cuttable");

        assert_eq!(piece_leaf_count(&guide), 1);
        assert!(guide.preorder_cuts().count() >= 2);
    }
}
