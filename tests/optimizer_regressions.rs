use freecut::{
    domain::{
        CutPiece, CutSettings, LayoutKind, PatternDirection, PieceId, Project, StockPiece, Unit,
    },
    measurement::parse_length,
    optimizer::{BaselineOptimizer, OptimizeError, OptimizerConfig, OptimizerEffort},
    project_io::{load_project_document_from_str, PROJECT_FILE_VERSION},
    render::Rect,
};

#[test]
fn balanced_optimizer_places_gui_case_after_adding_one_more_small_cut() {
    let project = Project {
        name: "repro".to_string(),
        stock_pieces: vec![StockPiece {
            id: PieceId(1),
            width: 2440,
            length: 1220,
            quantity: Some(1),
            pattern: PatternDirection::None,
            enabled: true,
            offset_width: 0,
            offset_length: 0,
            required: false,
        }],
        cut_pieces: vec![
            CutPiece {
                id: PieceId(2),
                label: "cut-2".to_string(),
                width: 800,
                length: 100,
                quantity: 15,
                pattern: PatternDirection::None,
                can_rotate: true,
                enabled: true,
                assigned_stock: None,
            },
            CutPiece {
                id: PieceId(3),
                label: "cut-3".to_string(),
                width: 100,
                length: 78,
                quantity: 44,
                pattern: PatternDirection::None,
                can_rotate: true,
                enabled: true,
                assigned_stock: None,
            },
            CutPiece {
                id: PieceId(4),
                label: "cut-4".to_string(),
                width: 200,
                length: 60,
                quantity: 10,
                pattern: PatternDirection::None,
                can_rotate: true,
                enabled: true,
                assigned_stock: None,
            },
            CutPiece {
                id: PieceId(5),
                label: "cut-5".to_string(),
                width: 150,
                length: 150,
                quantity: 35,
                pattern: PatternDirection::None,
                can_rotate: true,
                enabled: true,
                assigned_stock: None,
            },
        ],
        settings: CutSettings {
            unit: Unit::Millimeter,
            kerf_width: 0,
            layout: LayoutKind::Guillotine,
        },
    };

    let solution = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Balanced))
        .expect("should fit");

    assert_eq!(solution.sheets.len(), 1);
    assert_eq!(solution.sheets[0].placed_pieces.len(), 104);
}

#[test]
fn optimizer_skips_disabled_pieces_and_uses_usable_stock_area() {
    let project = Project {
        name: "enabled-and-offset".to_string(),
        stock_pieces: vec![
            StockPiece {
                id: PieceId(1),
                width: 1000,
                length: 500,
                quantity: Some(1),
                pattern: PatternDirection::None,
                enabled: true,
                offset_width: 100,
                offset_length: 50,
                required: false,
            },
            StockPiece {
                id: PieceId(2),
                width: 5000,
                length: 5000,
                quantity: Some(1),
                pattern: PatternDirection::None,
                enabled: false,
                offset_width: 0,
                offset_length: 0,
                required: false,
            },
        ],
        cut_pieces: vec![
            CutPiece {
                id: PieceId(3),
                label: "fits-usable".to_string(),
                width: 900,
                length: 450,
                quantity: 1,
                pattern: PatternDirection::None,
                can_rotate: false,
                enabled: true,
                assigned_stock: None,
            },
            CutPiece {
                id: PieceId(4),
                label: "excluded".to_string(),
                width: 4000,
                length: 4000,
                quantity: 1,
                pattern: PatternDirection::None,
                can_rotate: false,
                enabled: false,
                assigned_stock: None,
            },
        ],
        settings: CutSettings {
            unit: Unit::Millimeter,
            kerf_width: 0,
            layout: LayoutKind::Guillotine,
        },
    };

    let solution = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Fast))
        .expect("enabled cut fits the usable stock area");

    assert_eq!(solution.sheets.len(), 1);
    let sheet = &solution.sheets[0];
    assert_eq!(sheet.stock_id, PieceId(1));
    // Output shows the full stock; the offset strips are waste and pieces stay in the usable area.
    assert_eq!((sheet.width, sheet.length), (1000, 500));
    assert!(sheet.waste.contains(&Rect {
        x: 900,
        y: 0,
        width: 100,
        length: 500,
    }));
    assert!(sheet.waste.contains(&Rect {
        x: 0,
        y: 450,
        width: 900,
        length: 50,
    }));
    assert_eq!(sheet.placed_pieces.len(), 1);
    assert_eq!(sheet.placed_pieces[0].cut_id, PieceId(3));
    let placed = sheet.placed_pieces[0].rect;
    assert!(placed.x + placed.width <= 900 && placed.y + placed.length <= 450);

    let mut too_big = project.clone();
    too_big.stock_pieces[0].offset_width = 101;
    assert!(BaselineOptimizer
        .optimize_with_config(&too_big, OptimizerConfig::new(OptimizerEffort::Fast))
        .is_err());
}

