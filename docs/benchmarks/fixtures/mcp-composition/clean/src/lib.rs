//! Small clean-tree fixture for ProjectAtlas agent navigation.

pub mod api;
pub mod service;
pub mod states;
pub mod storage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub id: u64,
    pub quantity: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderRequest {
    pub id: u64,
    pub quantity: u32,
}
