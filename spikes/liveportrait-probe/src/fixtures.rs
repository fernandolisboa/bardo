//! Test fixtures written by tools/fixtures.py from upstream's own code.

pub fn upstream() -> serde_json::Value {
    serde_json::from_str(include_str!("../tests/fixtures/upstream.json")).unwrap()
}

/// Every number of a (nested) JSON array, flattened.
pub fn floats(v: &serde_json::Value) -> Vec<f32> {
    match v {
        serde_json::Value::Array(items) => items.iter().flat_map(floats).collect(),
        n => vec![n.as_f64().unwrap() as f32],
    }
}

pub fn bytes(v: &serde_json::Value) -> Vec<u8> {
    floats(v).into_iter().map(|f| f as u8).collect()
}

pub fn close(got: f32, want: f32, tolerance: f32) {
    let scale = want.abs().max(1.0);
    assert!(
        (got - want).abs() <= tolerance * scale,
        "got {got}, want {want} (±{tolerance})"
    );
}
