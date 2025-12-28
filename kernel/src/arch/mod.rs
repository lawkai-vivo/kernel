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

#[cfg(target_arch = "arm")]
pub mod arm;
#[cfg(target_arch = "arm")]
pub use arm::{irq, ArchImpl, Context};

#[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
pub mod riscv;
#[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
pub use riscv::{irq, ArchImpl, Context};

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
#[cfg(target_arch = "aarch64")]
pub use aarch64::{irq, ArchImpl, Context};

use crate::scheduler::ContextSwitchHookHolder;

pub trait Arch {
    extern "C" fn current_cpu_id() -> usize;
    extern "C" fn switch_context_with_hook(hook: *mut ContextSwitchHookHolder);
    extern "C" fn disable_local_irq();
    extern "C" fn enable_local_irq();
    extern "C" fn local_irq_enabled() -> bool;
    extern "C" fn disable_local_irq_save() -> usize;
    extern "C" fn enable_local_irq_restore(val: usize);
    extern "C" fn idle();
    extern "C" fn switch_stack(
        to_sp: usize,
        continuation: extern "C" fn(to_sp: usize, from_sp: usize),
        return_address: usize,
    ) -> !;
    extern "C" fn pend_context_switch();
    extern "C" fn claim_context_switch() -> bool;
    extern "C" fn current_sp() -> usize;
    extern "C" fn start_schedule(cont: extern "C" fn() -> !);
}
