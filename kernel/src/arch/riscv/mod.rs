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

pub(crate) mod irq;
mod trap;

use crate::{irq as sysirq, scheduler, scheduler::ContextSwitchHookHolder, thread::Thread};
use blueos_kconfig::NUM_CORES;
use core::{
    cell::Cell,
    mem::offset_of,
    sync::atomic::{compiler_fence, Ordering},
};
pub use trap::*;

pub(crate) const NR_SWITCH: usize = !0;

// See https://five-embeddev.com/riscv-priv-isa-manual/Priv-v1.12/machine.html#machine-status-registers-mstatus-and-mstatush
pub(crate) const MSTATUS_MIE: usize = 1 << 3;
pub(crate) const MSTATUS_MPIE: usize = 1 << 7;
pub(crate) const MSTATUS_MPP_MASK: usize = 0b11 << 11;
pub(crate) const MSTATUS_MPP_U: usize = 0b00 << 11;
pub(crate) const MSTATUS_MPP_S: usize = 0b01 << 11;
pub(crate) const MSTATUS_MPP_M: usize = 0b11 << 11;
pub(crate) const MIE_SSIE: usize = 1 << 1;
pub(crate) const MIE_MSIE: usize = 1 << 3;
pub(crate) const MIE_STIE: usize = 1 << 5;
pub(crate) const MIE_MTIE: usize = 1 << 7;
pub(crate) const MIE_SEIE: usize = 1 << 9;
pub(crate) const MIE_MEIE: usize = 1 << 11;
// We haven't supported supervisor mode and user mode yet.

// FIXME: We don't need atomic here.
static mut PENDING_SWITCH_CONTEXT: [Cell<bool>; NUM_CORES] =
    [const { Cell::new(false) }; NUM_CORES];

#[inline]
pub(crate) extern "C" fn pend_switch_context() {
    if !sysirq::is_in_irq() {
        scheduler::relinquish_me();
        return;
    }
    let level = disable_local_irq_save();
    let id = current_cpu_id();
    unsafe { PENDING_SWITCH_CONTEXT[id].set(true) };
    enable_local_irq_restore(level);
}

#[inline]
pub(crate) extern "C" fn claim_switch_context() -> bool {
    let level = disable_local_irq_save();
    let id = current_cpu_id();
    let ok = unsafe { PENDING_SWITCH_CONTEXT[id].get() };
    unsafe { PENDING_SWITCH_CONTEXT[id].set(false) };
    enable_local_irq_restore(level);
    ok
}

#[inline]
pub(crate) extern "C" fn local_irq_enabled() -> bool {
    let x: usize;
    unsafe {
        core::arch::asm!("csrr {}, mstatus", out(reg) x,
                         options(nostack))
    };
    x & MSTATUS_MIE != 0
}

#[macro_export]
macro_rules! arch_bootstrap {
    ($stack_start:path, $stack_end:path, $cont: path) => {
        core::arch::naked_asm!(
            "csrci mstatus, 0x8",
            "la gp, __global_pointer$",
            "la sp, {stack_end}",
            "csrr t0, mhartid",
            "li t1, {stack_size}",
            "mul t0, t0, t1",
            "sub sp, sp, t0",
            "call {bootstrap}",
            "la t0, {cont}",
            "jalr x0, t0, 0",
            stack_size = const 0x1000,
            stack_end = sym $stack_end,
            bootstrap = sym $crate::arch::riscv::bootstrap,
            cont = sym $cont,
        );
    }
}

macro_rules! clear_mstatus_mie {
    () => {
        "
        csrci mstatus, 0x8
        "
    };
}

macro_rules! set_mstatus_mie {
    () => {
        "
        csrsi mstatus, 0x8
        "
    };
}

#[inline]
pub(crate) extern "C" fn disable_local_irq() {
    compiler_fence(Ordering::SeqCst);
    unsafe { core::arch::asm!(clear_mstatus_mie!(), options(nostack)) };
}

#[inline]
pub(crate) extern "C" fn enable_local_irq() {
    unsafe { core::arch::asm!(set_mstatus_mie!(), options(nostack)) };
    compiler_fence(Ordering::SeqCst);
}

#[inline]
pub(crate) extern "C" fn idle() {
    unsafe { core::arch::asm!("wfi", options(nostack)) };
}

#[inline]
pub(crate) extern "C" fn disable_local_irq_save() -> usize {
    compiler_fence(Ordering::SeqCst);
    let old: usize;
    unsafe {
        core::arch::asm!("csrrci {old}, mstatus, {bit}",
                         bit = const MSTATUS_MIE,
                         old = out(reg) old,
                         options(nostack),
        )
    };
    old
}

#[inline]
pub(crate) extern "C" fn enable_local_irq_restore(old: usize) {
    unsafe {
        core::arch::asm!("csrw mstatus, {old}", old = in(reg) old,
                         options(nostack))
    };
    compiler_fence(Ordering::SeqCst);
}

#[inline]
pub extern "C" fn current_sp() -> usize {
    let x: usize;
    unsafe { core::arch::asm!("mv {}, sp", out(reg) x, options(nostack, nomem)) };
    x
}

