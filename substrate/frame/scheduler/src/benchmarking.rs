// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Scheduler pallet benchmarking.

use alloc::vec;
use frame_benchmarking::v2::*;
use frame_support::{
	ensure,
	traits::{schedule::Priority, BoundedInline},
	weights::WeightMeter,
};
use frame_system::{EventRecord, RawOrigin};

use crate::*;

type SystemCall<T> = frame_system::Call<T>;
type SystemOrigin<T> = <T as frame_system::Config>::RuntimeOrigin;

const SEED: u32 = 0;
const BLOCK_NUMBER: u32 = 2;

fn assert_last_event<T: Config>(generic_event: <T as Config>::RuntimeEvent) {
	let events = frame_system::Pallet::<T>::events();
	let system_event: <T as frame_system::Config>::RuntimeEvent = generic_event.into();
	// compare to the last event record
	let EventRecord { event, .. } = &events[events.len() - 1];
	assert_eq!(event, &system_event);
}

/// Add `n` items to the schedule.
///
/// For `resolved`:
/// - `
/// - `None`: aborted (hash without preimage)
/// - `Some(true)`: hash resolves into call if possible, plain call otherwise
/// - `Some(false)`: plain call
fn fill_schedule<T: Config>(when: BlockNumberFor<T>, n: u32) -> Result<(), &'static str> {
	let t = DispatchTime::At(when);
	let origin: <T as Config>::PalletsOrigin = frame_system::RawOrigin::Root.into();
	for i in 0..n {
		let call = make_call::<T>(None);
		let period = Some(((i + 100).into(), 100));
		let name = u32_to_name(i);
		Pallet::<T>::do_schedule_named(name, t, period, 0, origin.clone(), call)?;
	}
	ensure!(Agenda::<T>::get(when).len() == n as usize, "didn't fill schedule");
	Ok(())
}

fn u32_to_name(i: u32) -> TaskName {
	i.using_encoded(blake2_256)
}

fn make_task<T: Config>(
	periodic: bool,
	named: bool,
	signed: bool,
	maybe_lookup_len: Option<u32>,
	priority: Priority,
) -> ScheduledOf<T> {
	let call = make_call::<T>(maybe_lookup_len);
	let maybe_periodic = match periodic {
		true => Some((100u32.into(), 100)),
		false => None,
	};
	let maybe_id = match named {
		true => Some(u32_to_name(0)),
		false => None,
	};
	let origin = make_origin::<T>(signed);
	Scheduled { maybe_id, priority, call, maybe_periodic, origin, _phantom: PhantomData }
}

fn bounded<T: Config>(len: u32) -> Option<BoundedCallOf<T>> {
	let call =
		<<T as Config>::RuntimeCall>::from(SystemCall::remark { remark: vec![0; len as usize] });
	T::Preimages::bound(call).ok()
}

fn make_call<T: Config>(maybe_lookup_len: Option<u32>) -> BoundedCallOf<T> {
	let bound = BoundedInline::bound() as u32;
	let mut len = match maybe_lookup_len {
		Some(len) => len.min(T::Preimages::MAX_LENGTH as u32 - 2).max(bound) - 3,
		None => bound.saturating_sub(4),
	};

	loop {
		let c = match bounded::<T>(len) {
			Some(x) => x,
			None => {
				len -= 1;
				continue
			},
		};
		if c.lookup_needed() == maybe_lookup_len.is_some() {
			break c
		}
		if maybe_lookup_len.is_some() {
			len += 1;
		} else {
			if len > 0 {
				len -= 1;
			} else {
				break c
			}
		}
	}
}

fn make_origin<T: Config>(signed: bool) -> <T as Config>::PalletsOrigin {
	match signed {
		true => frame_system::RawOrigin::Signed(account("origin", 0, SEED)).into(),
		false => frame_system::RawOrigin::Root.into(),
	}
}

#[benchmarks]
mod benchmarks {
	use super::*;

	// `service_agendas` when no work is done.
	#[benchmark]
	fn service_agendas_base() {
		let now = BLOCK_NUMBER.into();
		IncompleteSince::<T>::put(now - One::one());

		#[block]
		{
			Pallet::<T>::service_agendas(&mut WeightMeter::new(), now, 0);
		}

		assert_eq!(IncompleteSince::<T>::get(), Some(now - One::one()));
	}

