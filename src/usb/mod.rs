pub mod uhci;

use crate::{
    io::io_allocator::IoOffset,
    util::{
        async_channel::{self, Receiver, Sender},
        bit_manipulation::GetBits,
        oneshot::{self, Sender as OneshotSender},
    },
};

use uhci::Uhci;

use core::fmt;

use alloc::{vec, vec::Vec};

#[derive(Clone, Copy)]
pub enum Pid {
    Setup,
    In,
    Out,
}

pub struct UsbDevice {
    pub address: u8,
}

#[derive(Clone)]
pub struct UsbPacket {
    pub pid: Pid,
    pub address: u8,
    pub endpoint: u8,
    pub data_toggle: bool,
    pub data: Vec<u8>,
}

impl UsbPacket {
    pub fn setup(params: UsbSetupRequestParams) -> UsbPacket {
        let mut data = vec![0; UsbSetupRequest::SIZE];
        {
            let mut data = UsbSetupRequest(&mut data);
            data.set_request_type(params.request_type);
            data.set_request(params.request);
            data.set_value(params.value);
            data.set_index(params.index);
            data.set_length(params.length);
        }

        UsbPacket {
            pid: params.pid,
            address: params.address,
            endpoint: params.endpoint,
            data_toggle: params.data_toggle,
            data,
        }
    }
}

pub struct ConfigurationDescriptors(Vec<u8>);

impl ConfigurationDescriptors {
    pub fn iter(&self) -> UsbDescriptorIterator<'_> {
        UsbDescriptorIterator(&self.0)
    }
}

impl<'a> IntoIterator for &'a ConfigurationDescriptors {
    type Item = UsbDescriptor<'a>;

    type IntoIter = UsbDescriptorIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub type UsbPacketRequest = (Vec<UsbPacket>, OneshotSender<Vec<Vec<u8>>>);

#[derive(Clone)]
pub struct UsbServiceHandle {
    tx: Sender<UsbPacketRequest>,
}

impl UsbServiceHandle {
    pub async fn queue_work(&self, work: Vec<UsbPacket>) -> Vec<Vec<u8>> {
        let (tx, rx) = oneshot::channel();
        self.tx.send((work, tx)).await;
        rx.recv().await.expect("Received oneshot twice")
    }

    #[allow(unused)]
    pub async fn get_device_descriptor(&self, address: u8) -> UsbDeviceDescriptor<Vec<u8>> {
        const DESCRIPTOR_LENGTH: u16 = 18;
        let setup = UsbSetupRequestParams {
            pid: Pid::Setup,
            address,
            endpoint: 0,
            data_toggle: false,
            request_type: 0x80,
            request: UsbSetupRequestValue::GetDescriptor,
            value: (UsbDescriptorType::Device as u16) << 8,
            index: 0,
            length: DESCRIPTOR_LENGTH,
        };

        let setup = UsbPacket::setup(setup);

        let read = UsbPacket {
            pid: Pid::In,
            address,
            endpoint: 0,
            data_toggle: false,
            data: vec![0; DESCRIPTOR_LENGTH as usize],
        };

        let ack = UsbPacket {
            pid: Pid::Out,
            address,
            endpoint: 0,
            data_toggle: false,
            data: vec![],
        };

        let mut response = self.queue_work(vec![setup, read, ack]).await;
        UsbDeviceDescriptor(response.remove(1))
    }

