pub mod permissions;
pub mod reports;
pub mod tooling;
mod tooling_provider;
pub mod workspace;
mod zed;

pub use vtcode_acp_client::{acp_connection, register_acp_connection};
pub use zed::{StandardAcpAdapter, ZedAcpAdapter};