fn required_stock_project(layout: LayoutKind, offcut_quantity: u32, required: bool) -> Project {
    Project {
        name: "required-stock".to_string(),
        stock_pieces: vec![
            StockPiece {
                id: PieceId(1),
                width: 1000,
                length: 1000,
                quantity: Some(1),
                pattern: PatternDirection::None,
                enabled: true,
                offset_width: 0,
                offset_length: 0,
                required: false,
            },
            StockPiece {
                id: PieceId(2),
                width: 300,
                length: 300,
                quantity: Some(offcut_quantity),
                pattern: PatternDirection::None,
                enabled: true,
                offset_width: 0,
                offset_length: 0,
                required,
            },
        ],
        cut_pieces: vec![cut(3, 200, 200, 4, true)],
        settings: CutSettings {
            unit: Unit::Millimeter,
            kerf_width: 0,
            layout,
        },
    }
}

#[test]
fn required_stock_is_used_by_every_effort_and_layout() {
    for layout in [LayoutKind::Guillotine, LayoutKind::Nested] {
        for effort in [
            OptimizerEffort::Fast,
            OptimizerEffort::Balanced,
            OptimizerEffort::Thorough,
        ] {
            let optional = BaselineOptimizer
                .optimize_with_config(
                    &required_stock_project(layout, 1, false),
                    OptimizerConfig::new(effort),
                )
                .expect("all cuts fit on the large sheet");
            assert_eq!(optional.sheets.len(), 1, "{layout:?} {effort:?}");
            assert_eq!(optional.sheets[0].stock_id, PieceId(1));

            let required = BaselineOptimizer
                .optimize_with_config(
                    &required_stock_project(layout, 2, true),
                    OptimizerConfig::new(effort),
                )
                .expect("required offcuts can each take one cut");
            let offcut_sheets = required
                .sheets
                .iter()
                .filter(|sheet| sheet.stock_id == PieceId(2))
                .collect::<Vec<_>>();
            assert_eq!(offcut_sheets.len(), 2, "{layout:?} {effort:?}");
            assert!(offcut_sheets
                .iter()
                .all(|sheet| !sheet.placed_pieces.is_empty()));
            assert_eq!(
                required
                    .sheets
                    .iter()
                    .map(|sheet| sheet.placed_pieces.len())
                    .sum::<usize>(),
                4
            );
            assert_solution_within_bounds_and_non_overlapping(&required);
        }
    }
}

#[test]
fn required_stock_without_enough_cuts_has_no_solution() {
    let error = BaselineOptimizer
        .optimize_with_config(
            &required_stock_project(LayoutKind::Guillotine, 5, true),
            OptimizerConfig::new(OptimizerEffort::Thorough),
        )
        .expect_err("five required sheets cannot each get one of four cuts");
    assert_eq!(error, OptimizeError::NoSolution);

    let mut disabled = required_stock_project(LayoutKind::Guillotine, 5, true);
    disabled.stock_pieces[1].enabled = false;
    let solution = BaselineOptimizer
        .optimize_with_config(&disabled, OptimizerConfig::new(OptimizerEffort::Fast))
        .expect("excluded stock is not required");
    assert_eq!(solution.sheets.len(), 1);
}

