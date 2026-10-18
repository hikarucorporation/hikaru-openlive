// crates/hikaru_transport/src/lib.rs

pub mod transport_state;

// Esta línea es la que permite que el secuenciador lo encuentre fácilmente
pub use transport_state::{TransportPosition, TransportPlaybackState, DEFAULT_PPQN};