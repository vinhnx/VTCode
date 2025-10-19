#[cfg(test)]
pub mod permissions;
#[cfg(not(test))]
mod permissions;

#[cfg(test)]
pub mod reports;
#[cfg(not(test))]
mod reports;

#[cfg(test)]
pub mod tooling;
#[cfg(not(test))]
mod tooling;

#[cfg(test)]
pub mod workspace;
#[cfg(not(test))]
mod workspace;
mod zed;

pub use vtcode_acp_client::{acp_client, register_acp_client};
pub use zed::ZedAcpAdapter;
