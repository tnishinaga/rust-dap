// Copyright 2026 Kenta Ida
//
// SPDX-License-Identifier: Apache-2.0

//! Polling USB-CDC <-> UART bridge for the RP2350 firmware.

use crate::hal;
use crate::line_coding::UartConfig;
use crate::util::{read_usb_serial_byte_cs, write_usb_serial_byte_cs, UartConfigAndClock};
use embedded_hal_nb::serial::{Read, Write};
use hal::pac::{UART0, UART1};
use hal::uart::{Enabled, Reader, UartDevice, UartPeripheral, ValidUartPinout, Writer};
use hal::usb::UsbBus;
use heapless::spsc::{Consumer, Producer};
use usbd_serial::SerialPort;

pub trait SplitUart: UartDevice + Sized {
    fn split<P: ValidUartPinout<Self>>(
        uart: UartPeripheral<Enabled, Self, P>,
    ) -> (Reader<Self, P>, Writer<Self, P>);
}

impl SplitUart for UART0 {
    fn split<P: ValidUartPinout<Self>>(
        uart: UartPeripheral<Enabled, Self, P>,
    ) -> (Reader<Self, P>, Writer<Self, P>) {
        uart.split()
    }
}

impl SplitUart for UART1 {
    fn split<P: ValidUartPinout<Self>>(
        uart: UartPeripheral<Enabled, Self, P>,
    ) -> (Reader<Self, P>, Writer<Self, P>) {
        uart.split()
    }
}

pub fn drain_usb_to_uart_tx<const N: usize>(
    usb_serial: &mut SerialPort<UsbBus>,
    uart_tx_producer: &mut Producer<u8, N>,
) {
    while uart_tx_producer.ready() {
        if let Ok(data) = read_usb_serial_byte_cs(usb_serial) {
            uart_tx_producer.enqueue(data).unwrap();
        } else {
            break;
        }
    }
}

pub fn drain_uart_tx_queue<D: UartDevice, P: ValidUartPinout<D>, const N: usize>(
    uart_writer: &mut Option<Writer<D, P>>,
    uart_tx_consumer: &mut Consumer<u8, N>,
) {
    let uart = uart_writer.as_mut().unwrap();
    while let Some(data) = uart_tx_consumer.peek() {
        if uart.write(*data).is_ok() {
            uart_tx_consumer.dequeue().unwrap();
        } else {
            break;
        }
    }
}

pub fn drain_uart_rx_to_queue<D: UartDevice, P: ValidUartPinout<D>, const N: usize>(
    uart_reader: &mut Option<Reader<D, P>>,
    uart_rx_producer: &mut Producer<u8, N>,
) {
    let uart = uart_reader.as_mut().unwrap();
    while uart_rx_producer.ready() {
        if let Ok(data) = uart.read() {
            uart_rx_producer.enqueue(data).unwrap();
        } else {
            break;
        }
    }
}

pub fn drain_uart_rx_queue<const N: usize>(
    usb_serial: &mut SerialPort<UsbBus>,
    uart_rx_consumer: &mut Consumer<u8, N>,
) {
    while let Some(data) = uart_rx_consumer.peek() {
        if write_usb_serial_byte_cs(usb_serial, *data).is_ok() {
            uart_rx_consumer.dequeue().unwrap();
        } else {
            break;
        }
    }
    usb_serial.flush().ok();
}

pub fn reconfigure_uart<D: UartDevice + SplitUart, P: ValidUartPinout<D>>(
    uart_reader: &mut Option<Reader<D, P>>,
    uart_writer: &mut Option<Writer<D, P>>,
    uart_config: &mut UartConfigAndClock,
    expected_config: &UartConfig,
) {
    let reader = uart_reader.take().unwrap();
    let writer = uart_writer.take().unwrap();
    let enabled = UartPeripheral::join(reader, writer)
        .disable()
        .enable(expected_config.into(), uart_config.clock)
        .unwrap();
    uart_config.config = *expected_config;
    let (new_reader, new_writer) = D::split(enabled);
    uart_reader.replace(new_reader);
    uart_writer.replace(new_writer);
}
