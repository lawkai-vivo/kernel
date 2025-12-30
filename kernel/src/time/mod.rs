// Copyright (c) 2025 vivo Mobile Communication Co., Ltd.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//       http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// SysTick originally refers to the system timer of the Cortex-M
// platform. We extend its definition to the timer of every platform
// the BlueKernel supports. We use the term `cycles` to refer to the
// internal counter of the timer. We use the term `hz` to descripe
// how many cycles within a second. Tick is defined as a short period
// of time, which is atomic in the system, just like the Planck time
// in physical world. We use `TICKS_PER_SECOND` to measure it. A
// derived value, cycles_per_tick = hz / TICKS_PER_SECOND.

use crate::{arch, support::DisableInterruptGuard, sync::SpinLock};
use core::time::Duration;

// Currently in SMP system, all cores should be referencing the
// counter of the CPU#0.
static mut TICKS: u64 = 0;

pub trait SysTick {
    // Reading the current counter of the timer requires some time(Time
    // Drifting), we can only estimate it.
    fn estimate_current_cycles() -> u64;
    // Deliever a timer interrupt at the specified counter.
    // To support it, mps2 should use SP804 Dual-Timer, mps3 should
    // use Generic Timer.
    fn expire_at(moment: u64);
    const fn hz() -> u64;
}

pub(crate) extern "C" fn increment_system_ticks() {
    if arch::current_cpu_id() != 0 {
        return;
    }
    let _guard = DisableInterruptGuard::new();
    unsafe { TICKS += 1 };
}

pub(crate) fn current_system_ticks() -> u64 {
    let _guard = DisableInterruptGuard::new();
    unsafe { TICKS }
}

pub(crate) fn system_ticks_to_duration(ticks: u64) -> Duration {
    let val = 1_000_000 * now / TICKS_PER_SECOND;
    Duration::from_micros(val)
}

pub(crate) fn ticks_to_cycles(ticks: u64) -> u64 {
    // TODO: Warn if hz() % TICKS_PER_SECOND != 0.
    const K: u64 = SysTickImpl::hz() / TICKS_PER_SECOND;
    K * ticks
}

pub(crate) fn uptime() -> Duration {
    let now = current_system_ticks();
    system_ticks_to_duration(now)
}

pub struct ScopeTimer<'a> {
    start: u64,
    diff: &'a mut u64,
}

impl ScopeTimer<'_> {
    pub fn new(diff: &mut u64) -> Self {
        ScopeTimer {
            start: current_system_ticks(),
            diff,
        }
    }
}

impl Drop for ScopeTimer<'_> {
    fn drop(&mut self) {
        *self.diff = current_system_ticks() - self.start;
    }
}
