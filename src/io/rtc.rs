use crate::{
    interrupts::InterruptHandlerData,
    io::{Port, PortManager},
    util::interrupt_guard::InterruptGuarded,
};
use alloc::sync::Arc;

use thiserror_no_std::Error;

const NMI_ENABLE: bool = true;

#[derive(Debug)]
pub struct DateTime {
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub weekday: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub century: u8,
}

fn get_nmi_mask(nmi_enable: bool) -> u8 {
    if nmi_enable {
        0
    } else {
        1 << 7
    }
}

fn select_reg(control_port: &mut Port, nmi_enable: bool, reg: u8) {
    control_port.writeb(get_nmi_mask(nmi_enable) | reg);
}

fn read_cmos_reg(ports: &mut Ports, nmi_enable: bool, reg: u8) -> u8 {
    select_reg(&mut ports.cmos_nmi_control_port, nmi_enable, reg);
    ports.cmos_data_port.readb()
}

fn write_cmos_reg(ports: &mut Ports, nmi_enable: bool, reg: u8, val: u8) {
    select_reg(&mut ports.cmos_nmi_control_port, nmi_enable, reg);
    ports.cmos_data_port.writeb(val);
}

fn update_in_progress(ports: &mut Ports, nmi_enable: bool) -> bool {
    const STATUS_REG_A_NUM: u8 = 0x0a;
    select_reg(
        &mut ports.cmos_nmi_control_port,
        nmi_enable,
        STATUS_REG_A_NUM,
    );
    in_progress_set(ports.cmos_data_port.readb())
}

fn in_progress_set(status_reg_a: u8) -> bool {
    const IN_PROGRESS_MASK: u8 = 1 << 7;
    status_reg_a & IN_PROGRESS_MASK == IN_PROGRESS_MASK
}

fn enable_interrupts(ports: &mut Ports) {
    let prev = read_cmos_reg(ports, NMI_ENABLE, 0x8b);
    write_cmos_reg(ports, NMI_ENABLE, 0x8b, prev | 0x40);
}

fn set_interrupt_rate(ports: &mut Ports) {
    // See MC146818 docs for rate control, set up 256 Hz
    let mut data = read_cmos_reg(ports, NMI_ENABLE, 0x0a);
    data = (data & 0xf0) | 1;
    write_cmos_reg(ports, NMI_ENABLE, 0x0a, data);
}

fn set_data_format(ports: &mut Ports, nmi_enable: bool) {
    const STATUS_REG_B_NUM: u8 = 0x0b;
    let mut status_reg = read_cmos_reg(ports, nmi_enable, STATUS_REG_B_NUM);
    status_reg |= 1 << 1; // Enables 24 hour mode
    status_reg |= 1 << 2; // Enables binary format of retrieved values

    write_cmos_reg(ports, nmi_enable, STATUS_REG_B_NUM, status_reg);
}

fn clear_interrupt_mask(ports: &mut Ports) {
    read_cmos_reg(ports, NMI_ENABLE, 0x0c);
}

fn update_guarded_op<R, F: Fn(&mut Ports) -> R>(ports: &mut Ports, f: F) -> R {
    let mut ret;
    loop {
        while update_in_progress(ports, NMI_ENABLE) {
            continue;
        }

        ret = f(ports);

        if update_in_progress(ports, NMI_ENABLE) {
            continue;
        }

        break;
    }

    ret
}

#[derive(Debug, Error)]
pub enum RtcInitError {
    #[error("failed to get control port")]
    FailedToGetControlPort,
    #[error("failed to get data port")]
    FailedToGetDataPort,
}

struct Ports {
    cmos_nmi_control_port: Port,
    cmos_data_port: Port,
}

impl Ports {
    fn new(port_manager: &mut PortManager) -> Result<Ports, RtcInitError> {
        let cmos_nmi_control_port = port_manager
            .request_port(0x70)
            .ok_or(RtcInitError::FailedToGetControlPort)?;
        let cmos_data_port = port_manager
            .request_port(0x71)
            .ok_or(RtcInitError::FailedToGetDataPort)?;

        Ok(Ports {
            cmos_nmi_control_port,
            cmos_data_port,
        })
    }
}

pub struct Rtc {
    ports: Arc<InterruptGuarded<Ports>>,
}

impl Rtc {
    pub fn new<F: Fn() + 'static>(
        port_manager: &mut PortManager,
        interrupt_handlers: &InterruptHandlerData,
        on_tick: F,
    ) -> Result<Rtc, RtcInitError> {
        let interrupt_guard = InterruptGuarded::new(());
        let interrupt_guard = interrupt_guard.lock();

        let mut ports = Ports::new(port_manager)?;

        set_data_format(&mut ports, NMI_ENABLE);
        set_interrupt_rate(&mut ports);
        enable_interrupts(&mut ports);

        let ports = Arc::new(InterruptGuarded::new(ports));

        interrupt_handlers.register(crate::interrupts::IrqId::Pic2(0), {
            let ports = Arc::clone(&ports);
            move || {
                on_tick();
                clear_interrupt_mask(&mut ports.lock());
            }
        });

        drop(interrupt_guard);

        Ok(Rtc { ports })
    }

    pub fn write(&mut self, date_time: &DateTime) {
        update_guarded_op(&mut self.ports.lock(), |ports| {
            write_cmos_reg(ports, NMI_ENABLE, 0x00, date_time.seconds);
            write_cmos_reg(ports, NMI_ENABLE, 0x02, date_time.minutes);
            write_cmos_reg(ports, NMI_ENABLE, 0x04, date_time.hours);
            write_cmos_reg(ports, NMI_ENABLE, 0x06, date_time.weekday);
            write_cmos_reg(ports, NMI_ENABLE, 0x07, date_time.day);
            write_cmos_reg(ports, NMI_ENABLE, 0x08, date_time.month);
            write_cmos_reg(ports, NMI_ENABLE, 0x09, date_time.year);
            write_cmos_reg(ports, NMI_ENABLE, 0x32, date_time.century);
        });
    }

    pub fn read(&mut self) -> DateTime {
        update_guarded_op(&mut self.ports.lock(), |ports| {
            let seconds = read_cmos_reg(ports, NMI_ENABLE, 0x00);
            let minutes = read_cmos_reg(ports, NMI_ENABLE, 0x02);
            let hours = read_cmos_reg(ports, NMI_ENABLE, 0x04);
            let weekday = read_cmos_reg(ports, NMI_ENABLE, 0x06);
            let day = read_cmos_reg(ports, NMI_ENABLE, 0x07);
            let month = read_cmos_reg(ports, NMI_ENABLE, 0x08);
            let year = read_cmos_reg(ports, NMI_ENABLE, 0x09);
            let century = read_cmos_reg(ports, NMI_ENABLE, 0x32);

            DateTime {
                seconds,
                minutes,
                hours,
                weekday,
                day,
                month,
                year,
                century,
            }
        })
    }

    pub fn tick_freq() -> f32 {
        256.0
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(test_in_progress_flag, {
        test_true!(in_progress_set(1 << 7));
        test_false!(in_progress_set(0x34));
        test_true!(in_progress_set((1 << 7) | 0x34));
        Ok(())
    });
}
