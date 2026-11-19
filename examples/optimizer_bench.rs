//! Quality benchmark for the cut optimizer.
//!
//! Run with `cargo run --release --example optimizer_bench`. Each row reports the sheets used
//! against the area lower bound, material utilization of the used sheets, run time, and how many
//! guillotine sheets fail an independent edge-to-edge cuttability check.

use std::time::Instant;

use freecut::{
    domain::{
        CutPiece, CutSettings, LayoutKind, PatternDirection, PieceId, Project, StockPiece, Unit,
    },
    measurement::length_from_whole_units,
    optimizer::{BaselineOptimizer, OptimizerConfig, OptimizerEffort},
    render::{Rect, Solution},
};

struct Prng(u64);

impl Prng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn range(&mut self, low: u32, high: u32) -> u32 {
        low + u32::try_from(self.next() % u64::from(high - low + 1)).unwrap()
    }
}

fn mm(value: u32) -> u32 {
    length_from_whole_units(value, Unit::Millimeter).unwrap()
}

fn stock(id: u64, width: u32, length: u32, quantity: u32) -> StockPiece {
    StockPiece {
        id: PieceId(id),
        width: mm(width),
        length: mm(length),
        quantity: Some(quantity),
        pattern: PatternDirection::None,
        enabled: true,
        offset_width: 0,
        offset_length: 0,
        required: false,
    }
}

fn cut(id: u64, width: u32, length: u32, quantity: u32) -> CutPiece {
    CutPiece {
        id: PieceId(id),
        label: format!("cut-{id}"),
        width: mm(width),
        length: mm(length),
        quantity,
        pattern: PatternDirection::None,
        can_rotate: true,
        assigned_stock: None,
        enabled: true,
    }
}

fn project(name: &str, stock_pieces: Vec<StockPiece>, cut_pieces: Vec<CutPiece>) -> Project {
    Project {
        name: name.to_string(),
        stock_pieces,
        cut_pieces,
        settings: CutSettings {
            unit: Unit::Millimeter,
            kerf_width: mm(3),
            layout: LayoutKind::Guillotine,
        },
    }
}

fn random_project(
    name: &str,
    seed: u64,
    types: u64,
    width: (u32, u32),
    length: (u32, u32),
    quantity: (u32, u32),
) -> Project {
    let mut prng = Prng(seed);
    let cuts = (0..types)
        .map(|index| {
            cut(
                index + 2,
                prng.range(width.0, width.1),
                prng.range(length.0, length.1),
                prng.range(quantity.0, quantity.1),
            )
        })
        .collect();
    project(name, vec![stock(1, 2440, 1220, 200)], cuts)
}

fn benchmark_projects() -> Vec<Project> {
    vec![
        project(
            "kitchen-cabinets",
            vec![stock(1, 2440, 1220, 50)],
            vec![
                cut(2, 720, 560, 12),
                cut(3, 560, 600, 6),
                cut(4, 764, 560, 8),
                cut(5, 600, 100, 10),
                cut(6, 718, 396, 10),
                cut(7, 2100, 600, 4),
                cut(8, 900, 560, 4),
                cut(9, 450, 420, 8),
            ],
        ),
        project(
            "wardrobe",
            vec![stock(1, 2800, 2070, 30)],
            vec![
                cut(2, 2400, 600, 4),
                cut(3, 1200, 600, 6),
                cut(4, 1164, 580, 8),
                cut(5, 1164, 120, 6),
                cut(6, 2380, 400, 2),
                cut(7, 580, 380, 12),
            ],
        ),
        random_project("random-medium", 11, 20, (100, 1200), (100, 600), (1, 6)),
        random_project("random-small-many", 22, 30, (50, 400), (50, 300), (2, 12)),
        random_project("random-large-few", 33, 10, (500, 1400), (300, 1100), (1, 3)),
        random_project("strips", 44, 12, (800, 2400), (60, 220), (2, 10)),
        project(
            "mixed-stock",
            vec![
                stock(1, 2440, 1220, 10),
                stock(2, 1220, 1220, 10),
                stock(3, 1220, 610, 10),
            ],
            vec![
                cut(4, 1100, 580, 5),
                cut(5, 600, 400, 7),
                cut(6, 1200, 300, 4),
                cut(7, 300, 300, 9),
            ],
        ),
    ]
}

fn area(width: u32, length: u32) -> u128 {
    u128::from(width) * u128::from(length)
}