    pub async fn get_configuration_descriptors(&self, address: u8) -> ConfigurationDescriptors {
        const DESCRIPTOR_LENGTH: u16 = 9;

        let work = generate_get_configuration_descriptor(address, DESCRIPTOR_LENGTH);
        let mut response = self.queue_work(work).await;

        let response = UsbConfigurationDescriptor(response.remove(1));
        let total_length = response.total_length();

        let work = generate_get_configuration_descriptor(address, total_length);
        let mut response = self.queue_work(work).await;
        ConfigurationDescriptors(response.remove(1))
    }
}

pub struct Usb {
    next_usb_addr: u8,
    uhci: Uhci,
    device_rx: Option<Receiver<UsbDevice>>,
    device_tx: Sender<UsbDevice>,
    packet_rx: Receiver<UsbPacketRequest>,
    packet_tx: Sender<UsbPacketRequest>,
}

impl Usb {
    pub fn new(uhci: Uhci) -> Usb {
        let (device_tx, device_rx) = async_channel::channel();
        let (packet_tx, packet_rx) = async_channel::channel();

        Usb {
            next_usb_addr: 1,
            uhci,
            device_rx: Some(device_rx),
            device_tx,
            packet_tx,
            packet_rx,
        }
    }

    pub fn device_channel(&mut self) -> Receiver<UsbDevice> {
        core::mem::take(&mut self.device_rx).expect("Can only run device_channel one time")
    }

    pub fn handle(&self) -> UsbServiceHandle {
        let tx = self.packet_tx.clone();
        UsbServiceHandle { tx }
    }

    async fn set_address(&mut self, address: u8) {
        let address_params = UsbSetupRequestParams {
            pid: Pid::Setup,
            address: 0,
            endpoint: 0,
            data_toggle: false,
            request_type: 0,
            request: UsbSetupRequestValue::SetAddress,
            value: address as u16,
            index: 0,
            length: 0,
        };

        let packet = UsbPacket::setup(address_params);

        let ack = UsbPacket {
            pid: Pid::In,
            address: 0,
            endpoint: 0,
            data_toggle: true,
            data: vec![],
        };

        let work = vec![packet, ack];

        self.queue_work(work).await;
    }

    async fn set_configuration(&mut self, address: u8, config: u8) {
        let set_configuration_params = UsbSetupRequestParams {
            pid: Pid::Setup,
            address,
            endpoint: 0,
            data_toggle: false,
            request_type: 0,
            request: UsbSetupRequestValue::SetConfiguration,
            value: config as u16,
            index: 0,
            length: 0,
        };

        let packet = UsbPacket::setup(set_configuration_params);

        let ack = UsbPacket {
            pid: Pid::In,
            address: 0,
            endpoint: 0,
            data_toggle: true,
            data: vec![],
        };

        let work = vec![packet, ack];

        self.queue_work(work).await;
    }

    async fn queue_work(&mut self, work: Vec<UsbPacket>) {
        self.uhci.append_work(work).await;
    }

    pub async fn service(&mut self) {
        self.uhci.init().await;
        info!("UHCI initialized");

        for port_offset in [IoOffset::new(0x10), IoOffset::new(0x12)] {
            let address = self.next_usb_addr;
            if !self.uhci.reset_port(port_offset).await {
                continue;
            }

            self.set_address(address).await;
            // Assume single configuration for now
            self.set_configuration(address, 1).await;
            self.device_tx.send(UsbDevice { address }).await;
        }

        loop {
            let work = self.packet_rx.recv().await;
            let finished_work = self.uhci.append_work(work.0).await;

            work.1.send(finished_work).await;
        }
    }
}

#[repr(u8)]
#[allow(unused)]
pub enum UsbSetupRequestValue {
    GetStatus = 0,
    ClearFeature = 1,
    SetFeature = 3,
    SetAddress = 5,
    GetDescriptor = 6,
    SetDescriptor = 7,
    GetConfiguration = 8,
    SetConfiguration = 9,
    GetInterface = 10,
    SetInterface = 11,
    SyncFrame = 12,
}

#[repr(u8)]
#[allow(unused)]
enum UsbDescriptorType {
    Device = 1,
    Configuration = 2,
    String = 3,
    Interface = 4,
    Endpoint = 5,
    DeviceQualifier = 6,
    OtherSpeedConfiguration = 7,
    InterfacePower = 8,
}

pub struct UsbSetupRequestParams {
    pub pid: Pid,
    pub address: u8,
    pub endpoint: u8,
    pub data_toggle: bool,
    pub request_type: u8,
    pub request: UsbSetupRequestValue,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

struct UsbSetupRequest<'a>(&'a mut [u8]);

#[allow(unused)]
impl UsbSetupRequest<'_> {
    const SIZE: usize = 8;