#[test]
fn assigned_cuts_are_only_placed_on_their_stock_for_every_effort_and_layout() {
    for layout in [LayoutKind::Guillotine, LayoutKind::Nested] {
        for effort in [
            OptimizerEffort::Fast,
            OptimizerEffort::Balanced,
            OptimizerEffort::Thorough,
        ] {
            let mut project = required_stock_project(layout, 2, false);
            project.cut_pieces.push(CutPiece {
                assigned_stock: Some(PieceId(2)),
                ..cut(4, 250, 250, 2, true)
            });

            let solution = BaselineOptimizer
                .optimize_with_config(&project, OptimizerConfig::new(effort))
                .expect("assigned cuts fit their offcuts");

            for sheet in &solution.sheets {
                for piece in &sheet.placed_pieces {
                    if piece.cut_id == PieceId(4) {
                        assert_eq!(sheet.stock_id, PieceId(2), "{layout:?} {effort:?}");
                    }
                }
            }
            assert_eq!(
                solution
                    .sheets
                    .iter()
                    .map(|sheet| sheet.placed_pieces.len())
                    .sum::<usize>(),
                6
            );
            assert_solution_within_bounds_and_non_overlapping(&solution);
        }
    }
}

#[test]
fn assigned_cut_that_does_not_fit_its_stock_has_no_solution() {
    let mut project = required_stock_project(LayoutKind::Guillotine, 1, false);
    project.cut_pieces.push(CutPiece {
        assigned_stock: Some(PieceId(2)),
        ..cut(4, 400, 400, 1, true)
    });

    let error = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Thorough))
        .expect_err("a 400 x 400 cut cannot go on its assigned 300 x 300 stock");
    assert_eq!(error, OptimizeError::NoSolution);
}

#[test]
fn thorough_guillotine_places_q71_side_strip_case() {
    let project = Project {
        name: "side-strip-repro".to_string(),
        stock_pieces: vec![StockPiece {
            id: PieceId(2),
            width: 2440,
            length: 1220,
            quantity: Some(1),
            pattern: PatternDirection::None,
            enabled: true,
            offset_width: 0,
            offset_length: 0,
            required: false,
        }],
        cut_pieces: vec![
            CutPiece {
                id: PieceId(1),
                label: "cut-1".to_string(),
                width: 100,
                length: 100,
                quantity: 85,
                pattern: PatternDirection::None,
                can_rotate: true,
                enabled: true,
                assigned_stock: None,
            },
            CutPiece {
                id: PieceId(3),
                label: "cut-3".to_string(),
                width: 234,
                length: 344,
                quantity: 21,
                pattern: PatternDirection::None,
                can_rotate: true,
                enabled: true,
                assigned_stock: None,
            },
            CutPiece {
                id: PieceId(4),
                label: "cut-4".to_string(),
                width: 40,
                length: 120,
                quantity: 71,
                pattern: PatternDirection::None,
                can_rotate: true,
                enabled: true,
                assigned_stock: None,
            },
        ],
        settings: CutSettings {
            unit: Unit::Millimeter,
            kerf_width: 1,
            layout: LayoutKind::Guillotine,
        },
    };

    let solution = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Thorough))
        .expect("q71 side-strip case should fit on one sheet");

    assert_eq!(solution.sheets.len(), 1);
    assert_eq!(solution.sheets[0].placed_pieces.len(), 177);
    assert_eq!(
        solution.sheets[0]
            .placed_pieces
            .iter()
            .filter(|piece| piece.cut_id == PieceId(1))
            .count(),
        85
    );
    assert_eq!(
        solution.sheets[0]
            .placed_pieces
            .iter()
            .filter(|piece| piece.cut_id == PieceId(3))
            .count(),
        21
    );
    assert_eq!(
        solution.sheets[0]
            .placed_pieces
            .iter()
            .filter(|piece| piece.cut_id == PieceId(4))
            .count(),
        71
    );
}

#[test]
fn guillotine_thorough_is_independent_of_stock_input_order_for_issue_35() {
    for source in [
        include_str!("fixtures/issue-35-pass-v1.freecut.json"),
        include_str!("fixtures/issue-35-fail-v1.freecut.json"),
    ] {
        let document = load_project_document_from_str(source).expect("issue #35 fixture loads");
        assert_eq!(document.version, PROJECT_FILE_VERSION);
        assert_eq!(document.optimizer_effort, OptimizerEffort::Thorough);

        let solution = BaselineOptimizer
            .optimize_with_config(
                &document.project,
                OptimizerConfig::new(document.optimizer_effort),
            )
            .expect("issue #35 stock ordering should not decide whether a solution exists");

        assert_eq!(solution.sheets.len(), 4);
        assert_eq!(
            solution
                .sheets
                .iter()
                .map(|sheet| sheet.placed_pieces.len())
                .sum::<usize>(),
            21
        );
        assert_solution_within_bounds_and_non_overlapping(&solution);
    }
}

