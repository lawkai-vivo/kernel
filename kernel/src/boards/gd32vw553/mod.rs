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

use crate::{
    allocator,
    arch::{
        self,
        riscv::{local_irq_enabled, trap_entry, Context},
    },
    devices::{console, dumb, tty::n_tty::Tty, Device, DeviceManager},
    scheduler,
    support::SmpStagedInit,
    time,
    types::Arc,
};
use alloc::{
    alloc::{alloc as system_alloc, dealloc as system_dealloc},
    string::String,
};
use blueos_driver::pinctrl::gd32_af::{AfioMode, Gd32Alterfunc, OutputSpeed, OutputType, PullMode};
use blueos_kconfig::TICKS_PER_SECOND;

const CLOCK_ADDR: usize = 0xD1000000;
const CLOCK_TIME: usize = CLOCK_ADDR;
const CLOCK_CMP: usize = CLOCK_ADDR + core::mem::size_of::<u64>();
const CLOCK_HZ: u64 = 40_000_000;
const NANOS_PER_CLOCK_CYCLE: u64 = 1_000_000_000 / CLOCK_HZ;

type Handler = unsafe extern "C" fn();

#[repr(C, align(64))]
struct Vector<const N: usize>([Handler; N]);

impl<const N: usize> Vector<N> {
    pub const fn new() -> Self {
        Self([const { trap_entry }; N])
    }

    pub const fn set(&mut self, index: usize, h: Handler) -> &mut Self {
        self.0[index] = h;
        self
    }
}

#[used]
static mut VECTOR: Vector<{ 116usize }> = Vector::new();

#[inline]
fn clock_timecmp_ptr() -> *mut u64 {
    unsafe { (CLOCK_CMP) as *mut u64 }
}

#[inline]
pub fn current_clock_cycles() -> u64 {
    unsafe { (CLOCK_TIME as *const u64).read_volatile() }
}

#[inline]
pub fn current_cpu_cycles() -> u64 {
    let hi: u32;
    let lo: u32;
    unsafe {
        core::arch::asm!(
            "rdcycle {lo}",
            "rdcycleh {hi}",
            hi = out(reg) hi,
            lo = out(reg) lo,
            options(nostack, nomem),
        )
    }
    ((hi as u64) << 32) + lo as u64
}

fn set_timecmp(deadline: u64) {
    unsafe { clock_timecmp_ptr().write_volatile(deadline) };
}

#[inline]
fn init_vector_table() {
    unsafe {
        core::arch::asm!(
            "la {x}, {trap_entry}",
            // "ori {x}, {x}, 0x3",
            "csrw mtvec, {x}",
            "la {x}, {vector}",
            // Inline assembler is unable to encode mtvt, use the numeric value of mtvt here.
            "csrw 0x307, {x}",
            x = out(reg) _,
            trap_entry = sym trap_entry,
            vector = sym VECTOR,
            options(nostack),
        );
        // Software interrupt.
        // VECTOR.set(3, trap_entry);
        // Timer interrupt.
        // VECTOR.set(7, trap_entry);
    }
}

macro_rules! rv_csr_set {
    ($csr:expr, $val:expr) => {{
        let __v: u32 = $val as u32; // 假设rv_csr_t是u32
        unsafe {
            core::arch::asm!(
                concat!("csrs ", stringify!($csr), ", {0}"),
                in(reg) __v,
                options(nomem, nostack)
            );
        }
    }};
}

pub(crate) fn handle_irq(_ctx: &Context, _mcause: usize, _mtval: usize) {
    let irq_number: usize;
    unsafe {
        core::arch::asm!(
            // // Inline assembler is unable to encode mnxti, use the numeric value of mtvt here.
            "csrr {}, 0x345",
            out(reg) irq_number,
            options(nostack),
        )
    }
}

pub(crate) fn set_timeout_after_nanos(_nanos: u64) {
    // set_timecmp(nanos / NANOS_PER_CLOCK_CYCLE);
}

pub(crate) fn set_timeout_after_clock_cycles(cycles: u64) {
    set_timecmp(cycles);
}

#[inline]
pub(crate) fn clock_cycles_to_millis(cycles: u64) -> u64 {
    cycles * 1_000 / CLOCK_HZ
}

pub(crate) fn clock_cycles_to_duration(cycles: u64) -> core::time::Duration {
    core::time::Duration::from_nanos(cycles * NANOS_PER_CLOCK_CYCLE)
}

pub(crate) fn uptime() -> core::time::Duration {
    clock_cycles_to_duration(current_clock_cycles())
}

unsafe fn copy_data() {
    extern "C" {
        static __data_lma: u8;
        static mut __data_start: u8;
        static mut __data_end: u8;
    }
    let src = core::ptr::addr_of!(__data_lma);
    let dst = core::ptr::addr_of_mut!(__data_start);
    let size = core::ptr::addr_of_mut!(__data_end).offset_from(dst as *const _) as usize;
    core::ptr::copy_nonoverlapping(src, dst, size)
}

pub(crate) fn init() {
    debug_assert!(!local_irq_enabled());
    unsafe { copy_data() };
    crate::boot::init_bss();
    use blueos_hal::clock_control::ClockControl;
    blueos_driver::clock_control::gd32_clock_control::Gd32ClockControl::init();
    crate::boot::init_heap();

    init_vector_table();
    time::systick_init(0);
    time::reset_systick();
}

crate::define_peripheral! {
    (console_uart, blueos_driver::uart::gd32vw55x::Gd32vw55xUart,
    blueos_driver::uart::gd32vw55x::Gd32vw55xUart::new(
        0x4000_4800
    ))
}

define_pin_states!(
    Gd32Alterfunc,
    (
        0x4002_0400,
        15,
        PullMode::PullUp,
        AfioMode::Af8,
        OutputType::PushPull,
        OutputSpeed::Medium,
    ),
    (
        0x4002_0000,
        8,
        PullMode::PullUp,
        AfioMode::Af2,
        OutputType::PushPull,
        OutputSpeed::Medium,
    ),
);

// Used by many drivers in gd32's SDK.
#[no_mangle]
pub extern "C" fn delay_1ms(millis: u32) {
    let ticks = (millis as usize * TICKS_PER_SECOND) / 1000;
    if ticks == 0 {
        scheduler::yield_me()
    } else {
        scheduler::suspend_me_for(ticks);
    }
}
