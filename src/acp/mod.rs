pub mod permissions;
pub mod reports;
pub mod tooling;
pub mod workspace;
mod zed;

pub use vtcode_acp_client::{acp_connection, register_acp_connection};
pub use zed::ZedAcpAdapter;