#[test]
fn optimizer_assigns_cuts_when_every_required_sheet_must_be_filled_tightly() {
    // All ten sheets are required and the cuts fill 93% of them; no greedy construction places
    // every cut, so the search has to repair a near miss instead of reporting no solution.
    let document = load_project_document_from_str(include_str!(
        "fixtures/wardrobe-required-unassigned-v2.freecut.json"
    ))
    .expect("wardrobe fixture loads");
    assert!(document
        .project
        .cut_pieces
        .iter()
        .all(|cut| cut.assigned_stock.is_none()));

    for effort in [OptimizerEffort::Balanced, OptimizerEffort::Thorough] {
        let solution = BaselineOptimizer
            .optimize_with_config(&document.project, OptimizerConfig::new(effort))
            .expect("optimizer should find a layout without manual stock assignment");

        assert_eq!(solution.sheets.len(), 10);
        assert_eq!(
            solution
                .sheets
                .iter()
                .map(|sheet| sheet.placed_pieces.len())
                .sum::<usize>(),
            32
        );
        assert_solution_within_bounds_and_non_overlapping(&solution);
    }
}

#[test]
fn fractional_inch_kerf_controls_exact_fit_for_all_layouts() {
    for layout in [LayoutKind::Guillotine, LayoutKind::Nested] {
        let exact = fractional_kerf_project(layout, "1/8");
        let too_wide = fractional_kerf_project(layout, "0.126");

        let solution = BaselineOptimizer
            .optimize_with_config(&exact, OptimizerConfig::new(OptimizerEffort::Fast))
            .expect("two pieces and a 1/8 inch kerf should fit exactly");
        let error = BaselineOptimizer
            .optimize_with_config(&too_wide, OptimizerConfig::new(OptimizerEffort::Fast))
            .expect_err("a kerf wider than 1/8 inch should not fit");

        assert_eq!(solution.sheets.len(), 1);
        assert_eq!(solution.sheets[0].placed_pieces.len(), 2);
        assert_solution_within_bounds_and_non_overlapping(&solution);
        assert_eq!(error, OptimizeError::NoSolution);
    }
}

#[test]
fn nested_places_small_non_guillotine_advantage_case() {
    let nested = nested_project(
        10,
        10,
        vec![
            cut(10, 2, 2, 1, false),
            cut(20, 2, 3, 1, false),
            cut(30, 2, 6, 1, false),
            cut(40, 7, 8, 1, false),
        ],
    );
    let mut guillotine = nested.clone();
    guillotine.settings.layout = LayoutKind::Guillotine;

    let nested_solution = BaselineOptimizer
        .optimize_with_config(&nested, OptimizerConfig::new(OptimizerEffort::Thorough))
        .expect("nested should place this small non-guillotine layout");
    let guillotine_error = BaselineOptimizer
        .optimize_with_config(&guillotine, OptimizerConfig::new(OptimizerEffort::Thorough))
        .expect_err("guillotine should not place this non-sliceable layout with one sheet");

    assert_eq!(guillotine_error, OptimizeError::NoSolution);
    assert_eq!(nested_solution.sheets.len(), 1);
    assert_eq!(nested_solution.sheets[0].placed_pieces.len(), 4);
    assert_solution_within_bounds_and_non_overlapping(&nested_solution);
}

#[test]
fn nested_rotates_narrow_piece_into_remaining_strip() {
    let project = nested_project(
        100,
        60,
        vec![cut(10, 60, 60, 1, false), cut(20, 50, 40, 1, true)],
    );

    let solution = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Thorough))
        .expect("rotated narrow piece should fit in the side strip");

    assert_eq!(solution.sheets.len(), 1);
    assert_eq!(solution.sheets[0].placed_pieces.len(), 2);
    assert!(solution.sheets[0]
        .placed_pieces
        .iter()
        .any(|piece| piece.cut_id == PieceId(20) && piece.rotated));
    assert_solution_within_bounds_and_non_overlapping(&solution);
}

