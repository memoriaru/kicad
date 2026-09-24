#[allow(clippy::result_large_err)] // tonic 生成的 Status 在 ARM 上超 clippy 阈值
pub mod pb {
    tonic::include_proto!("kroute.v1");
}

pub use pb::k_route_client::KRouteClient;
pub use pb::k_route_server::{KRoute, KRouteServer};
pub use pb::*;
