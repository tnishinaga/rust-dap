// Copyright 2021-2022 Kenta Ida
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

#![no_std]
#![no_main]

use embedded_hal::digital::StatefulOutputPin;
use hal::clocks::Clock;
use hal::gpio::FunctionUart;
use hal::pac;
use rp235x_hal as hal;
use rust_dap::{DapConfig, DapIdentity};
use rust_dap_rp::bridge;
use rust_dap_rp::line_coding::UartConfig;
use rust_dap_rp::util::{UartConfigAndClock, UsbIdentity};
use usb_device::bus::UsbBusAllocator;

#[cfg(not(feature = "defmt"))]
use panic_halt as _;
#[cfg(feature = "defmt")]
use {defmt_rtt as _, panic_probe as _};

#[cfg(all(feature = "swd", any(feature = "jtag", feature = "swj")))]
compile_error!("SWD, JTAG, and SWJ transport features are mutually exclusive");
#[cfg(all(feature = "jtag", feature = "swj"))]
compile_error!("SWD, JTAG, and SWJ transport features are mutually exclusive");
#[cfg(not(any(feature = "swd", feature = "jtag", feature = "swj")))]
compile_error!("select one transport feature: swd, jtag, or swj");

/// Tell the RP2350 Boot ROM that this is an Arm executable image.
#[link_section = ".start_block"]
#[used]
pub static IMAGE_DEF: hal::block::ImageDef = hal::block::ImageDef::secure_exe();

const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

#[rp235x_hal::entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);
    let clocks = hal::clocks::init_clocks_and_plls(
        XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .unwrap();

    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let uart_pins = (
        pins.gpio0.into_function::<FunctionUart>(),
        pins.gpio1.into_function::<FunctionUart>(),
    );
    let mut uart_config = UartConfigAndClock {
        config: UartConfig::from(hal::uart::UartConfig::default()),
        clock: clocks.peripheral_clock.freq(),
    };
    let uart = hal::uart::UartPeripheral::new(pac.UART0, uart_pins, &mut pac.RESETS)
        .enable((&uart_config.config).into(), uart_config.clock)
        .unwrap();
    let (uart_reader, uart_writer) = uart.split();
    let mut uart_reader = Some(uart_reader);
    let mut uart_writer = Some(uart_writer);

    let usb_allocator = UsbBusAllocator::new(hal::usb::UsbBus::new(
        pac.USB,
        pac.USB_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    #[cfg(all(feature = "swd", not(feature = "bitbang")))]
    let transport = {
        use hal::gpio::FunctionPio0;
        let swclk = pins.gpio2.into_function::<FunctionPio0>();
        let swdio = pins.gpio3.into_function::<FunctionPio0>();
        let reset = pins.gpio4.into_function::<FunctionPio0>();
        rust_dap_rp::util::SwdIoSet::new(
            pac.PIO0,
            swclk,
            swdio,
            reset,
            clocks.system_clock.freq().to_Hz(),
            &mut pac.RESETS,
        )
    };

    #[cfg(all(feature = "swd", feature = "bitbang"))]
    let transport = {
        use rust_dap_rp::bitbang::{CortexMDelay, PicoBidirPin};
        let swclk = PicoBidirPin::new(pins.gpio2.into_floating_input());
        let swdio = PicoBidirPin::new(pins.gpio3.into_floating_input());
        let reset = PicoBidirPin::new(pins.gpio4.into_floating_input());
        rust_dap_rp::bitbang::SwdIoSet::new(swclk, swdio, reset, CortexMDelay)
    };

    #[cfg(all(feature = "jtag", not(feature = "bitbang")))]
    let transport = {
        use hal::gpio::FunctionPio0;
        let tck = pins.gpio2.into_function::<FunctionPio0>();
        let tms = pins.gpio3.into_function::<FunctionPio0>();
        let tdo = pins.gpio5.into_function::<FunctionPio0>();
        let tdi = pins.gpio6.into_function::<FunctionPio0>();
        let trst = pins.gpio7.into_function::<FunctionPio0>();
        let srst = pins.gpio4.into_function::<FunctionPio0>();
        rust_dap_rp::util::JtagIoSet::new(
            pac.PIO0,
            tck,
            tms,
            tdi,
            tdo,
            Some(trst),
            Some(srst),
            clocks.system_clock.freq().to_Hz(),
            &mut pac.RESETS,
        )
    };

    #[cfg(all(feature = "jtag", feature = "bitbang"))]
    let transport = {
        use rust_dap_rp::bitbang::{CortexMDelay, PicoBidirPin};
        let tck = PicoBidirPin::new(pins.gpio2.into_floating_input());
        let tms = PicoBidirPin::new(pins.gpio3.into_floating_input());
        let tdo = PicoBidirPin::new(pins.gpio5.into_floating_input());
        let tdi = PicoBidirPin::new(pins.gpio6.into_floating_input());
        let trst = PicoBidirPin::new(pins.gpio7.into_floating_input());
        let srst = PicoBidirPin::new(pins.gpio4.into_floating_input());
        rust_dap_rp::bitbang::JtagIoSet::new(tck, tms, tdi, tdo, trst, srst, CortexMDelay)
    };

    // SWJ switches between SWD and JTAG at runtime, so it intentionally
    // uses the common bit-banging transport.
    #[cfg(feature = "swj")]
    let transport = {
        use rust_dap_rp::bitbang::{CortexMDelay, PicoBidirPin};
        let clk = PicoBidirPin::new(pins.gpio2.into_floating_input());
        let dio = PicoBidirPin::new(pins.gpio3.into_floating_input());
        let tdo = PicoBidirPin::new(pins.gpio5.into_floating_input());
        let tdi = PicoBidirPin::new(pins.gpio6.into_floating_input());
        let trst = PicoBidirPin::new(pins.gpio7.into_floating_input());
        let srst = PicoBidirPin::new(pins.gpio4.into_floating_input());
        rust_dap_rp::bitbang::SwjIoSet::new(clk, dio, tdi, tdo, trst, srst, CortexMDelay)
    };

    let (mut usb_serial, mut usb_dap, mut usb_bus) = rust_dap_rp::util::initialize_usb::<_, 64>(
        transport,
        &usb_allocator,
        UsbIdentity {
            serial: "raspberry-pi-pico-2",
            ..UsbIdentity::default()
        },
        DapConfig::new(
            DapIdentity {
                serial_number: "raspberry-pi-pico-2",
                product_firmware_version: env!("GIT_REV"),
                ..DapIdentity::default()
            },
            clocks.system_clock.freq().to_Hz(),
        ),
    );

    let mut led = pins.gpio25.into_push_pull_output();
    loop {
        if usb_bus.poll(&mut [&mut usb_serial, &mut usb_dap]) {
            usb_dap.process().ok();
            led.toggle().ok();
        }

        bridge::drain_usb_to_uart_tx(&mut usb_serial, &mut uart_writer);
        bridge::drain_uart_rx_to_usb(&mut usb_serial, &mut uart_reader);

        if let Ok(expected_config) = UartConfig::try_from(usb_serial.line_coding()) {
            if expected_config != uart_config.config {
                bridge::reconfigure_uart(
                    &mut uart_reader,
                    &mut uart_writer,
                    &mut uart_config,
                    &expected_config,
                );
            }
        }
    }
}
