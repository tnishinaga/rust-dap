#[cfg(feature = "rp2040")]
mod rp2040;
#[cfg(feature = "rp2350")]
mod rp2350;

#[cfg(feature = "rp2040")]
pub use rp2040::*;
#[cfg(feature = "rp2350")]
pub use rp2350::*;
