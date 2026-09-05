//! Opaque ID generation. IDs are never parsed for type information.

use rand::Rng;

/// 128-bit random hex with a kind prefix. Wide enough that IDs generated
/// independently on multiple devices never collide (sync LWW keys on ID).
pub fn gen_id(prefix: &str) -> String {
    let mut rng = rand::thread_rng();
    let a: u32 = rng.gen();
    let b: u32 = rng.gen();
    let c: u32 = rng.gen();
    let d: u32 = rng.gen();
    format!("{prefix}-{a:08x}{b:08x}{c:08x}{d:08x}")
}

pub fn gen_device_id() -> String {
    let mut rng = rand::thread_rng();
    let a: u32 = rng.gen();
    let b: u32 = rng.gen();
    format!("dev-{a:08x}{b:08x}")
}

pub fn request_id() -> String {
    gen_id("req")
}

/// The literal `root` is reserved by the CLI and must never be generated as an ID.
pub fn is_reserved_id(id: &str) -> bool {
    id == "root"
}
