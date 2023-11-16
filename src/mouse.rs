use crate::{
    cursor,
    usb::{
        EndpointAddress, Pid, TransferType, UsbDescriptor, UsbDevice, UsbPacket, UsbServiceHandle,
    },
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
        let endpoint = find_appropriate_endpoint(&self.device, &self.usb_tx)
            .await
            .expect("Failed to find endpoint");

        let mut data_toggle = false;
        loop {
            let read_packet = UsbPacket {
                // FIXME: Set address and endpoint
                address: self.device.address,
                endpoint,
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

async fn find_appropriate_endpoint(
    device: &UsbDevice,
    usb_service: &UsbServiceHandle,
) -> Option<u8> {
    let configurations = usb_service
        .get_configuration_descriptors(device.address)
        .await;

    let mut in_relevant_interface = false;
    for descriptor in &configurations {
        if let UsbDescriptor::Interface(intf) = &descriptor {
            in_relevant_interface = intf.interface_class() == 3
                && intf.interface_subclass() == 1
                && intf.interface_protocol() == 2;
        }

        if !in_relevant_interface {
            continue;
        }

        let endpoint = match descriptor {
            UsbDescriptor::Endpoint(endpoint) => endpoint,
            _ => continue,
        };

        if endpoint.transfer_type() != TransferType::Interrupt {
            continue;
        }

        if let EndpointAddress::In(address) = endpoint.endpoint_address() {
            return Some(address);
        }
    }

    None
}
