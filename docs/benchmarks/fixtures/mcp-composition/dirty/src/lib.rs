//! Dirty-tree fixture for ProjectAtlas current-source navigation.

pub mod checkout;
pub mod pricing;
pub mod states;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineItem {
    pub unit_cents: u64,
    pub quantity: u32,
}
