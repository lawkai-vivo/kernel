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

use super::{
    claim_switch_context, disable_local_irq, enable_local_irq, Context, IsrContext, NR_SWITCH,
};
use crate::{
    boards::handle_plic_irq,
    debug,
    irq::{enter_irq, leave_irq},
    rv_restore_context, rv_restore_isr_context, rv_save_context, rv_save_isr_context, scheduler,
    scheduler::ContextSwitchHookHolder,
    support::sideeffect,
    syscalls::{dispatch_syscall, Context as ScContext},
    thread,
    thread::Thread,
    types::Arc,
};
use core::{
    mem::offset_of,
    sync::atomic::{compiler_fence, fence, Ordering},
};

pub(crate) const INTERRUPT_MASK: usize = 1usize << (usize::BITS - 1);
pub(crate) const TIMER_INT: usize = INTERRUPT_MASK | 0x7;
pub(crate) const ECALL: usize = 0xB;
pub(crate) const EXTERN_INT: usize = INTERRUPT_MASK | 0xB;

// trap_handler decides whether nested interrupt is allowed.
#[repr(align(4))]
#[naked]
pub(crate) unsafe extern "C" fn trap_entry() {
    core::arch::naked_asm!(
        concat!(
            rv_save_isr_context!(),
            "
            call {enter_irq}
            mv a0, sp
            mv a1, mcause
            mv a2, mtval
            call {handle_trap}
            call {leave_irq}
            ",
            rv_restore_isr_context!(),
            "
            mret
            "
        ),
        enter_irq = sym enter_irq,
        leave_irq = sym leave_irq,
        handle_trap = sym handle_trap,
        ctx_size = const core::mem::size_of::<Context>(),
        ra = const offset_of!(IsrContext, ra),
        t0 = const offset_of!(IsrContext, t0),
        t1 = const offset_of!(IsrContext, t1),
        t2 = const offset_of!(IsrContext, t2),
        t3 = const offset_of!(IsrContext, t3),
        t4 = const offset_of!(IsrContext, t4),
        t5 = const offset_of!(IsrContext, t5),
        t6 = const offset_of!(IsrContext, t6),
        a0 = const offset_of!(IsrContext, a0),
        a1 = const offset_of!(IsrContext, a1),
        a2 = const offset_of!(IsrContext, a2),
        a3 = const offset_of!(IsrContext, a3),
        a4 = const offset_of!(IsrContext, a4),
        a5 = const offset_of!(IsrContext, a5),
        a6 = const offset_of!(IsrContext, a6),
        a7 = const offset_of!(IsrContext, a7),
    )
}

#[derive(Default, Debug)]
struct SyscallGuard {
    isr_ctx: Context,
}

impl SyscallGuard {
    pub fn new() -> Self {
        let mut g = Self::default();
        unsafe {
            core::arch::asm!(
                "
                fence rw, rw
                ",
                rv_save_context!(),
                base = in(reg) &mut g.isr_ctx as *mut _ as usize,
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
        compiler_fence(Ordering::SeqCst);
        leave_irq();
        enable_local_irq();
        g
    }
}

impl Drop for SyscallGuard {
    fn drop(&mut self) {
        disable_local_irq();
        enter_irq();
        compiler_fence(Ordering::SeqCst);
        unsafe {
            core::arch::asm!(
                "
                fence rw, rw
                ",
                rv_restore_context!(),
                base = in(reg) &mut self.isr_ctx as *mut _ as usize,
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
}

extern "C" fn handle_ecall(ctx: &mut IsrContext) {
    {
        let scg = SyscallGuard::new();
        let sc = ScContext {
            nr: ctx.a7,
            args: [ctx.a0, ctx.a1, ctx.a2, ctx.a3, ctx.a4, ctx.a5],
        };
        ctx.a0 = dispatch_syscall(&sc);
    }
    let mepc: usize;
    unsafe {
        core::arch::asm!(
            "
        csrr {tmp}, mepc
        addi {tmp}, {tmp}, 4
        csrw mepc, {tmp}
        ",
            tmp = out(reg) mepc,
            options(nostack),
        )
    }
}

fn might_switch_context() {
    if !claim_switch_context() {
        return;
    }
    let this_thread = scheduler::current_thread_ref();
    let Some(next) = scheduler::next_preferred_thread(this_thread.priority()) else {
        return;
    };
    this_thread.disable_preempt();
    this_thread.start_context_switch(thread::READY);
    let next_ptr = Arc::as_ptr(&next);
    let mut hooks = ContextSwitchHookHolder::new(next);
    let prev_hooks = super::switch(&mut hooks, this_thread as *const _, next_ptr);
    scheduler::save_context_finish_hook(Some(prev_hooks));
    this_thread.enable_preempt();
}

extern "C" fn handle_trap(ctx: &mut IsrContext, mcause: usize, mtval: usize) {
    debug_assert!(!super::local_irq_enabled());
    match mcause {
        EXTERN_INT => {
            handle_plic_irq(ctx, mcause, mtval);
        }
        TIMER_INT => {
            crate::time::handle_tick_increment();
        }
        ECALL => {
            handle_ecall(ctx);
        }
        _ => {
            let t = scheduler::current_thread_ref();
            panic!(
                "[C#{}:0x{:x}] Unexpected trap: context: {:?}, mcause: 0x{:x}, mtval: 0x{:x}",
                super::current_cpu_id(),
                Thread::id(t),
                ctx,
                mcause,
                mtval
            );
        }
    }
    might_switch_context();
}
