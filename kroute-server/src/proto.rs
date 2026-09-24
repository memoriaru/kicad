pub mod pb {
    tonic::include_proto!("kroute.v1");
}

pub use pb::k_route_client::KRouteClient;
pub use pb::k_route_server::{KRoute, KRouteServer};
pub use pb::*;