#[test]
fn nested_uses_pattern_wildcard_like_guillotine() {
    let project = Project {
        name: "nested-pattern-wildcard".to_string(),
        stock_pieces: vec![StockPiece {
            id: PieceId(1),
            width: 100,
            length: 100,
            quantity: Some(1),
            pattern: PatternDirection::ParallelToWidth,
            enabled: true,
            offset_width: 0,
            offset_length: 0,
            required: false,
        }],
        cut_pieces: vec![cut(10, 50, 50, 1, false)],
        settings: CutSettings {
            unit: Unit::Millimeter,
            kerf_width: 0,
            layout: LayoutKind::Nested,
        },
    };

    let solution = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Fast))
        .expect("patternless cut should fit patterned stock");

    assert_eq!(solution.sheets.len(), 1);
    assert_eq!(solution.sheets[0].placed_pieces.len(), 1);
    assert_eq!(
        solution.sheets[0].placed_pieces[0].pattern,
        PatternDirection::None
    );
}

#[test]
fn nested_respects_finite_stock_quantity() {
    let project = nested_project(100, 100, vec![cut(10, 60, 100, 2, false)]);

    let error = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Thorough))
        .expect_err("one finite stock sheet cannot hold both cuts");

    assert_eq!(error, OptimizeError::NoSolution);
}

#[test]
fn nested_rejects_when_total_cut_area_exceeds_stock_area() {
    let project = nested_project(100, 100, vec![cut(10, 80, 80, 2, false)]);

    let error = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Fast))
        .expect_err("total cut area exceeds available stock area");

    assert_eq!(error, OptimizeError::NoSolution);
}

#[test]
fn nested_rejects_individually_unpassable_cut() {
    let project = nested_project(100, 100, vec![cut(10, 101, 50, 1, false)]);

    let error = BaselineOptimizer
        .optimize_with_config(&project, OptimizerConfig::new(OptimizerEffort::Fast))
        .expect_err("cut is wider than any stock piece");

    assert_eq!(error, OptimizeError::NoSolution);
}

#[test]
fn nested_repeats_deterministically_for_all_effort_levels() {
    let project = nested_project(
        180,
        120,
        vec![
            cut(10, 70, 40, 1, true),
            cut(20, 60, 50, 1, true),
            cut(30, 30, 80, 1, true),
            cut(40, 25, 25, 3, true),
        ],
    );

    for effort in [
        OptimizerEffort::Fast,
        OptimizerEffort::Balanced,
        OptimizerEffort::Thorough,
    ] {
        let first = BaselineOptimizer
            .optimize_with_config(&project, OptimizerConfig::new(effort))
            .expect("first nested run should produce a solution");
        let second = BaselineOptimizer
            .optimize_with_config(&project, OptimizerConfig::new(effort))
            .expect("second nested run should produce a solution");

        assert_eq!(first, second);
    }
}

#[test]
fn guillotine_places_rotation_disabled_small_cut_cases_at_every_effort() {
    for disabled_cut_id in [4, 5] {
        let project = rotation_disabled_regression_project(LayoutKind::Guillotine, disabled_cut_id);

        // Diversified construction lets even Fast find this layout; it used to need Balanced.
        for effort in [
            OptimizerEffort::Fast,
            OptimizerEffort::Balanced,
            OptimizerEffort::Thorough,
        ] {
            let solution = BaselineOptimizer
                .optimize_with_config(&project, OptimizerConfig::new(effort))
                .expect("guillotine search should place this practical case on one sheet");

            assert_eq!(solution.sheets.len(), 1, "{effort:?}");
            assert_eq!(solution.sheets[0].placed_pieces.len(), 47);
            assert!(solution.sheets[0].cutting_guide.is_some());
            assert_solution_within_bounds_and_non_overlapping(&solution);
        }
    }
}

