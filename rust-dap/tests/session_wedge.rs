//! Regression test: a new host session must be able to talk to the probe
//! even if a previous session left an un-sent response buffered.
//!
//! Background: `CmsisDap::process()` keeps a single pending-response slot
//! (`next_in`). If a host writes a command but goes away before reading the
//! response (killed session, an accidental second `probe-rs`/gdb on the same
//! probe, etc.), that response stays buffered and could never be delivered.
//! CMSIS-DAP is strict request/response, so when a *new* command arrives
//! while a response is still buffered, the previous host must have abandoned
//! it — the firmware has to service the new command rather than head-of-line
//! block on the stale one. Before the fix it blocked, which showed up on the
//! host as "Could not determine a suitable packet size" (probe wedged until a
//! USB bus reset / replug).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use rust_dap::{
    ActivePort, CmsisDap, ConnectPort, DapCapabilities, DapConfig, DapError, DapIdentity,
    DapTransport, SwjPins,
};
use usb_device::bus::{PollResult, UsbBus, UsbBusAllocator};
use usb_device::device::{UsbDeviceBuilder, UsbVidPid};
use usb_device::endpoint::{EndpointAddress, EndpointType};
use usb_device::{Result as UsbResult, UsbDirection, UsbError};

const MPS: usize = 64;

// ---- controllable mock USB bus ----

#[derive(Default)]
struct BusState {
    /// OUT packets waiting to be read by the firmware (host → device).
    out_queue: VecDeque<Vec<u8>>,
    /// IN packets the firmware has written (device → host), in order.
    in_sent: Vec<Vec<u8>>,
    /// When true, IN writes fail with WouldBlock (host not draining).
    in_blocked: bool,
    ep_counter: u8,
}

#[derive(Clone)]
struct MockBus {
    state: Arc<Mutex<BusState>>,
}

impl MockBus {
    fn new(state: Arc<Mutex<BusState>>) -> Self {
        Self { state }
    }
}

impl UsbBus for MockBus {
    fn alloc_ep(
        &mut self,
        ep_dir: UsbDirection,
        _ep_addr: Option<EndpointAddress>,
        _ep_type: EndpointType,
        _max_packet_size: u16,
        _interval: u8,
    ) -> UsbResult<EndpointAddress> {
        let mut s = self.state.lock().unwrap();
        let idx = s.ep_counter as usize;
        s.ep_counter += 1;
        Ok(EndpointAddress::from_parts(idx, ep_dir))
    }

    fn enable(&mut self) {}
    fn reset(&self) {}
    fn set_device_address(&self, _addr: u8) {}

    fn write(&self, ep_addr: EndpointAddress, buf: &[u8]) -> UsbResult<usize> {
        if ep_addr.is_in() {
            let mut s = self.state.lock().unwrap();
            if s.in_blocked {
                return Err(UsbError::WouldBlock);
            }
            s.in_sent.push(buf.to_vec());
        }
        Ok(buf.len())
    }

    fn read(&self, ep_addr: EndpointAddress, buf: &mut [u8]) -> UsbResult<usize> {
        if ep_addr.is_out() {
            let mut s = self.state.lock().unwrap();
            if let Some(pkt) = s.out_queue.pop_front() {
                buf[..pkt.len()].copy_from_slice(&pkt);
                return Ok(pkt.len());
            }
        }
        Err(UsbError::WouldBlock)
    }

    fn set_stalled(&self, _ep_addr: EndpointAddress, _stalled: bool) {}
    fn is_stalled(&self, _ep_addr: EndpointAddress) -> bool {
        false
    }
    fn suspend(&self) {}
    fn resume(&self) {}
    fn poll(&self) -> PollResult {
        PollResult::None
    }
}

// ---- null transport (the test commands never touch the wire) ----

struct NullTransport;

impl DapTransport for NullTransport {
    fn capabilities(&self) -> DapCapabilities {
        DapCapabilities::empty()
    }
    fn connect(&mut self, _port: ConnectPort, _config: &DapConfig) -> Result<ActivePort, DapError> {
        Err(DapError::NotSupported)
    }
    fn disconnect(&mut self, _config: &DapConfig) -> Result<(), DapError> {
        Err(DapError::NotSupported)
    }
    fn swj_sequence(
        &mut self,
        _config: &DapConfig,
        _count: usize,
        _data: &[u8],
    ) -> Result<(), DapError> {
        Err(DapError::NotSupported)
    }
    fn swj_pins(
        &mut self,
        _config: &DapConfig,
        _output: SwjPins,
        _select: SwjPins,
        _wait_us: u32,
    ) -> Result<SwjPins, DapError> {
        Err(DapError::NotSupported)
    }
    fn swj_clock(&mut self, _config: &mut DapConfig, _frequency_hz: u32) -> Result<(), DapError> {
        Err(DapError::NotSupported)
    }
}

/// DAP_Info(Vendor) — a request whose response differs from PacketSize, so
/// a leftover response is distinguishable from the new command's response.
const DAP_INFO_VENDOR: &[u8] = &[0x00, 0x01];
/// DAP_Info(PacketSize) — response is `[0x00, mps_lo, mps_hi]`.
const DAP_INFO_PACKET_SIZE: &[u8] = &[0x00, 0xff];

fn packet_size_response() -> Vec<u8> {
    // DAP_Info reply: command byte, info length byte, then the info data.
    let mut r = vec![0x00, 0x02];
    r.extend_from_slice(&(MPS as u16).to_le_bytes());
    r
}

#[test]
fn new_command_is_serviced_despite_a_stale_buffered_response() {
    let state = Arc::new(Mutex::new(BusState::default()));
    let alloc = UsbBusAllocator::new(MockBus::new(state.clone()));
    let mut dap: CmsisDap<'_, MockBus, NullTransport, MPS> = CmsisDap::new(
        &alloc,
        NullTransport,
        DapConfig {
            identity: DapIdentity {
                vendor: "WEDGE-VENDOR",
                ..Default::default()
            },
            ..Default::default()
        },
    );
    // Building the device finalizes the allocator so the endpoints allocated
    // above become usable (otherwise reads/writes panic with "UsbBus
    // initialization not complete"). We drive `dap.process()` directly rather
    // than through `UsbDevice::poll`, so the device only needs to exist.
    let _usb_dev = UsbDeviceBuilder::new(&alloc, UsbVidPid(0x6666, 0x4444)).build();

    // Phase 1 — host A sends a command but never drains the IN endpoint
    // (killed / stalled). The response is produced but cannot be sent, so it
    // stays buffered.
    {
        let mut s = state.lock().unwrap();
        s.out_queue.push_back(DAP_INFO_VENDOR.to_vec());
        s.in_blocked = true;
    }
    dap.process().unwrap();
    assert!(
        state.lock().unwrap().in_sent.is_empty(),
        "host A's response should not have been sent (IN was blocked)"
    );

    // Phase 2 — a fresh host B connects (IN now drains) and asks for the
    // packet size. B must get *its* answer; the abandoned response from A must
    // not head-of-line block it.
    {
        let mut s = state.lock().unwrap();
        s.in_blocked = false;
        s.out_queue.push_back(DAP_INFO_PACKET_SIZE.to_vec());
    }
    dap.process().unwrap();

    let sent = state.lock().unwrap().in_sent.clone();
    assert_eq!(
        sent,
        vec![packet_size_response()],
        "host B must receive exactly its packet-size response, with the \
         abandoned response from host A discarded (got {sent:02x?})"
    );
}
