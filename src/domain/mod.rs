//! Core project model for Freecut.
//!
//! This layer should stay independent from GUI toolkit, PDF backend, and concrete optimizer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub stock_pieces: Vec<StockPiece>,
    pub cut_pieces: Vec<CutPiece>,
    pub settings: CutSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    Millimeter,
    Inch,
    Foot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternDirection {
    None,
    ParallelToWidth,
    ParallelToLength,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockPiece {
    pub id: PieceId,
    /// Width in `measurement` geometry ticks.
    pub width: u32,
    /// Length in `measurement` geometry ticks.
    pub length: u32,
    pub quantity: Option<u32>,
    pub pattern: PatternDirection,
    /// Whether this piece takes part in optimization.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Width in `measurement` geometry ticks subtracted from `width` to get the usable width.
    #[serde(default)]
    pub offset_width: u32,
    /// Length in `measurement` geometry ticks subtracted from `length` to get the usable length.
    #[serde(default)]
    pub offset_length: u32,
    /// Whether every sheet of this piece must receive at least one cut.
    #[serde(default)]
    pub required: bool,
}

impl StockPiece {
    /// Usable width after subtracting the width offset.
    #[must_use]
    pub fn usable_width(&self) -> u32 {
        self.width.saturating_sub(self.offset_width)
    }

    /// Usable length after subtracting the length offset.
    #[must_use]
    pub fn usable_length(&self) -> u32 {
        self.length.saturating_sub(self.offset_length)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CutPiece {
    pub id: PieceId,
    pub label: String,
    /// Width in `measurement` geometry ticks.
    pub width: u32,
    /// Length in `measurement` geometry ticks.
    pub length: u32,
    pub quantity: u32,
    pub pattern: PatternDirection,
    pub can_rotate: bool,
    /// Stock piece this cut must be placed on; `None` lets the optimizer choose.
    #[serde(default)]
    pub assigned_stock: Option<PieceId>,
    /// Whether this piece takes part in optimization.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PieceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CutSettings {
    /// Unit used to parse and display project measurements.
    pub unit: Unit,
    /// Kerf width in `measurement` geometry ticks.
    pub kerf_width: u32,
    pub layout: LayoutKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutKind {
    Guillotine,
    Nested,
}
