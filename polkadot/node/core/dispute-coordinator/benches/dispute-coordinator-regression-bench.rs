// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot.  If not, see <http://www.gnu.org/licenses/>.

//! dispute-coordinator throughput test
//!
//! Dispute Coordinator benchmark based on Kusama parameters and scale.
//!
//! Subsystems involved:
//! - dispute-coordinator
//! - dispute-distribution

use polkadot_subsystem_bench::{
	configuration::TestConfiguration,
	disputes::{benchmark_dispute_coordinator, prepare_test, DisputesOptions, TestState},
	usage::BenchmarkUsage,
	utils::save_to_file,
};
use std::io::Write;

const BENCH_COUNT: usize = 10;

fn main() -> Result<(), String> {
	let mut messages = vec![];
	let mut config = TestConfiguration::default();
	config.n_cores = 100;
	config.n_validators = 500;
	config.num_blocks = 10;
	config.peer_bandwidth = 524288000000;
	config.bandwidth = 524288000000;
	config.latency = None;
	config.connectivity = 100;
	config.generate_pov_sizes();
	let options = DisputesOptions { n_disputes: 50 };

	println!("Benchmarking...");
	let usages: Vec<BenchmarkUsage> = (0..BENCH_COUNT)
		.map(|n| {
			print!("\r[{}{}]", "#".repeat(n), "_".repeat(BENCH_COUNT - n));
			std::io::stdout().flush().unwrap();
			let state = TestState::new(&config, &options);
			let mut env = prepare_test(&state, false);
			env.runtime().block_on(benchmark_dispute_coordinator(&mut env, &state))
		})
		.collect();
	println!("\rDone!{}", " ".repeat(BENCH_COUNT));

	let average_usage = BenchmarkUsage::average(&usages);
	save_to_file(
		"charts/dispute-coordinator-regression-bench.json",
		average_usage.to_chart_json().map_err(|e| e.to_string())?,
	)
	.map_err(|e| e.to_string())?;
	println!("{}", average_usage);

	// We expect some small variance for received and sent because the
	// test messages are generated at every benchmark run and they contain
	// random data so use 0.01 as the accepted variance.
	messages.extend(average_usage.check_network_usage(&[
		("Received from peers", 23.8, 0.01),
		("Sent to peers", 227.1, 0.01),
	]));
	messages.extend(average_usage.check_cpu_usage(&[
		("dispute-coordinator", 0.0026, 0.1),
		("dispute-distribution", 0.0086, 0.1),
	]));

	if messages.is_empty() {
		Ok(())
	} else {
		eprintln!("{}", messages.join("\n"));
		Err("Regressions found".to_string())
	}
}