    fn request_type(&self) -> u8 {
        self.0[0]
    }

    // FIXME: Request type should be a more complex type than u8
    fn set_request_type(&mut self, val: u8) {
        self.0[0] = val;
    }

    fn request(&self) -> u8 {
        self.0[1]
    }

    fn set_request(&mut self, val: UsbSetupRequestValue) {
        self.0[1] = val as u8;
    }

    fn value(&self) -> u16 {
        u16::from_le_bytes(
            self.0[2..2 + 2]
                .try_into()
                .expect("Failed to cast to 2 byte array"),
        )
    }

    fn set_value(&mut self, val: u16) {
        self.0[2..2 + 2].copy_from_slice(&val.to_le_bytes())
    }

    fn index(&self) -> u16 {
        u16::from_le_bytes(
            self.0[4..4 + 2]
                .try_into()
                .expect("Failed to cast to 2 byte array"),
        )
    }

    fn set_index(&mut self, val: u16) {
        self.0[4..4 + 2].copy_from_slice(&val.to_le_bytes())
    }

    fn length(&self) -> u16 {
        u16::from_le_bytes(
            self.0[6..6 + 2]
                .try_into()
                .expect("Failed to cast to 2 byte array"),
        )
    }

    fn set_length(&mut self, val: u16) {
        self.0[6..6 + 2].copy_from_slice(&val.to_le_bytes())
    }
}

pub struct UsbDeviceDescriptor<T>(T);

impl<T> UsbDeviceDescriptor<T>
where
    T: AsRef<[u8]>,
{
    pub fn length(&self) -> u8 {
        self.0.as_ref()[0]
    }
    pub fn descriptor_type(&self) -> u8 {
        self.0.as_ref()[1]
    }
    pub fn bcd_usb_version(&self) -> u16 {
        u16::from_le_bytes(
            self.0.as_ref()[2..2 + 2]
                .try_into()
                .expect("Failed to convert to 2 byte array"),
        )
    }
    pub fn device_class(&self) -> u8 {
        self.0.as_ref()[4]
    }
    pub fn device_sublcass(&self) -> u8 {
        self.0.as_ref()[5]
    }
    pub fn device_protocol(&self) -> u8 {
        self.0.as_ref()[6]
    }
    pub fn max_packet_size_endpoint_zero(&self) -> u8 {
        self.0.as_ref()[7]
    }
    pub fn vendor_id(&self) -> u16 {
        u16::from_le_bytes(
            self.0.as_ref()[8..8 + 2]
                .try_into()
                .expect("Failed to convert to 2 byte array"),
        )
    }
    pub fn product_id(&self) -> u16 {
        u16::from_le_bytes(
            self.0.as_ref()[10..10 + 2]
                .try_into()
                .expect("Failed to convert to 2 byte array"),
        )
    }
    pub fn device_version(&self) -> u16 {
        u16::from_le_bytes(
            self.0.as_ref()[12..12 + 2]
                .try_into()
                .expect("Failed to convert to 2 byte array"),
        )
    }
    pub fn manufacturer_string_id(&self) -> u8 {
        self.0.as_ref()[14]
    }
    pub fn product_string_id(&self) -> u8 {
        self.0.as_ref()[15]
    }
    pub fn serial_number_id(&self) -> u8 {
        self.0.as_ref()[16]
    }

    pub fn num_configurations(&self) -> u8 {
        self.0.as_ref()[17]
    }
}

