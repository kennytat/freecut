//! Quality search on top of the population pipeline.
//!
//! Two stages run after the genetic pipeline:
//!
//! 1. Diversified construction builds first-fit layouts for several cut orders, every packing
//!    heuristic, two bin-choice rules and (for mixed stock) two stock orders.
//! 2. Bin-subset repacking repeatedly takes the emptiest sheet plus a few others, repacks their
//!    cuts from scratch with a randomized order and heuristic, and keeps the result when it
//!    ranks better. This is the move that eliminates sheets.
//!
//! Candidates are ranked by fewest sheets, then least used stock area, then how strongly waste
//! is concentrated on few sheets (sum of squared sheet fill). Concentrated waste leaves reusable
//! offcuts and gives the repacking a nearly empty sheet to eliminate.

use std::cmp::Ordering;

use super::{
    compare_cut_instances_for_first_fit, compare_stock_instances_for_first_fit, Candidate,
    CutCatalog, CutInstance, OptimizerEffort, PackingBackend, ProblemInstance, StockInstance,
    StockInventory, TinyPrng,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SearchConfig {
    diversified_construction: bool,
    repack_iterations: usize,
    /// Stop repacking after this many consecutive iterations without an improvement.
    stagnation_limit: usize,
    /// Restarts of the repair walk when no construction places every cut.
    repair_rounds: usize,
    seed: u64,
}

/// Repair walk length per round before restarting from the initial candidate.
const REPAIR_ROUND_ITERATIONS: usize = 2_500;

impl SearchConfig {
    pub(super) const fn from_effort(effort: OptimizerEffort) -> Self {
        let (repack_iterations, stagnation_limit, repair_rounds) = match effort {
            OptimizerEffort::Fast => (0, 0, 2),
            OptimizerEffort::Balanced => (400, 150, 8),
            OptimizerEffort::Thorough => (4_000, 600, 16),
        };

        Self {
            diversified_construction: true,
            repack_iterations,
            stagnation_limit,
            repair_rounds,
            seed: 0x5EED,
        }
    }

    #[cfg(test)]
    pub(super) const fn disabled() -> Self {
        Self {
            diversified_construction: false,
            repack_iterations: 0,
            stagnation_limit: 0,
            repair_rounds: 0,
            seed: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CutOrder {
    WidthThenLength,
    Area,
    LongestSide,
    LengthThenWidth,
    Perimeter,
}

const CUT_ORDERS: [CutOrder; 5] = [
    CutOrder::WidthThenLength,
    CutOrder::Area,
    CutOrder::LongestSide,
    CutOrder::LengthThenWidth,
    CutOrder::Perimeter,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinRule {
    /// Try open sheets in the order they were opened.
    FirstFit,
    /// Try the fullest open sheet first, keeping emptier sheets free for later pieces.
    FullestFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StockOrder {
    LargestFirst,
    SmallestFirst,
}

#[derive(Debug, Clone, Copy)]
struct Recipe<H> {
    order: CutOrder,
    heuristic: H,
    bin_rule: BinRule,
    stock_order: StockOrder,
}

/// Returns the best valid candidate from `population` after the configured quality search.
pub(super) fn best_after_search<B>(
    population: Vec<Candidate<B>>,
    instance: &ProblemInstance,
    config: SearchConfig,
) -> Option<Candidate<B>>
where
    B: PackingBackend,
{
    if !config.diversified_construction && config.repack_iterations == 0 {
        return super::select_best_valid_candidate(population);
    }

    let constructed = if config.diversified_construction {
        diversified_candidates::<B>(instance)
    } else {
        Vec::new()
    };

    // Start from the least infeasible candidate when no construction places every cut: tight
    // instances (many required sheets, high fill) often miss by a cut or two, and repacking can
    // still find room for them.
    let best = population
        .into_iter()
        .chain(constructed)
        .filter(Candidate::is_consistent)
        .reduce(|best, candidate| {
            if compare_quality(&candidate, &best) == Ordering::Greater {
                candidate
            } else {
                best
            }
        })?;

    let best = if best.is_valid() {
        best
    } else {
        repair(&best, instance, config)?
    };

    Some(repack_search(best, instance, config))
}

/// Walks from an infeasible candidate towards one that places every cut. Repacks that keep the
/// candidate equally infeasible are accepted too, so the walk can cross the plateau of layouts
/// that miss the same cuts; each round restarts from `start` with a fresh seed.
fn repair<B>(
    start: &Candidate<B>,
    instance: &ProblemInstance,
    config: SearchConfig,
) -> Option<Candidate<B>>
where
    B: PackingBackend,
{
    if start.bins.len() < 2 {
        return None;
    }

    let heuristics = B::heuristic_variants();
    let mixed_stock = has_mixed_stock_sizes(&instance.stock);

    for round in 0..config.repair_rounds {
        let mut prng = TinyPrng::new(config.seed.wrapping_add(round as u64));
        let mut current = start.clone_candidate();

        for _ in 0..REPAIR_ROUND_ITERATIONS {
            let subset = choose_subset(&current, &mut prng);
            let recipe = random_recipe(&heuristics, mixed_stock, &mut prng);
            let Some(candidate) = repack_subset(&current, &subset, instance, &recipe, &mut prng)
            else {
                continue;
            };
            if candidate.infeasibility() <= current.infeasibility() {
                current = candidate;
                if current.is_valid() {
                    return Some(current);
                }
            }
        }
    }

    None
}

fn diversified_candidates<B>(instance: &ProblemInstance) -> Vec<Candidate<B>>
where
    B: PackingBackend,
{
    let stock_orders = if has_mixed_stock_sizes(&instance.stock) {
        vec![StockOrder::LargestFirst, StockOrder::SmallestFirst]
    } else {
        vec![StockOrder::LargestFirst]
    };

    let mut candidates = Vec::new();
    for order in CUT_ORDERS {
        let cuts = ordered_cuts(&instance.cuts, order);
        for heuristic in B::heuristic_variants() {
            for bin_rule in [BinRule::FirstFit, BinRule::FullestFirst] {
                for &stock_order in &stock_orders {
                    let recipe = Recipe {
                        order,
                        heuristic,
                        bin_rule,
                        stock_order,
                    };
                    candidates.push(construct::<B>(
                        &cuts,
                        instance.stock.clone(),
                        CutCatalog::new(instance.cuts.clone()),
                        instance.kerf_width,
                        &recipe,
                    ));
                }
            }
        }
    }

    candidates
}

fn has_mixed_stock_sizes(stock: &[StockInstance]) -> bool {
    stock
        .windows(2)
        .any(|pair| (pair[0].width, pair[0].length) != (pair[1].width, pair[1].length))
}

fn ordered_cuts(cuts: &[CutInstance], order: CutOrder) -> Vec<CutInstance> {
    let mut cuts = cuts.to_vec();
    cuts.sort_by(|left, right| {
        let key = |cut: &CutInstance| -> (u64, u64) {
            let width = u64::from(cut.width);
            let length = u64::from(cut.length);
            match order {
                CutOrder::WidthThenLength => (width, length),
                CutOrder::Area => (width * length, width.max(length)),
                CutOrder::LongestSide => (width.max(length), width.min(length)),
                CutOrder::LengthThenWidth => (length, width),
                CutOrder::Perimeter => (width + length, width.max(length)),
            }
        };
        key(right)
            .cmp(&key(left))
            .then_with(|| compare_cut_instances_for_first_fit(left, right))
    });
    cuts
}

fn construct<B>(
    cuts: &[CutInstance],
    mut stock: Vec<StockInstance>,
    cut_catalog: CutCatalog,
    kerf_width: u32,
    recipe: &Recipe<B::Heuristic>,
) -> Candidate<B>
where
    B: PackingBackend,
{
    match recipe.stock_order {
        StockOrder::LargestFirst => stock.sort_by(compare_stock_instances_for_first_fit),
        StockOrder::SmallestFirst => stock.sort_by(|left, right| {
            stock_area(left)
                .cmp(&stock_area(right))
                .then_with(|| compare_stock_instances_for_first_fit(left, right))
        }),
    }

    let mut candidate = Candidate::new(StockInventory::new(stock), cut_catalog);
    candidate.open_required_bins(kerf_width);
    for cut in cuts {
        match recipe.bin_rule {
            BinRule::FirstFit => {
                candidate.place_cut_first_fit(cut, kerf_width, recipe.heuristic);
            }
            BinRule::FullestFirst => {
                candidate.place_cut_fullest_first(cut, kerf_width, recipe.heuristic);
            }
        }
    }
    candidate
}

fn stock_area(stock: &StockInstance) -> u64 {
    u64::from(stock.width) * u64::from(stock.length)
}

impl<B> Candidate<B>
where
    B: PackingBackend,
{
    fn place_cut_fullest_first(
        &mut self,
        cut: &CutInstance,
        kerf_width: u32,
        heuristic: B::Heuristic,
    ) -> bool {
        // Bins are tried fullest first; empty required sheets still get the first chance so
        // they always receive a cut.
        let mut order = (0..self.bins.len()).collect::<Vec<_>>();
        order.sort_by(|&left, &right| {
            let left_bin = &self.bins[left];
            let right_bin = &self.bins[right];
            B::is_unused_required(right_bin)
                .cmp(&B::is_unused_required(left_bin))
                .then_with(|| {
                    B::fitness(right_bin)
                        .partial_cmp(&B::fitness(left_bin))
                        .unwrap_or(Ordering::Equal)
                })
                .then_with(|| left.cmp(&right))
        });

        for index in order {
            if B::insert(&mut self.bins[index], cut, heuristic) {
                self.remove_unused_cut(cut.into());
                return true;
            }
        }

        if let Some(bin) = self
            .available_stock
            .take_first_fitting_stock::<B>(cut, kerf_width, heuristic)
        {
            self.bins.push(bin);
            self.remove_unused_cut(cut.into());
            return true;
        }

        self.unused_cuts.push(cut.clone());
        false
    }

    fn clone_candidate(&self) -> Self {
        Self {
            bins: self.bins.clone(),
            available_stock: self.available_stock.clone(),
            unused_cuts: self.unused_cuts.clone(),
            cut_catalog: self.cut_catalog.clone(),
        }
    }

    /// How far the candidate is from valid: unplaced cuts, their area, then empty required
    /// sheets. All zero for a valid candidate; lower is better.
    fn infeasibility(&self) -> (usize, u64, usize) {
        let empty_required = self
            .available_stock
            .stock
            .iter()
            .filter(|stock| stock.required)
            .count()
            + self
                .bins
                .iter()
                .filter(|bin| B::is_unused_required(bin))
                .count();
        (
            self.unused_cuts.len(),
            super::total_cut_area(&self.unused_cuts),
            empty_required,
        )
    }

    /// Sum of squared sheet fill: higher when waste is concentrated on fewer sheets.
    fn fill_concentration(&self) -> f64 {
        self.bins
            .iter()
            .map(|bin| {
                let fill = B::fitness(bin);
                fill * fill
            })
            .sum()
    }
}

/// `Greater` means `left` is the better candidate. Any feasible candidate beats any infeasible one.
fn compare_quality<B>(left: &Candidate<B>, right: &Candidate<B>) -> Ordering
where
    B: PackingBackend,
{
    right
        .infeasibility()
        .cmp(&left.infeasibility())
        .then_with(|| right.bins.len().cmp(&left.bins.len()))
        .then_with(|| right.used_stock_area().cmp(&left.used_stock_area()))
        .then_with(|| {
            left.fill_concentration()
                .partial_cmp(&right.fill_concentration())
                .unwrap_or(Ordering::Equal)
        })
}

fn repack_search<B>(
    mut best: Candidate<B>,
    instance: &ProblemInstance,
    config: SearchConfig,
) -> Candidate<B>
where
    B: PackingBackend,
{
    let heuristics = B::heuristic_variants();
    let mixed_stock = has_mixed_stock_sizes(&instance.stock);
    let mut prng = TinyPrng::new(config.seed);
    let mut since_improvement = 0;

    for _ in 0..config.repack_iterations {
        if best.bins.len() < 2 || since_improvement >= config.stagnation_limit {
            break;
        }
        since_improvement += 1;

        let subset = choose_subset(&best, &mut prng);
        let recipe = random_recipe(&heuristics, mixed_stock, &mut prng);

        if let Some(candidate) = repack_subset(&best, &subset, instance, &recipe, &mut prng) {
            if compare_quality(&candidate, &best) == Ordering::Greater {
                best = candidate;
                since_improvement = 0;
            }
        }
    }

    best
}

fn random_recipe<H: Copy>(heuristics: &[H], mixed_stock: bool, prng: &mut TinyPrng) -> Recipe<H> {
    Recipe {
        order: CUT_ORDERS[prng.next_index(CUT_ORDERS.len())],
        heuristic: heuristics[prng.next_index(heuristics.len())],
        bin_rule: if prng.next_index(2) == 0 {
            BinRule::FirstFit
        } else {
            BinRule::FullestFirst
        },
        stock_order: if mixed_stock && prng.next_index(2) == 1 {
            StockOrder::SmallestFirst
        } else {
            StockOrder::LargestFirst
        },
    }
}

/// Picks one of the emptiest sheets plus one to three others. While cuts are unplaced, the first
/// pick is instead a sheet the largest unplaced cut fits on, so the repack can make room for it.
fn choose_subset<B>(candidate: &Candidate<B>, prng: &mut TinyPrng) -> Vec<usize>
where
    B: PackingBackend,
{
    if let Some(cut) = candidate
        .unused_cuts
        .iter()
        .max_by_key(|cut| u64::from(cut.width) * u64::from(cut.length))
    {
        let mut hosts = Vec::new();
        let mut others = Vec::new();
        for index in 0..candidate.bins.len() {
            let stock = B::clone_stock_instance(&candidate.bins[index]);
            if super::cut_fits_stock(cut, &stock) {
                hosts.push(index);
            } else {
                others.push(index);
            }
        }
        if !hosts.is_empty() {
            let mut subset = vec![hosts.remove(prng.next_index(hosts.len()))];
            others.extend(hosts);
            let extra = 1 + prng.next_index(others.len().min(3));
            for _ in 0..extra.min(others.len()) {
                subset.push(others.remove(prng.next_index(others.len())));
            }
            return subset;
        }
    }

    let mut by_fill = (0..candidate.bins.len()).collect::<Vec<_>>();
    by_fill.sort_by(|&left, &right| {
        B::fitness(&candidate.bins[left])
            .partial_cmp(&B::fitness(&candidate.bins[right]))
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.cmp(&right))
    });

    let emptiest_pick = prng.next_index(by_fill.len().min(3));
    let mut subset = vec![by_fill.remove(emptiest_pick)];
    let extra = 1 + prng.next_index(by_fill.len().min(3));
    for _ in 0..extra {
        subset.push(by_fill.remove(prng.next_index(by_fill.len())));
    }

    subset
}

fn repack_subset<B>(
    candidate: &Candidate<B>,
    subset: &[usize],
    instance: &ProblemInstance,
    recipe: &Recipe<B::Heuristic>,
    prng: &mut TinyPrng,
) -> Option<Candidate<B>>
where
    B: PackingBackend,
{
    let keys = subset
        .iter()
        .flat_map(|&index| B::placed_cut_keys(&candidate.bins[index]))
        .collect::<Vec<_>>();
    // Unplaced cuts join every repack so an infeasible candidate can work them in.
    let mut cuts = candidate.cuts_for_keys(&keys)?;
    cuts.extend(candidate.unused_cuts.iter().cloned());
    let mut cuts = ordered_cuts(&cuts, recipe.order);
    perturb(&mut cuts, prng);

    let mut stock = candidate.available_stock.stock.clone();
    stock.extend(
        subset
            .iter()
            .map(|&index| B::clone_stock_instance(&candidate.bins[index])),
    );

    let repacked = construct::<B>(
        &cuts,
        stock,
        candidate.cut_catalog.clone(),
        instance.kerf_width,
        recipe,
    );
    if repacked.unused_cuts.len() > candidate.unused_cuts.len() {
        return None;
    }

    let mut result = candidate.clone_candidate();
    let mut removed = subset.to_vec();
    removed.sort_unstable_by(|left, right| right.cmp(left));
    for index in removed {
        result.bins.remove(index);
    }
    result.bins.extend(repacked.bins);
    result.available_stock = repacked.available_stock;
    result.unused_cuts = repacked.unused_cuts;

    result.is_consistent().then_some(result)
}

/// Swaps a few neighbouring cuts so repeated repacks explore orders near the sorted one.
fn perturb(cuts: &mut [CutInstance], prng: &mut TinyPrng) {
    if cuts.len() < 2 {
        return;
    }

    let swaps = prng.next_index(cuts.len() / 3 + 1);
    for _ in 0..swaps {
        let index = prng.next_index(cuts.len() - 1);
        let distance = 1 + prng.next_index(3.min(cuts.len() - 1 - index));
        cuts.swap(index, index + distance);
    }
}
