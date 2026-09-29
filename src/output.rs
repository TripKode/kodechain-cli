//! Human output: tables (tabled) or raw JSON (--json global flag).

use tabled::{Table, Tabled};

#[derive(Tabled)]
pub struct Row {
    pub field: String,
    pub value: String,
}

pub fn kv(pairs: Vec<(String, String)>) {
    let rows: Vec<Row> = pairs
        .into_iter()
        .map(|(field, value)| Row { field, value })
        .collect();
    println!("{}", Table::new(rows));
}

pub fn json(v: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
    );
}

pub fn ok(msg: &str) {
    println!("✅ {msg}");
}

pub fn info(msg: &str) {
    println!("{msg}");
}
