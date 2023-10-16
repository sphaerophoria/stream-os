use crate::{
    interrupts::{InterruptHandlerData, InterruptHandlerRegisterError},
    io::{IoAllocator, IoOffset, IoRange},
    util::interrupt_guard::InterruptGuarded,
};
use alloc::sync::Arc;

use super::io_allocator::OffsetOutOfRange;

const NMI_ENABLE: bool = true;
const CONTROL_OFFEST: IoOffset = IoOffset::new(0);
const DATA_OFFEST: IoOffset = IoOffset::new(1);

#[derive(Debug)]
pub enum ReadError {
    Seconds(OffsetOutOfRange),
    Minutes(OffsetOutOfRange),
    Hours(OffsetOutOfRange),
    Weekday(OffsetOutOfRange),
    Day(OffsetOutOfRange),
    Month(OffsetOutOfRange),
    Year(OffsetOutOfRange),
    Century(OffsetOutOfRange),
}

#[derive(Debug)]
pub enum WriteError {
    Seconds(OffsetOutOfRange),
    Minutes(OffsetOutOfRange),
    Hours(OffsetOutOfRange),
    Weekday(OffsetOutOfRange),
    Day(OffsetOutOfRange),
    Month(OffsetOutOfRange),
    Year(OffsetOutOfRange),
    Century(OffsetOutOfRange),
}

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

fn select_reg(cmos_io: &mut IoRange, nmi_enable: bool, reg: u8) -> Result<(), OffsetOutOfRange> {
    cmos_io.write_u8(CONTROL_OFFEST, get_nmi_mask(nmi_enable) | reg)
}

fn read_cmos_reg(cmos_io: &mut IoRange, nmi_enable: bool, reg: u8) -> Result<u8, OffsetOutOfRange> {
    select_reg(cmos_io, nmi_enable, reg)?;
    cmos_io.read_u8(DATA_OFFEST)
}

fn write_cmos_reg(
    cmos_io: &mut IoRange,
    nmi_enable: bool,
    reg: u8,
    val: u8,
) -> Result<(), OffsetOutOfRange> {
    select_reg(cmos_io, nmi_enable, reg)?;
    cmos_io.write_u8(DATA_OFFEST, val)
}

fn update_in_progress(cmos_io: &mut IoRange, nmi_enable: bool) -> Result<bool, OffsetOutOfRange> {
    const STATUS_REG_A_NUM: u8 = 0x0a;
    select_reg(cmos_io, nmi_enable, STATUS_REG_A_NUM)?;
    Ok(in_progress_set(cmos_io.read_u8(DATA_OFFEST)?))
}

fn in_progress_set(status_reg_a: u8) -> bool {
    const IN_PROGRESS_MASK: u8 = 1 << 7;
    status_reg_a & IN_PROGRESS_MASK == IN_PROGRESS_MASK
}

fn enable_interrupts(cmos_io: &mut IoRange) -> Result<(), OffsetOutOfRange> {
    let prev = read_cmos_reg(cmos_io, NMI_ENABLE, 0x8b)?;
    write_cmos_reg(cmos_io, NMI_ENABLE, 0x8b, prev | 0x40)
}

fn set_interrupt_rate(cmos_io: &mut IoRange) -> Result<(), OffsetOutOfRange> {
    // See MC146818 docs for rate control, set up 256 Hz
    let mut data = read_cmos_reg(cmos_io, NMI_ENABLE, 0x0a)?;
    data = (data & 0xf0) | 1;
    write_cmos_reg(cmos_io, NMI_ENABLE, 0x0a, data)?;
    Ok(())
}

fn set_data_format(cmos_io: &mut IoRange, nmi_enable: bool) -> Result<(), OffsetOutOfRange> {
    const STATUS_REG_B_NUM: u8 = 0x0b;
    let mut status_reg = read_cmos_reg(cmos_io, nmi_enable, STATUS_REG_B_NUM)?;
    status_reg |= 1 << 1; // Enables 24 hour mode
    status_reg |= 1 << 2; // Enables binary format of retrieved values

    write_cmos_reg(cmos_io, nmi_enable, STATUS_REG_B_NUM, status_reg)
}

fn clear_interrupt_mask(cmos_io: &mut IoRange) -> Result<(), OffsetOutOfRange> {
    read_cmos_reg(cmos_io, NMI_ENABLE, 0x0c)?;
    Ok(())
}

fn update_guarded_op<R, F: Fn(&mut IoRange) -> R>(cmos_io: &mut IoRange, f: F) -> R {
    let mut ret;
    loop {
        while update_in_progress(cmos_io, NMI_ENABLE).unwrap() {
            continue;
        }

        ret = f(cmos_io);

        if update_in_progress(cmos_io, NMI_ENABLE).unwrap() {
            continue;
        }

        break;
    }

    ret
}

