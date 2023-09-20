use core::task::Poll;

use crate::{
    future::wakeup_executor,
    interrupts::{InterruptHandlerData, InterruptHandlerRegisterError, IrqId},
    io::io_allocator::{IoAllocator, IoOffset, IoRange, OffsetOutOfRange},
};

const BASE_ADDR: u16 = 0x3f8;
const DATA_OFFSET: IoOffset = IoOffset::new(0);
const ENABLE_INTERRUPT_OFFSET: IoOffset = IoOffset::new(1);
const INTERRUPT_ID_FIFO_CONTROL_OFFSET: IoOffset = IoOffset::new(2);
const LINE_CONTROL_OFFSET: IoOffset = IoOffset::new(3);
const MODEM_CONTROL_OFFSET: IoOffset = IoOffset::new(4);
const LINE_STATUS_OFFSET: IoOffset = IoOffset::new(5);
const _MODEM_STATUS_OFFSET: IoOffset = IoOffset::new(6);
const _SCRATCH_OFFSET: IoOffset = IoOffset::new(7);

#[derive(Debug)]
pub enum SerialInitError {
    IoRangeReserved,
    RegisterInterrupt(InterruptHandlerRegisterError),
    WriteFailed(OffsetOutOfRange),
    Loopback,
}

#[derive(Debug)]
pub struct WriteError(OffsetOutOfRange);

pub struct Serial {
    serial_io: IoRange,
}

impl Serial {
    pub fn new(
        io_allocator: &mut IoAllocator,
        interrupt_handlers: &InterruptHandlerData,
    ) -> Result<Serial, SerialInitError> {
        use SerialInitError::*;

        let mut serial_io = io_allocator
            .request_io_range(BASE_ADDR, 8)
            .ok_or(IoRangeReserved)?;

        interrupt_handlers
            .register(IrqId::Pic1(4), {
                move || {
                    wakeup_executor();
                }
            })
            .map_err(RegisterInterrupt)?;

        (|| -> Result<(), OffsetOutOfRange> {
            serial_io.write_u8(ENABLE_INTERRUPT_OFFSET, 0x02)?; // Enable transmit empty
            serial_io.write_u8(LINE_CONTROL_OFFSET, 0x80)?; // Enable DLAB (set baud rate divisor)
            serial_io.write_u8(DATA_OFFSET, 0x00)?;
            serial_io.write_u8(ENABLE_INTERRUPT_OFFSET, 0x02)?;
            serial_io.write_u8(LINE_CONTROL_OFFSET, 0x03)?; // 8 bits, no parity, one stop bit
            serial_io.write_u8(INTERRUPT_ID_FIFO_CONTROL_OFFSET, 0xC7)?; // Enable FIFO, clear them, with 14-byte threshold
            serial_io.write_u8(MODEM_CONTROL_OFFSET, 0x0B)?; // IRQs enabled, RTS/DSR set
            serial_io.write_u8(MODEM_CONTROL_OFFSET, 0x1E)?; // Set in loopback mode, test the serial chip
            serial_io.write_u8(DATA_OFFSET, 0xAE)?; // Test serial chip (send byte 0xAE and check if serial returns same byte)
            Ok(())
        })()
        .map_err(WriteFailed)?;

        // Check if serial is faulty (i.e: not same byte as sent)
        if serial_io.read_u8(DATA_OFFSET).map_err(WriteFailed)? != 0xAE {
            return Err(Loopback);
        }

        // If serial is not faulty set it in normal operation mode
        // (not-loopback with IRQs enabled and OUT#1 and OUT#2 bits enabled)
        serial_io
            .write_u8(MODEM_CONTROL_OFFSET, 0x0F)
            .map_err(WriteFailed)?;
        Ok(Serial { serial_io })
    }

    #[allow(unused)]
    async fn wait_transmit_empty(&mut self) {
        if is_transmit_ready(&mut self.serial_io) {
            return;
        }
        let waiter = TransmitEmptyWaiter {
            serial_io: &mut self.serial_io,
        };
        waiter.await;
    }

    fn write_byte(&mut self, a: u8) -> Result<(), WriteError> {
        while !is_transmit_ready(&mut self.serial_io) {}

        self.serial_io
            .write_u8(DATA_OFFSET, a)
            .map_err(WriteError)?;

        Ok(())
    }

    pub async fn write_str(&mut self, s: &str) -> Result<(), WriteError> {
        for b in s.as_bytes() {
            self.wait_transmit_empty().await;
            self.write_byte(*b)?;
        }

        Ok(())
    }
}

fn is_transmit_ready(serial_io: &mut IoRange) -> bool {
    (serial_io
        .read_u8(LINE_STATUS_OFFSET)
        .expect("line status not allocated")
        & 0x20)
        != 0
}

struct TransmitEmptyWaiter<'a> {
    serial_io: &'a mut IoRange,
}

impl core::future::Future for TransmitEmptyWaiter<'_> {
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        if is_transmit_ready(self.serial_io) {
            return Poll::Ready(());
        }

        Poll::Pending
    }
}

impl core::fmt::Write for Serial {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.as_bytes() {
            self.write_byte(*b).expect("failed to write to serial");
        }
        Ok(())
    }
}