#[test]
fn nested_balanced_and_thorough_place_wide_panel_rotation_disabled_case() {
    let project = rotation_disabled_regression_project(LayoutKind::Nested, 2);

    for effort in [OptimizerEffort::Balanced, OptimizerEffort::Thorough] {
        let solution = BaselineOptimizer
            .optimize_with_config(&project, OptimizerConfig::new(effort))
            .expect("nested should handle the wide-panel case without requiring rotation");

        assert_eq!(solution.sheets.len(), 1);
        assert_eq!(solution.sheets[0].placed_pieces.len(), 47);
        assert_solution_within_bounds_and_non_overlapping(&solution);
    }
}

fn nested_project(stock_width: u32, stock_length: u32, cut_pieces: Vec<CutPiece>) -> Project {
    Project {
        name: "nested-regression".to_string(),
        stock_pieces: vec![StockPiece {
            id: PieceId(1),
            width: stock_width,
            length: stock_length,
            quantity: Some(1),
            pattern: PatternDirection::None,
            enabled: true,
            offset_width: 0,
            offset_length: 0,
            required: false,
        }],
        cut_pieces,
        settings: CutSettings {
            unit: Unit::Millimeter,
            kerf_width: 0,
            layout: LayoutKind::Nested,
        },
    }
}

fn rotation_disabled_regression_project(layout: LayoutKind, disabled_cut_id: u64) -> Project {
    Project {
        name: "rotation-disabled-regression".to_string(),
        stock_pieces: vec![StockPiece {
            id: PieceId(1),
            width: 2440,
            length: 1220,
            quantity: Some(1),
            pattern: PatternDirection::None,
            enabled: true,
            offset_width: 0,
            offset_length: 0,
            required: false,
        }],
        cut_pieces: vec![
            cut(2, 500, 620, 4, disabled_cut_id != 2),
            cut(3, 1223, 220, 3, disabled_cut_id != 3),
            cut(4, 110, 100, 30, disabled_cut_id != 4),
            cut(5, 100, 200, 10, disabled_cut_id != 5),
        ],
        settings: CutSettings {
            unit: Unit::Millimeter,
            kerf_width: 2,
            layout,
        },
    }
}

fn fractional_kerf_project(layout: LayoutKind, kerf: &str) -> Project {
    let inch = |source| parse_length(source, Unit::Inch).expect("fixture length parses");

    Project {
        name: "fractional-inch-kerf".to_string(),
        stock_pieces: vec![StockPiece {
            id: PieceId(1),
            width: inch("2 1/8"),
            length: inch("1"),
            quantity: Some(1),
            pattern: PatternDirection::None,
            enabled: true,
            offset_width: 0,
            offset_length: 0,
            required: false,
        }],
        cut_pieces: vec![CutPiece {
            id: PieceId(2),
            label: "one-inch-square".to_string(),
            width: inch("1"),
            length: inch("1"),
            quantity: 2,
            pattern: PatternDirection::None,
            can_rotate: false,
            enabled: true,
            assigned_stock: None,
        }],
        settings: CutSettings {
            unit: Unit::Inch,
            kerf_width: inch(kerf),
            layout,
        },
    }
}

fn cut(id: u64, width: u32, length: u32, quantity: u32, can_rotate: bool) -> CutPiece {
    CutPiece {
        id: PieceId(id),
        label: format!("cut-{id}"),
        width,
        length,
        quantity,
        pattern: PatternDirection::None,
        can_rotate,
        enabled: true,
        assigned_stock: None,
    }
}

fn assert_solution_within_bounds_and_non_overlapping(solution: &freecut::render::Solution) {
    for sheet in &solution.sheets {
        for (index, placed) in sheet.placed_pieces.iter().enumerate() {
            assert!(
                placed.rect.x + placed.rect.width <= sheet.width,
                "placed piece {index} exceeds sheet width"
            );
            assert!(
                placed.rect.y + placed.rect.length <= sheet.length,
                "placed piece {index} exceeds sheet length"
            );

            for other in sheet.placed_pieces.iter().skip(index + 1) {
                assert!(
                    !rects_overlap(placed.rect, other.rect),
                    "placed pieces overlap: {:?} and {:?}",
                    placed.rect,
                    other.rect
                );
            }
        }
    }
}

fn rects_overlap(left: Rect, right: Rect) -> bool {
    left.x < right.x + right.width
        && left.x + left.width > right.x
        && left.y < right.y + right.length
        && left.y + left.length > right.y
}
