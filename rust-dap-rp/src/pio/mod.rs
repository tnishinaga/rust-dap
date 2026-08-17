// Copyright 2022 Ein Terakawa
// Copyright 2023 Toshifumi Nishinaga
//
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::pio_crate as pio;
use pio::Program;
pub mod pio0 {
    use crate::hal::{self, gpio::FunctionPio0};
    pub type Pin<P> = hal::gpio::Pin<P, FunctionPio0, hal::gpio::PullDown>;
}

pub mod jtag;
pub mod swd;

#[cfg(feature = "set_clock")]
use core::num::NonZeroU32;

/// Default PIO divisor. A divisor of 1.0 makes the PIO state-machine clock
/// equal to the board's system clock.
const DEFAULT_PIO_DIVISOR: f32 = 1.0;

#[cfg(feature = "set_clock")]
fn clock_divisor(system_clock_hz: u32, frequency_hz: u32, cycles_per_clock: u32) -> f32 {
    let (Some(system_clock_hz), Some(frequency_hz), Some(cycles_per_clock)) = (
        NonZeroU32::new(system_clock_hz),
        NonZeroU32::new(frequency_hz),
        NonZeroU32::new(cycles_per_clock),
    ) else {
        return DEFAULT_PIO_DIVISOR;
    };

    (system_clock_hz.get() as f32 / cycles_per_clock.get() as f32 / frequency_hz.get() as f32)
        .max(DEFAULT_PIO_DIVISOR)
}

#[cfg(feature = "set_clock")]
fn default_swj_clock_hz(system_clock_hz: u32, cycles_per_clock: u32) -> u32 {
    (system_clock_hz as f32 / cycles_per_clock as f32 / DEFAULT_PIO_DIVISOR) as u32
}

fn swj_pins_divisor(system_clock_hz: u32) -> f32 {
    // The SWJ pin program consumes ten PIO cycles per wait_us unit.
    (system_clock_hz as f32 / 10_000_000.0).max(DEFAULT_PIO_DIVISOR)
}

fn swj_pins_program() -> Program<{ pio::RP2040_MAX_PROGRAM_SIZE }> {
    type Assembler = pio::Assembler<{ pio::RP2040_MAX_PROGRAM_SIZE }>;
    let mut a = Assembler::new();
    let mut delay_loop = a.label();
    let mut wrap_target = a.label();
    let mut wrap_source = a.label();

    a.bind(&mut wrap_target);

    // command data
    // [
    //      pin_output_values: u32,
    //      pin_directions: u32,
    //      wait_us:    u32
    // ]

    // load and set output value
    a.pull(false, true);
    a.out(pio::OutDestination::PINS, 32);
    // load and set direction
    a.pull(false, true);
    a.out(pio::OutDestination::PINDIRS, 32);

    // load output delay to Y
    a.pull(false, true);
    a.out(pio::OutDestination::Y, 32);

    // wait_us
    a.bind(&mut delay_loop);
    // delay 9(+1) cycles * 0.1us/cycle = 1us
    a.jmp_with_delay(pio::JmpCondition::YDecNonZero, &mut delay_loop, 9);

    // check all pin status
    a.r#in(pio::InSource::PINS, 32);
    a.push(false, true);

    a.bind(&mut wrap_source);

    a.assemble_with_wrap(wrap_source, wrap_target)
}

fn all_pins_to_input_program() -> Program<{ pio::RP2040_MAX_PROGRAM_SIZE }> {
    type Assembler = pio::Assembler<{ pio::RP2040_MAX_PROGRAM_SIZE }>;
    let mut a = Assembler::new();
    let mut wrap_target = a.label();
    let mut wrap_source = a.label();

    a.bind(&mut wrap_target);

    a.mov(
        pio::MovDestination::X,
        pio::MovOperation::None,
        pio::MovSource::NULL,
    );
    a.out(pio::OutDestination::PINDIRS, 32);

    a.bind(&mut wrap_source);

    a.assemble_with_wrap(wrap_source, wrap_target)
}