#[inline(always)]
pub(crate) extern "C" fn switch_context(saved_sp_mut: *mut u8, to_sp: usize) {
    switch_context_with_hook(saved_sp_mut, to_sp, core::ptr::null_mut());
}

#[inline(never)]
pub(crate) extern "C" fn ecall_switch_context_with_hook(
    saved_sp_mut: *mut u8,
    to_sp: usize,
    hook: *mut ContextSwitchHookHolder,
) {
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") saved_sp_mut as usize => _,
            inlateout("a1") to_sp => _,
            in("a2") hook as usize,
            in("a7") NR_SWITCH,
            options(nostack),
        )
    }
}

#[inline]
pub(crate) extern "C" fn switch_context_with_hook(
    saved_sp_mut: *mut u8,
    to_sp: usize,
    hook: *mut ContextSwitchHookHolder,
) {
    ecall_switch_context_with_hook(saved_sp_mut, to_sp, hook)
}

#[inline(always)]
#[allow(clippy::empty_loop)]
pub(crate) extern "C" fn restore_context_with_hook(
    to_sp: usize,
    hook: *mut ContextSwitchHookHolder,
) -> ! {
    switch_context_with_hook(core::ptr::null_mut(), to_sp, hook);
    unreachable!("Should have switched to another thread");
}

// This context is used when we are performing context switching in
// thread mode or in the first level ISR.
// TODO: Add floating point registers.
#[cfg_attr(target_pointer_width = "64", repr(C, align(16)))]
#[cfg_attr(target_pointer_width = "32", repr(C, align(8)))]
#[derive(Default, Debug)]
pub(crate) struct Context {
    pub ra: usize,
    pub gp: usize,
    pub tp: usize,
    pub fp: usize,
    pub s1: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub mepc: usize,
    pub mcause: usize,
    pub mtval: usize,
    pub mstatus: usize,
    pub padding: usize,
}

#[cfg_attr(target_pointer_width = "64", repr(C, align(16)))]
#[cfg_attr(target_pointer_width = "32", repr(C, align(8)))]
#[derive(Default, Debug)]
pub(crate) struct IsrContext {
    // We're not allowing nested ISR at the moment.
    pub ra: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
}

impl IsrContext {
    #[inline]
    pub(crate) fn init(&mut self) -> &mut Self {
        self
    }

    // We are following C-ABI, since Rust ABI is not stablized.
    // FIXME: rustc miscompiles it if inlined.
    #[inline(never)]
    pub(crate) fn set_return_address(&mut self, pc: usize) -> &mut Self {
        self.ra = pc;
        self
    }

    #[inline(never)]
    pub(crate) fn set_arg(&mut self, index: usize, val: usize) -> &mut Self {
        match index {
            0 => self.a0 = val,
            1 => self.a1 = val,
            2 => self.a2 = val,
            3 => self.a3 = val,
            4 => self.a4 = val,
            5 => self.a5 = val,
            6 => self.a6 = val,
            7 => self.a7 = val,
            _ => {}
        }
        self
    }
}

pub(crate) extern "C" fn bootstrap() {
    unsafe {
        core::arch::asm!(
            "csrs mstatus, {mstatus}",
            "csrs mie, {mie}",
            mstatus = in(reg) MSTATUS_MPP_M | MSTATUS_MPIE,
            mie = in(reg) MIE_MTIE|MIE_MSIE|MIE_MEIE,
            options(nostack),
        )
    };
}

pub(crate) extern "C" fn start_schedule(cont: extern "C" fn() -> !) {
    let current = crate::scheduler::current_thread_ref();
    current.lock().reset_saved_sp();
    let sp = current.saved_sp();
    unsafe {
        core::arch::asm!(
            "li ra, 0",
            "mv sp, {sp}",
            "jalr x0, {cont}, 0",
            sp = in(reg) sp,
            cont = in(reg) cont,
            options(noreturn),
        )
    }
}

#[inline(always)]
pub(crate) extern "C" fn current_cpu_id() -> usize {
    let id: usize;
    unsafe {
        core::arch::asm!("csrr {}, mhartid", out(reg) id,
                              options(nostack))
    };
    id
}

#[naked]
pub(crate) extern "C" fn switch_stack(
    to_sp: usize,
    cont: extern "C" fn(sp: usize, old_sp: usize),
) -> ! {
    unsafe {
        core::arch::naked_asm!(
            "
            mv t0, a1
            mv a1, sp
            mv sp, a0
            jalr x0, t0, 0
            "
        )
    }
}

#[cfg(target_pointer_width = "32")]
#[macro_export]
macro_rules! rv_save_context {
    () => {
        "
        sw ra, {ra}({base})
        sw gp, {gp}({base})
        sw tp, {tp}({base})
        sw fp, {fp}({base})
        sw s1, {s1}({base})
        sw s2, {s2}({base})
        sw s3, {s3}({base})
        sw s4, {s4}({base})
        sw s5, {s5}({base})
        sw s6, {s6}({base})
        sw s7, {s7}({base})
        sw s8, {s8}({base})
        sw s9, {s9}({base})
        sw s10, {s10}({base})
        sw s11, {s11}({base})
        csrr s1, mepc
        csrr s2, mstatus
        csrr s3, mcause
        csrr s3, mtval
        sw s1, {mepc}({base})
        sw s2, {mstatus}({base})
        sw s3, {mcause}({base})
        sw s4, {mtval}({base})
        "
    };
}