impl<T> fmt::Debug for UsbDeviceDescriptor<T>
where
    T: AsRef<[u8]>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UsbDeviceDescriptorView")
            .field("length", &self.length())
            .field("descriptor_type", &self.descriptor_type())
            .field("bcd_usb_version", &self.bcd_usb_version())
            .field("device_class", &self.device_class())
            .field("device_sublcass", &self.device_sublcass())
            .field("device_protocol", &self.device_protocol())
            .field(
                "max_packet_size_endpoint_zero",
                &self.max_packet_size_endpoint_zero(),
            )
            .field("vendor_id", &self.vendor_id())
            .field("product_id", &self.product_id())
            .field("device_version", &self.device_version())
            .field("manufacturer_string_id", &self.manufacturer_string_id())
            .field("product_string_id", &self.product_string_id())
            .field("serial_number_id", &self.serial_number_id())
            .field("num_configurations", &self.num_configurations())
            .finish()
    }
}

pub struct UsbConfigurationDescriptor<T>(T);

impl<T> UsbConfigurationDescriptor<T>
where
    T: AsRef<[u8]>,
{
    fn legnth(&self) -> u8 {
        self.0.as_ref()[0]
    }
    fn descriptor_type(&self) -> u8 {
        self.0.as_ref()[1]
    }
    fn total_length(&self) -> u16 {
        u16::from_le_bytes(
            self.0.as_ref()[2..2 + 2]
                .try_into()
                .expect("Failed to cast to 2 byte array"),
        )
    }
    fn num_interfaces(&self) -> u8 {
        self.0.as_ref()[4]
    }
    fn configuration_value(&self) -> u8 {
        self.0.as_ref()[5]
    }
    fn configuration_string_index(&self) -> u8 {
        self.0.as_ref()[6]
    }
    fn attributes(&self) -> u8 {
        self.0.as_ref()[7]
    }
    fn max_power(&self) -> u8 {
        self.0.as_ref()[8]
    }
}

impl<T> fmt::Debug for UsbConfigurationDescriptor<T>
where
    T: AsRef<[u8]>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UsbConfigurationDescriptor")
            .field("legnth", &self.legnth())
            .field("descriptor_type", &self.descriptor_type())
            .field("total_length", &self.total_length())
            .field("num_interfaces", &self.num_interfaces())
            .field("configuration_value", &self.configuration_value())
            .field(
                "configuration_string_index",
                &self.configuration_string_index(),
            )
            .field("attributes", &self.attributes())
            .field("max_power", &self.max_power())
            .finish()
    }
}

pub struct UsbInterfaceDescriptor<T>(T);

impl<T> UsbInterfaceDescriptor<T>
where
    T: AsRef<[u8]>,
{
    pub fn length(&self) -> u8 {
        self.0.as_ref()[0]
    }

    pub fn descriptor_type(&self) -> u8 {
        self.0.as_ref()[1]
    }

    pub fn interface_number(&self) -> u8 {
        self.0.as_ref()[2]
    }

    pub fn alternate_setting(&self) -> u8 {
        self.0.as_ref()[3]
    }

    pub fn num_endpoints(&self) -> u8 {
        self.0.as_ref()[4]
    }

    pub fn interface_class(&self) -> u8 {
        self.0.as_ref()[5]
    }

    pub fn interface_subclass(&self) -> u8 {
        self.0.as_ref()[6]
    }

    pub fn interface_protocol(&self) -> u8 {
        self.0.as_ref()[7]
    }

    pub fn interface_string_index(&self) -> u8 {
        self.0.as_ref()[8]
    }
}

impl<T> fmt::Debug for UsbInterfaceDescriptor<T>
where
    T: AsRef<[u8]>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UsbInterafceDescriptor")
            .field("length", &self.length())
            .field("descriptor_type", &self.descriptor_type())
            .field("interface_number", &self.interface_number())
            .field("alternate_setting", &self.alternate_setting())
            .field("num_endpoints", &self.num_endpoints())
            .field("interface_class", &self.interface_class())
            .field("interface_subclass", &self.interface_subclass())
            .field("interface_protocol", &self.interface_protocol())
            .field("interface_string_index", &self.interface_string_index())
            .finish()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum EndpointAddress {
    In(u8),
    Out(u8),
}

