use crate::io::port_manager::{Port, PortManager};

use thiserror_no_std::Error;

const BASE_ADDR: u16 = 0x3f8;

#[derive(Debug, Error)]
pub enum SerialInitError {
    #[error("data port reserved")]
    DataReserved,
    #[error("enable interrupt port reserved")]
    EnableInterruptReserved,
    #[error("interrupt id port reserved")]
    InterruptIdReserved,
    #[error("line control port reserved")]
    LineControlReserved,
    #[error("modem control port reserved")]
    ModemControlReserved,
    #[error("line status port reserved")]
    LineStatusReserved,
    #[error("modem status port reserved")]
    ModemStatusReserved,
    #[error("scratch port reserved")]
    ScratchReserved,
    #[error("loopback test failed")]
    Loopback,
}

pub struct Serial {
    data: Port,
    _enable_interrupt: Port,
    _interrupt_id_fifo_control: Port,
    _line_control: Port,
    _modem_control: Port,
    line_status: Port,
    _modem_status: Port,
    _scratch: Port,
}

impl Serial {
    pub fn new(port_manager: &mut PortManager) -> Result<Serial, SerialInitError> {
        use SerialInitError::*;

        let mut data = port_manager.request_port(BASE_ADDR).ok_or(DataReserved)?;
        let mut enable_interrupt = port_manager
            .request_port(BASE_ADDR + 1)
            .ok_or(EnableInterruptReserved)?;
        let mut interrupt_id_fifo_control = port_manager
            .request_port(BASE_ADDR + 2)
            .ok_or(InterruptIdReserved)?;
        let mut line_control = port_manager
            .request_port(BASE_ADDR + 3)
            .ok_or(LineControlReserved)?;
        let mut modem_control = port_manager
            .request_port(BASE_ADDR + 4)
            .ok_or(ModemControlReserved)?;
        let line_status = port_manager
            .request_port(BASE_ADDR + 5)
            .ok_or(LineStatusReserved)?;
        let modem_status = port_manager
            .request_port(BASE_ADDR + 6)
            .ok_or(ModemStatusReserved)?;
        let scratch = port_manager
            .request_port(BASE_ADDR + 7)
            .ok_or(ScratchReserved)?;

        enable_interrupt.writeb(0x00); // Disable all interrupts
        line_control.writeb(0x80); // Enable DLAB (set baud rate divisor)
        data.writeb(0x03); // Set divisor to 3 (lo byte) 38400 baud
        enable_interrupt.writeb(0x00); //                  (hi byte)
        line_control.writeb(0x03); // 8 bits, no parity, one stop bit
        interrupt_id_fifo_control.writeb(0xC7); // Enable FIFO, clear them, with 14-byte threshold
        modem_control.writeb(0x0B); // IRQs enabled, RTS/DSR set
        modem_control.writeb(0x1E); // Set in loopback mode, test the serial chip
        data.writeb(0xAE); // Test serial chip (send byte 0xAE and check if serial returns same byte)

        // Check if serial is faulty (i.e: not same byte as sent)
        if data.readb() != 0xAE {
            return Err(Loopback);
        }

        // If serial is not faulty set it in normal operation mode
        // (not-loopback with IRQs enabled and OUT#1 and OUT#2 bits enabled)
        modem_control.writeb(0x0F);

        Ok(Serial {
            data,
            _enable_interrupt: enable_interrupt,
            _interrupt_id_fifo_control: interrupt_id_fifo_control,
            _line_control: line_control,
            _modem_control: modem_control,
            line_status,
            _modem_status: modem_status,
            _scratch: scratch,
        })
    }

    fn is_transmit_empty(&mut self) -> u8 {
        self.line_status.readb() & 0x20
    }

    fn write_byte(&mut self, a: u8) {
        while self.is_transmit_empty() == 0 {}

        self.data.writeb(a);
    }
}

impl core::fmt::Write for Serial {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.as_bytes() {
            self.write_byte(*b)
        }
        Ok(())
    }
}
