#[allow(dead_code)]
#[path = "build_identity.rs"]
mod identity;

include!(concat!(env!("OUT_DIR"), "/build_identity.rs"));

pub fn label() -> String {
    format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        identity::source_label(COMMIT, DIRTY)
    )
}

pub fn info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "commit": COMMIT,
        "dirty": DIRTY,
    })
}