/// Returns true if `pieces` inside `region` can be separated by recursive edge-to-edge cuts.
fn guillotine_cuttable(pieces: &[Rect]) -> bool {
    if pieces.len() <= 1 {
        return true;
    }

    for vertical in [true, false] {
        let mut edges = pieces
            .iter()
            .map(|piece| {
                if vertical {
                    piece.x + piece.width
                } else {
                    piece.y + piece.length
                }
            })
            .collect::<Vec<_>>();
        edges.sort_unstable();
        edges.dedup();

        for edge in edges {
            let (first, second): (Vec<Rect>, Vec<Rect>) = pieces.iter().partition(|piece| {
                if vertical {
                    piece.x + piece.width <= edge
                } else {
                    piece.y + piece.length <= edge
                }
            });
            let separated = second.iter().all(|piece| {
                if vertical {
                    piece.x >= edge
                } else {
                    piece.y >= edge
                }
            });
            if separated && !first.is_empty() && !second.is_empty() {
                return guillotine_cuttable(&first) && guillotine_cuttable(&second);
            }
        }
    }

    false
}

fn report(solution: &Solution) -> (usize, f64, f64, usize, usize) {
    let cut_area = solution
        .sheets
        .iter()
        .flat_map(|sheet| &sheet.placed_pieces)
        .map(|piece| area(piece.rect.width, piece.rect.length))
        .sum::<u128>();
    let stock_area = solution
        .sheets
        .iter()
        .map(|sheet| area(sheet.width, sheet.length))
        .sum::<u128>();
    let not_cuttable = solution
        .sheets
        .iter()
        .filter(|sheet| {
            let rects = sheet
                .placed_pieces
                .iter()
                .map(|piece| piece.rect)
                .collect::<Vec<_>>();
            !guillotine_cuttable(&rects)
        })
        .count();
    let missing_guide = solution
        .sheets
        .iter()
        .filter(|sheet| sheet.cutting_guide.is_none())
        .count();
    #[allow(clippy::cast_precision_loss)]
    let emptiest_fill = solution
        .sheets
        .iter()
        .map(|sheet| {
            let placed = sheet
                .placed_pieces
                .iter()
                .map(|piece| area(piece.rect.width, piece.rect.length))
                .sum::<u128>();
            placed as f64 / area(sheet.width, sheet.length) as f64
        })
        .fold(1.0, f64::min);
    #[allow(clippy::cast_precision_loss)]
    let utilization = cut_area as f64 / stock_area as f64;
    (
        solution.sheets.len(),
        utilization,
        emptiest_fill,
        not_cuttable,
        missing_guide,
    )
}

fn lower_bound(project: &Project) -> u128 {
    let cut_area = project
        .cut_pieces
        .iter()
        .map(|cut| area(cut.width, cut.length) * u128::from(cut.quantity))
        .sum::<u128>();
    let largest_stock = project
        .stock_pieces
        .iter()
        .map(|stock| area(stock.width, stock.length))
        .max()
        .unwrap_or(1);
    cut_area.div_ceil(largest_stock)
}

fn main() {
    println!(
        "{:<18} {:<10} {:<9} {:>3} {:>6} {:>7} {:>8} {:>9} {:>8}",
        "project", "layout", "effort", "lb", "sheets", "util", "minFill", "ms", "bad/noGd"
    );

    for base in benchmark_projects() {
        let lb = lower_bound(&base);
        for layout in [LayoutKind::Guillotine, LayoutKind::Nested] {
            for effort in [
                OptimizerEffort::Fast,
                OptimizerEffort::Balanced,
                OptimizerEffort::Thorough,
            ] {
                let mut project = base.clone();
                project.settings.layout = layout;
                let started = Instant::now();
                let result =
                    BaselineOptimizer.optimize_with_config(&project, OptimizerConfig::new(effort));
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;

                match result {
                    Ok(solution) => {
                        let (sheets, utilization, emptiest_fill, not_cuttable, missing_guide) =
                            report(&solution);
                        let quality = if layout == LayoutKind::Guillotine {
                            format!("{not_cuttable}/{missing_guide}")
                        } else {
                            "-".to_string()
                        };
                        println!(
                            "{:<18} {:<10} {:<9} {:>3} {:>6} {:>6.1}% {:>7.1}% {:>9.1} {:>8}",
                            project.name,
                            format!("{layout:?}"),
                            format!("{effort:?}"),
                            lb,
                            sheets,
                            utilization * 100.0,
                            emptiest_fill * 100.0,
                            elapsed,
                            quality
                        );
                    }
                    Err(error) => println!(
                        "{:<18} {:<10} {:<9} {:>3} error: {error:?}",
                        project.name,
                        format!("{layout:?}"),
                        format!("{effort:?}"),
                        lb
                    ),
                }
            }
        }
    }
}