#[derive(Debug, Eq, PartialEq)]
pub enum TransferType {
    Control,
    Isochronus,
    Bulk,
    Interrupt,
    Unknown,
}

pub struct UsbEndpointDescriptor<T>(T);

impl<T> UsbEndpointDescriptor<T>
where
    T: AsRef<[u8]>,
{
    pub fn length(&self) -> u8 {
        self.0.as_ref()[0]
    }
    pub fn descriptor_type(&self) -> u8 {
        self.0.as_ref()[1]
    }
    pub fn endpoint_address(&self) -> EndpointAddress {
        let val = self.0.as_ref()[2];
        let address = val.get_bits(0, 4);
        match val.get_bit(7) {
            false => EndpointAddress::Out(address),
            true => EndpointAddress::In(address),
        }
    }

    pub fn attributes(&self) -> u8 {
        self.0.as_ref()[3]
    }

    pub fn transfer_type(&self) -> TransferType {
        match self.attributes().get_bits(0, 2) {
            0 => TransferType::Control,
            1 => TransferType::Isochronus,
            2 => TransferType::Bulk,
            3 => TransferType::Interrupt,
            _ => TransferType::Unknown,
        }
    }

    pub fn max_packet_size(&self) -> u16 {
        u16::from_le_bytes(
            self.0.as_ref()[4..6]
                .try_into()
                .expect("Failed to cast to 2 byte array"),
        )
    }
    pub fn interval(&self) -> u8 {
        self.0.as_ref()[6]
    }
}

impl<T> fmt::Debug for UsbEndpointDescriptor<T>
where
    T: AsRef<[u8]>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UsbEndpointDescriptor")
            .field("length", &self.length())
            .field("descriptor_type", &self.descriptor_type())
            .field("endpoint_address", &self.endpoint_address())
            .field("attributes", &self.attributes())
            .field("max_packet_size", &self.max_packet_size())
            .field("interval", &self.interval())
            .finish()
    }
}

#[derive(Debug)]
pub enum UsbDescriptor<'a> {
    Device(UsbDeviceDescriptor<&'a [u8]>),
    Configuration(UsbConfigurationDescriptor<&'a [u8]>),
    Interface(UsbInterfaceDescriptor<&'a [u8]>),
    Endpoint(UsbEndpointDescriptor<&'a [u8]>),
    Unknown(u8),
}

pub struct UsbDescriptorIterator<'a>(&'a [u8]);

impl<'a> Iterator for UsbDescriptorIterator<'a> {
    type Item = UsbDescriptor<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.len() < 2 {
            return None;
        }

        let length = self.0[0];
        let typ = self.0[1];

        let split = self.0.split_at(length.into());
        self.0 = split.1;

        let ret = match typ {
            1 => UsbDescriptor::Device(UsbDeviceDescriptor(split.0)),
            2 => UsbDescriptor::Configuration(UsbConfigurationDescriptor(split.0)),
            4 => UsbDescriptor::Interface(UsbInterfaceDescriptor(split.0)),
            5 => UsbDescriptor::Endpoint(UsbEndpointDescriptor(split.0)),
            _ => UsbDescriptor::Unknown(typ),
        };

        Some(ret)
    }
}

fn generate_get_configuration_descriptor(address: u8, size: u16) -> Vec<UsbPacket> {
    let setup = UsbSetupRequestParams {
        pid: Pid::Setup,
        address,
        endpoint: 0,
        data_toggle: false,
        request_type: 0x80,
        request: UsbSetupRequestValue::GetDescriptor,
        value: (UsbDescriptorType::Configuration as u16) << 8,
        index: 0,
        length: size,
    };

    let setup = UsbPacket::setup(setup);

    let read = UsbPacket {
        pid: Pid::In,
        address,
        endpoint: 0,
        data_toggle: false,
        data: vec![0; size as usize],
    };

    let ack = UsbPacket {
        pid: Pid::Out,
        address,
        endpoint: 0,
        data_toggle: false,
        data: vec![],
    };

    vec![setup, read, ack]
}