	// `service_agenda` when no work is done.
	#[benchmark]
	fn service_agenda_base(
		s: Linear<0, { T::MaxScheduledPerBlock::get() }>,
	) -> Result<(), BenchmarkError> {
		let now = BLOCK_NUMBER.into();
		fill_schedule::<T>(now, s)?;
		assert_eq!(Agenda::<T>::get(now).len() as u32, s);

		#[block]
		{
			Pallet::<T>::service_agenda(&mut WeightMeter::new(), true, now, now, 0);
		}

		assert_eq!(Agenda::<T>::get(now).len() as u32, s);

		Ok(())
	}

	// `service_task` when the task is a non-periodic, non-named, non-fetched call which is not
	// dispatched (e.g. due to being overweight).
	#[benchmark]
	fn service_task_base() {
		let now = BLOCK_NUMBER.into();
		let task = make_task::<T>(false, false, false, None, 0);
		// prevent any tasks from actually being executed as we only want the surrounding weight.
		let mut counter = WeightMeter::with_limit(Weight::zero());
		let _result;

		#[block]
		{
			_result = Pallet::<T>::service_task(&mut counter, now, now, 0, true, task);
		}

		// assert!(_result.is_ok());
	}

	// `service_task` when the task is a non-periodic, non-named, fetched call (with a known
	// preimage length) and which is not dispatched (e.g. due to being overweight).
	#[benchmark(pov_mode = MaxEncodedLen {
		// Use measured PoV size for the Preimages since we pass in a length witness.
		Preimage::PreimageFor: Measured
	})]
	fn service_task_fetched(
		s: Linear<{ BoundedInline::bound() as u32 }, { T::Preimages::MAX_LENGTH as u32 }>,
	) {
		let now = BLOCK_NUMBER.into();
		let task = make_task::<T>(false, false, false, Some(s), 0);
		// prevent any tasks from actually being executed as we only want the surrounding weight.
		let mut counter = WeightMeter::with_limit(Weight::zero());
		let _result;

		#[block]
		{
			_result = Pallet::<T>::service_task(&mut counter, now, now, 0, true, task);
		}

		// assert!(result.is_ok());
	}

	// `service_task` when the task is a non-periodic, named, non-fetched call which is not
	// dispatched (e.g. due to being overweight).
	#[benchmark]
	fn service_task_named() {
		let now = BLOCK_NUMBER.into();
		let task = make_task::<T>(false, true, false, None, 0);
		// prevent any tasks from actually being executed as we only want the surrounding weight.
		let mut counter = WeightMeter::with_limit(Weight::zero());
		let _result;

		#[block]
		{
			_result = Pallet::<T>::service_task(&mut counter, now, now, 0, true, task);
		}

		// assert!(result.is_ok());
	}

	// `service_task` when the task is a periodic, non-named, non-fetched call which is not
	// dispatched (e.g. due to being overweight).
	#[benchmark]
	fn service_task_periodic() {
		let now = BLOCK_NUMBER.into();
		let task = make_task::<T>(true, false, false, None, 0);
		// prevent any tasks from actually being executed as we only want the surrounding weight.
		let mut counter = WeightMeter::with_limit(Weight::zero());
		let _result;

		#[block]
		{
			_result = Pallet::<T>::service_task(&mut counter, now, now, 0, true, task);
		}

		// assert!(result.is_ok());
	}

	// `execute_dispatch` when the origin is `Signed`, not counting the dispatchable's weight.
	#[benchmark]
	fn execute_dispatch_signed() -> Result<(), BenchmarkError> {
		let mut counter = WeightMeter::new();
		let origin = make_origin::<T>(true);
		let call = T::Preimages::realize(&make_call::<T>(None))?.0;
		let result;

		#[block]
		{
			result = Pallet::<T>::execute_dispatch(&mut counter, origin, call);
		}

		assert!(result.is_ok());

		Ok(())
	}

	// `execute_dispatch` when the origin is not `Signed`, not counting the dispatchable's weight.
	#[benchmark]
	fn execute_dispatch_unsigned() -> Result<(), BenchmarkError> {
		let mut counter = WeightMeter::new();
		let origin = make_origin::<T>(false);
		let call = T::Preimages::realize(&make_call::<T>(None))?.0;
		let result;

		#[block]
		{
			result = Pallet::<T>::execute_dispatch(&mut counter, origin, call);
		}

		assert!(result.is_ok());

		Ok(())
	}

	#[benchmark]
	fn schedule(
		s: Linear<0, { T::MaxScheduledPerBlock::get() - 1 }>,
	) -> Result<(), BenchmarkError> {
		let when = BLOCK_NUMBER.into();
		let periodic = Some((BlockNumberFor::<T>::one(), 100));
		let priority = 0;
		// Essentially a no-op call.
		let call = Box::new(SystemCall::set_storage { items: vec![] }.into());

		fill_schedule::<T>(when, s)?;

		#[extrinsic_call]
		_(RawOrigin::Root, when, periodic, priority, call);

		ensure!(Agenda::<T>::get(when).len() == s as usize + 1, "didn't add to schedule");

		Ok(())
	}

	#[benchmark]
	fn cancel(s: Linear<1, { T::MaxScheduledPerBlock::get() }>) -> Result<(), BenchmarkError> {
		let when = BLOCK_NUMBER.into();

		fill_schedule::<T>(when, s)?;
		assert_eq!(Agenda::<T>::get(when).len(), s as usize);
		let schedule_origin =
			T::ScheduleOrigin::try_successful_origin().map_err(|_| BenchmarkError::Weightless)?;

		#[extrinsic_call]
		_(schedule_origin as SystemOrigin<T>, when, 0);

		ensure!(
			s == 1 || Lookup::<T>::get(u32_to_name(0)).is_none(),
			"didn't remove from lookup if more than 1 task scheduled for `when`"
		);
		// Removed schedule is NONE
		ensure!(
			s == 1 || Agenda::<T>::get(when)[0].is_none(),
			"didn't remove from schedule if more than 1 task scheduled for `when`"
		);
		ensure!(
			s > 1 || Agenda::<T>::get(when).len() == 0,
			"remove from schedule if only 1 task scheduled for `when`"
		);

		Ok(())
	}

	#[benchmark]
	fn schedule_named(
		s: Linear<0, { T::MaxScheduledPerBlock::get() - 1 }>,
	) -> Result<(), BenchmarkError> {
		let id = u32_to_name(s);
		let when = BLOCK_NUMBER.into();
		let periodic = Some((BlockNumberFor::<T>::one(), 100));
		let priority = 0;
		// Essentially a no-op call.
		let call = Box::new(SystemCall::set_storage { items: vec![] }.into());

		fill_schedule::<T>(when, s)?;

		#[extrinsic_call]
		_(RawOrigin::Root, id, when, periodic, priority, call);

		ensure!(Agenda::<T>::get(when).len() == s as usize + 1, "didn't add to schedule");

		Ok(())
	}

	#[benchmark]
	fn cancel_named(
		s: Linear<1, { T::MaxScheduledPerBlock::get() }>,
	) -> Result<(), BenchmarkError> {
		let when = BLOCK_NUMBER.into();

		fill_schedule::<T>(when, s)?;

		#[extrinsic_call]
		_(RawOrigin::Root, u32_to_name(0));

		ensure!(
			s == 1 || Lookup::<T>::get(u32_to_name(0)).is_none(),
			"didn't remove from lookup if more than 1 task scheduled for `when`"
		);
		// Removed schedule is NONE
		ensure!(
			s == 1 || Agenda::<T>::get(when)[0].is_none(),
			"didn't remove from schedule if more than 1 task scheduled for `when`"
		);
		ensure!(
			s > 1 || Agenda::<T>::get(when).len() == 0,
			"remove from schedule if only 1 task scheduled for `when`"
		);

		Ok(())
	}

	#[benchmark]
	fn schedule_retry(
		s: Linear<1, { T::MaxScheduledPerBlock::get() }>,
	) -> Result<(), BenchmarkError> {
		let when = BLOCK_NUMBER.into();

		fill_schedule::<T>(when, s)?;
		let name = u32_to_name(s - 1);
		let address = Lookup::<T>::get(name).unwrap();
		let period: BlockNumberFor<T> = 1_u32.into();
		let retry_config = RetryConfig { total_retries: 10, remaining: 10, period };
		Retries::<T>::insert(address, retry_config);
		let (mut when, index) = address;
		let task = Agenda::<T>::get(when)[index as usize].clone().unwrap();
		let mut weight_counter = WeightMeter::with_limit(T::MaximumWeight::get());

		#[block]
		{
			Pallet::<T>::schedule_retry(
				&mut weight_counter,
				when,
				when,
				index,
				&task,
				retry_config,
			);
		}

		when = when + BlockNumberFor::<T>::one();
		assert_eq!(
			Retries::<T>::get((when, 0)),
			Some(RetryConfig { total_retries: 10, remaining: 9, period })
		);

		Ok(())
	}

	#[benchmark]
	fn set_retry() -> Result<(), BenchmarkError> {
		let s = T::MaxScheduledPerBlock::get();
		let when = BLOCK_NUMBER.into();

		fill_schedule::<T>(when, s)?;
		let name = u32_to_name(s - 1);
		let address = Lookup::<T>::get(name).unwrap();
		let (when, index) = address;
		let period = BlockNumberFor::<T>::one();

		#[extrinsic_call]
		_(RawOrigin::Root, (when, index), 10, period);

		assert_eq!(
			Retries::<T>::get((when, index)),
			Some(RetryConfig { total_retries: 10, remaining: 10, period })
		);
		assert_last_event::<T>(
			Event::RetrySet { task: address, id: None, period, retries: 10 }.into(),
		);

		Ok(())
	}

	#[benchmark]
	fn set_retry_named() -> Result<(), BenchmarkError> {
		let s = T::MaxScheduledPerBlock::get();
		let when = BLOCK_NUMBER.into();

		fill_schedule::<T>(when, s)?;
		let name = u32_to_name(s - 1);
		let address = Lookup::<T>::get(name).unwrap();
		let (when, index) = address;
		let period = BlockNumberFor::<T>::one();

		#[extrinsic_call]
		_(RawOrigin::Root, name, 10, period);

		assert_eq!(
			Retries::<T>::get((when, index)),
			Some(RetryConfig { total_retries: 10, remaining: 10, period })
		);
		assert_last_event::<T>(
			Event::RetrySet { task: address, id: Some(name), period, retries: 10 }.into(),
		);

		Ok(())
	}

	#[benchmark]
	fn cancel_retry() -> Result<(), BenchmarkError> {
		let s = T::MaxScheduledPerBlock::get();
		let when = BLOCK_NUMBER.into();

		fill_schedule::<T>(when, s)?;
		let name = u32_to_name(s - 1);
		let address = Lookup::<T>::get(name).unwrap();
		let (when, index) = address;
		let period = BlockNumberFor::<T>::one();
		assert!(Pallet::<T>::set_retry(RawOrigin::Root.into(), (when, index), 10, period).is_ok());

		#[extrinsic_call]
		_(RawOrigin::Root, (when, index));

		assert!(!Retries::<T>::contains_key((when, index)));
		assert_last_event::<T>(Event::RetryCancelled { task: address, id: None }.into());

		Ok(())
	}

	#[benchmark]
	fn cancel_retry_named() -> Result<(), BenchmarkError> {
		let s = T::MaxScheduledPerBlock::get();
		let when = BLOCK_NUMBER.into();

		fill_schedule::<T>(when, s)?;
		let name = u32_to_name(s - 1);
		let address = Lookup::<T>::get(name).unwrap();
		let (when, index) = address;
		let period = BlockNumberFor::<T>::one();
		assert!(Pallet::<T>::set_retry_named(RawOrigin::Root.into(), name, 10, period).is_ok());

		#[extrinsic_call]
		_(RawOrigin::Root, name);

		assert!(!Retries::<T>::contains_key((when, index)));
		assert_last_event::<T>(Event::RetryCancelled { task: address, id: Some(name) }.into());

		Ok(())
	}

	impl_benchmark_test_suite! {
		Pallet,
		mock::new_test_ext(),
		mock::Test
	}
}