#[cfg(target_pointer_width = "64")]
#[macro_export]
macro_rules! rv_save_context {
    () => {
        "
        sd ra, {ra}({base})
        sd gp, {gp}({base})
        sd tp, {tp}({base})
        sd fp, {fp}({base})
        sd s1, {s1}({base})
        sd s2, {s2}({base})
        sd s3, {s3}({base})
        sd s4, {s4}({base})
        sd s5, {s5}({base})
        sd s6, {s6}({base})
        sd s7, {s7}({base})
        sd s8, {s8}({base})
        sd s9, {s9}({base})
        sd s10, {s10}({base})
        sd s11, {s11}({base})
        csrr s1, mepc
        csrr s2, mstatus
        csrr s3, mcause
        csrr s3, mtval
        sd s1, {mepc}({base})
        sd s2, {mstatus}({base})
        sd s3, {mcause}({base})
        sd s4, {mtval}({base})
        "
    };
}

#[cfg(target_pointer_width = "32")]
#[macro_export]
macro_rules! rv_restore_context {
    () => {
        "
        lw s1, {mepc}({base})
        lw s2, {mstatus}({base})
        lw s3, {mcause}({base})
        lw s4, {mtval}({base})
        csrw mepc, s1
        csrw mstatus, s2
        csrw mcause, s3
        csrw mtval, s4
        lw ra, {ra}({base})
        lw gp, {gp}({base})
        lw tp, {tp}({base})
        lw fp, {fp}({base})
        lw s1, {s1}({base})
        lw s2, {s2}({base})
        lw s3, {s3}({base})
        lw s4, {s4}({base})
        lw s5, {s5}({base})
        lw s6, {s6}({base})
        lw s7, {s7}({base})
        lw s8, {s8}({base})
        lw s9, {s9}({base})
        lw s10, {s10}({base})
        lw s11, {s11}({base})
        "
    };
}

#[cfg(target_pointer_width = "64")]
#[macro_export]
macro_rules! rv_restore_context {
    () => {
        "
        ld s1, {mepc}({base})
        ld s2, {mstatus}({base})
        ld s3, {mcause}({base})
        ld s4, {mtval}({base})
        csrw mepc, s1
        csrw mstatus, s2
        csrw mcause, s3
        csrw mtval, s4
        ld ra, {ra}({base})
        ld gp, {gp}({base})
        ld tp, {tp}({base})
        ld fp, {fp}({base})
        ld s1, {s1}({base})
        ld s2, {s2}({base})
        ld s3, {s3}({base})
        ld s4, {s4}({base})
        ld s5, {s5}({base})
        ld s6, {s6}({base})
        ld s7, {s7}({base})
        ld s8, {s8}({base})
        ld s9, {s9}({base})
        ld s10, {s10}({base})
        ld s11, {s11}({base})
        "
    };
}

#[naked]
extern "C" fn switch(
    hook: &mut ContextSwitchHookHolder,
    prev: *const Thread,
    next: *const Thread,
) -> &mut ContextSwitchHookHolder {
    unsafe {
        core::arch::naked_asm!(
            "
            addi sp, sp, -{ctx_size},
            ",
            rv_save_context!(),
            #[cfg(target_pointer_width = "32")]
            "
            sw sp, {saved_sp}(a1),
            lw sp, {saved_sp}(a2),
            ",
            #[cfg(target_pointer_width = "64")]
            "
            sd sp, {saved_sp}(a1),
            ld sp, {saved_sp}(a2),
            ",
            rv_restore_context!(),
            "
            addi sp, sp, {ctx_size},
            ret
            ",
            base = in("sp"),
            saved_sp = const offset_of!(Thread, saved_sp),
            ctx_size = const core::mem::size_of::<Context>(),
            mepc = const offset_of!(Context, mepc),
            mstatus = const offset_of!(Context, mstatus),
            mcause = const offset_of!(Context, mcause),
            mtval = const offset_of!(Context, mtval),
            ra = const offset_of!(Context, ra),
            gp = const offset_of!(Context, gp),
            tp = const offset_of!(Context, tp),
            fp = const offset_of!(Context, fp),
            s1 = const offset_of!(Context, s1),
            s2 = const offset_of!(Context, s2),
            s3 = const offset_of!(Context, s3),
            s4 = const offset_of!(Context, s4),
            s5 = const offset_of!(Context, s5),
            s6 = const offset_of!(Context, s6),
            s7 = const offset_of!(Context, s7),
            s8 = const offset_of!(Context, s8),
            s9 = const offset_of!(Context, s9),
            s10 = const offset_of!(Context, s10),
            s11 = const offset_of!(Context, s11),
        )
    }
}
