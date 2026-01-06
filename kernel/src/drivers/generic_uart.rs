// Copyright (c) 2026 vivo Mobile Communication Co., Ltd.
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

use core::{
    fmt::Write,
    ptr::{read_volatile, write_volatile},
};

/// 定义寄存器读写接口（适配 MMIO 或 x86 端口 I/O）
pub trait RegisterInterface {
    unsafe fn write(&self, offset: usize, value: u8);
    unsafe fn read(&self, offset: usize) -> u8;
}

/// MMIO 实现（适用于 RISC-V, ARM, 现代 x86）
pub struct MmioInterface {
    base: usize,
}

impl RegisterInterface for MmioInterface {
    unsafe fn write(&self, offset: usize, value: u8) {
        write_volatile((self.base + offset) as *mut u8, value);
    }
    unsafe fn read(&self, offset: usize) -> u8 {
        read_volatile((self.base + offset) as *const u8)
    }
}

/// 平台特定的寄存器布局配置
pub trait UartConfig {
    const DATA: usize; // 数据寄存器偏移
    const IER: usize; // 中断使能寄存器偏移
    const LSR: usize; // 状态寄存器偏移
    const LSR_TX_IDLE: u8; // 发送空闲位掩码
    const LSR_RX_READY: u8; // 接收就绪位掩码
}

/// 典型 NS16550 (QEMU RISC-V/PC) 配置
pub struct Ns16550Config;
impl UartConfig for Ns16550Config {
    const DATA: usize = 0;
    const IER: usize = 1;
    const LSR: usize = 5;
    const LSR_TX_IDLE: u8 = 0x20;
    const LSR_RX_READY: u8 = 0x01;
}

/// 通用 UART 驱动结构
pub struct GenericUart<C: UartConfig, I: RegisterInterface> {
    config: C,
    interface: I,
}

impl<C: UartConfig, I: RegisterInterface> GenericUart<C, I> {
    pub const fn new(config: C, interface: I) -> Self {
        Self { config, interface }
    }

    pub fn init(&self) {
        unsafe {
            // 简单的通用初始化逻辑（实际需根据 C 的特性深度定制）
            self.interface.write(C::IER, 0x00); // 禁用中断
        }
    }

    pub fn putchar(&self, c: u8) {
        unsafe {
            // 等待发送缓冲区空闲
            while (self.interface.read(C::LSR) & C::LSR_TX_IDLE) == 0 {}
            self.interface.write(C::DATA, c);
        }
    }

    pub fn getchar(&self) -> u8 {
        unsafe {
            // 等待数据
            while (self.interface.read(C::LSR) & C::LSR_RX_READY) == 0 {}
            self.interface.read(C::DATA)
        }
    }
}

// 为通用驱动实现 Write Trait 以支持 print!
impl<C: UartConfig, I: RegisterInterface> Write for GenericUart<C, I> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            self.putchar(b);
        }
        Ok(())
    }
}
