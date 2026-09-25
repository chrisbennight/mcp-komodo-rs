//! Local catalog timing; compare the same build profile and environment.

use std::{hint::black_box, time::Instant};

use komodo_mcp::KomodoMcp;
use serde_json::json;

fn main() {
    let started = Instant::now();
    black_box(KomodoMcp::list_tools_payload());
    let cold_us = started.elapsed().as_micros();
    let mut samples = Vec::new();
    for _ in 0..1000 {
        let started = Instant::now();
        black_box(KomodoMcp::list_tools_payload());
        samples.push(started.elapsed().as_nanos());
    }
    samples.sort_unstable();
    println!(
        "{}",
        json!({"coldMicroseconds":cold_us,"warmMedianNanoseconds":samples[500],
        "warmP95Nanoseconds":samples[950], "samples":samples.len(),
        "scope":"in-process catalog construction/cloning; excludes serialization and network"})
    );
}