#[derive(Debug)]
pub enum RtcInitError {
    RequestIoRange,
    SetDataFormat(OffsetOutOfRange),
    SetInterruptRate(OffsetOutOfRange),
    EnableInterrupts(OffsetOutOfRange),
    RegisterInterruptHandler(InterruptHandlerRegisterError),
}

pub struct Rtc {
    cmos_io: Arc<InterruptGuarded<IoRange>>,
}

impl Rtc {
    pub fn new<F: FnMut() + 'static>(
        io_allocator: &mut IoAllocator,
        interrupt_handlers: &InterruptHandlerData,
        mut on_tick: F,
    ) -> Result<Rtc, RtcInitError> {
        let interrupt_guard = InterruptGuarded::new(());
        let interrupt_guard = interrupt_guard.lock();

        let mut cmos_io = io_allocator
            .request_io_range(0x70, 2)
            .ok_or(RtcInitError::RequestIoRange)?;

        set_data_format(&mut cmos_io, NMI_ENABLE).map_err(RtcInitError::SetDataFormat)?;
        set_interrupt_rate(&mut cmos_io).map_err(RtcInitError::SetInterruptRate)?;
        enable_interrupts(&mut cmos_io).map_err(RtcInitError::EnableInterrupts)?;

        let cmos_io = Arc::new(InterruptGuarded::new(cmos_io));

        interrupt_handlers
            .register(crate::interrupts::IrqId::Pic2(0), {
                let cmos_io = Arc::clone(&cmos_io);
                move || {
                    on_tick();
                    if let Err(e) = clear_interrupt_mask(&mut cmos_io.lock()) {
                        error!("Failed to clear interrupt mask: {:?}", e);
                    }
                }
            })
            .map_err(RtcInitError::RegisterInterruptHandler)?;

        drop(interrupt_guard);

        Ok(Rtc { cmos_io })
    }

    pub fn write(&mut self, date_time: &DateTime) -> Result<(), WriteError> {
        update_guarded_op(&mut self.cmos_io.lock(), |cmos_io| {
            write_cmos_reg(cmos_io, NMI_ENABLE, 0x00, date_time.seconds)
                .map_err(WriteError::Seconds)?;
            write_cmos_reg(cmos_io, NMI_ENABLE, 0x02, date_time.minutes)
                .map_err(WriteError::Minutes)?;
            write_cmos_reg(cmos_io, NMI_ENABLE, 0x04, date_time.hours)
                .map_err(WriteError::Hours)?;
            write_cmos_reg(cmos_io, NMI_ENABLE, 0x06, date_time.weekday)
                .map_err(WriteError::Weekday)?;
            write_cmos_reg(cmos_io, NMI_ENABLE, 0x07, date_time.day).map_err(WriteError::Day)?;
            write_cmos_reg(cmos_io, NMI_ENABLE, 0x08, date_time.month)
                .map_err(WriteError::Month)?;
            write_cmos_reg(cmos_io, NMI_ENABLE, 0x09, date_time.year).map_err(WriteError::Year)?;
            write_cmos_reg(cmos_io, NMI_ENABLE, 0x32, date_time.century)
                .map_err(WriteError::Century)?;
            Ok(())
        })
    }

    pub fn read(&mut self) -> Result<DateTime, ReadError> {
        update_guarded_op(&mut self.cmos_io.lock(), |cmos_io| {
            let seconds = read_cmos_reg(cmos_io, NMI_ENABLE, 0x00).map_err(ReadError::Seconds)?;
            let minutes = read_cmos_reg(cmos_io, NMI_ENABLE, 0x02).map_err(ReadError::Minutes)?;
            let hours = read_cmos_reg(cmos_io, NMI_ENABLE, 0x04).map_err(ReadError::Hours)?;
            let weekday = read_cmos_reg(cmos_io, NMI_ENABLE, 0x06).map_err(ReadError::Weekday)?;
            let day = read_cmos_reg(cmos_io, NMI_ENABLE, 0x07).map_err(ReadError::Day)?;
            let month = read_cmos_reg(cmos_io, NMI_ENABLE, 0x08).map_err(ReadError::Month)?;
            let year = read_cmos_reg(cmos_io, NMI_ENABLE, 0x09).map_err(ReadError::Year)?;
            let century = read_cmos_reg(cmos_io, NMI_ENABLE, 0x32).map_err(ReadError::Century)?;

            Ok(DateTime {
                seconds,
                minutes,
                hours,
                weekday,
                day,
                month,
                year,
                century,
            })
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
