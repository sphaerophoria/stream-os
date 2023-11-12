use crate::{
    cursor,
    usb::{Pid, UsbDevice, UsbPacket, UsbServiceHandle},
    util::async_channel::Sender,
};

use alloc::vec;

const SENSITIVITY: f32 = 0.2 / 127.0;

pub struct Mouse {
    device: UsbDevice,
    usb_tx: UsbServiceHandle,
    pos_tx: Sender<cursor::Movement>,
}

impl Mouse {
    pub fn new(
        device: UsbDevice,
        usb_tx: UsbServiceHandle,
        pos_tx: Sender<cursor::Movement>,
    ) -> Mouse {
        Mouse {
            device,
            usb_tx,
            pos_tx,
        }
    }

    pub async fn service(&mut self) {
        let mut data_toggle = false;
        loop {
            let read_packet = UsbPacket {
                // FIXME: Set address and endpoint
                address: self.device.address,
                endpoint: 1,
                data_toggle,
                pid: Pid::In,
                data: vec![0; 8],
            };
            data_toggle = !data_toggle;
            let data = self.usb_tx.queue_work(vec![read_packet]).await;

            let x_movement = data[0][1] as i8;
            let x_movement = x_movement as f32 * SENSITIVITY;
            let y_movement = data[0][2] as i8;
            let y_movement = y_movement as f32 * SENSITIVITY;

            self.pos_tx
                .send(cursor::Movement {
                    x: x_movement,
                    y: y_movement,
                })
                .await;
        }
    }
}
