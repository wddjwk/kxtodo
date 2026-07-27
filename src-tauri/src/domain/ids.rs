//! Opaque ID generation. IDs are never parsed for type information.

use rand::Rng;

pub fn gen_id(prefix: &str) -> String {
    let mut rng = rand::thread_rng();
    let value: u32 = rng.gen();
    format!("{prefix}-{value:08x}")
}

pub fn request_id() -> String {
    let mut rng = rand::thread_rng();
    let a: u32 = rng.gen();
    let b: u32 = rng.gen();
    format!("req-{a:08x}{b:08x}")
}

/// The literal `root` is reserved by the CLI and must never be generated as an ID.
pub fn is_reserved_id(id: &str) -> bool {
    id == "root"
}
