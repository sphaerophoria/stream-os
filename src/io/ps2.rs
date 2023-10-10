use crate::{
    future::wakeup_executor,
    interrupts::{InterruptHandlerData, IrqId},
    io::io_allocator::{IoAllocator, IoOffset, IoRange},
    util::{
        bit_manipulation::{GetBits, SetBits},
        interrupt_guard::InterruptGuarded,
    },
};

pub struct Ps2Keyboard {
    data: IoRange,
    command: IoRange,
}

impl Ps2Keyboard {
    pub fn new(
        io_allocator: &mut IoAllocator,
        interrupt_handlers: &InterruptHandlerData,
    ) -> Ps2Keyboard {
        let _interrupt_guard = InterruptGuarded::new(());
        let _interrupt_guard = _interrupt_guard.lock();

        let mut data = io_allocator.request_io_range(0x60, 1).unwrap();
        // Also status
        let mut command = io_allocator.request_io_range(0x64, 1).unwrap();

        disable_ps2(&mut command);
        let _ = read_output_buffer(&mut data);
        initialize_config_reg(&mut command, &mut data);
        enable_ps2(&mut command);
        reset_devices(&mut command, &mut data);

        interrupt_handlers
            .register(IrqId::Pic1(1), wakeup_executor)
            .unwrap();

        // FIXME: run initialization steps
        Ps2Keyboard { data, command }
    }

    pub async fn read(&mut self) -> u8 {
        PollReadFut {
            status: &mut self.command,
        }
        .await;
        self.data.read_u8(IoOffset::new(0)).unwrap()
    }
}

struct PollReadFut<'a> {
    status: &'a mut IoRange,
}

impl core::future::Future for PollReadFut<'_> {
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let status = self.status.read_u8(IoOffset::new(0)).unwrap();
        if status.get_bit(0) {
            core::task::Poll::Ready(())
        } else {
            core::task::Poll::Pending
        }
    }
}

fn read_output_buffer(data: &mut IoRange) -> u8 {
    data.read_u8(IoOffset::new(0))
        .expect("unexpected out of range")
}

fn disable_ps2(command: &mut IoRange) {
    command
        .write_u8(IoOffset::new(0), 0xAD)
        .expect("unexpected out of range");
    command
        .write_u8(IoOffset::new(0), 0xA7)
        .expect("unexpected out of range");
}

fn enable_ps2(command: &mut IoRange) {
    command
        .write_u8(IoOffset::new(0), 0xAE)
        .expect("unexpected out of range");
    //command.write_u8(IoOffset::new(0), 0xA8).expect("unexpected out of range");
}

fn reset_devices(command: &mut IoRange, data: &mut IoRange) {
    command
        .write_u8(IoOffset::new(0), 0xD1)
        .expect("unexpected out of range");
    data.write_u8(IoOffset::new(0), 0xFF)
        .expect("unexpected out of range");
}

fn initialize_config_reg(command: &mut IoRange, data: &mut IoRange) {
    command
        .write_u8(IoOffset::new(0), 0x20)
        .expect("unexpected out of range");
    let mut config_reg_value = data
        .read_u8(IoOffset::new(0))
        .expect("unexpected out of range");
    config_reg_value.set_bit(0, true);
    config_reg_value.set_bit(1, false);
    config_reg_value.set_bit(6, false);
}
