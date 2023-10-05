pub mod tcp;

use alloc::vec::Vec;
use tcp::TcpFrame;

use core::convert::From;

use crate::{util::bit_manipulation::GetBits, IpAddr};

#[derive(Copy, Clone)]
#[repr(u16)]
pub enum EtherType {
    Ipv4 = 0x0800,
    Arp = 0x0806,
}

pub struct EthernetFrameParams<'a> {
    pub dest_mac: [u8; 6],
    pub source_mac: [u8; 6],
    pub ether_type: EtherType,
    pub payload: &'a [u8],
}

pub fn generate_ethernet_frame(params: &EthernetFrameParams<'_>) -> Vec<u8> {
    // 64, but CRC is appended by ethernet card
    const MIN_LENGTH: usize = 60;

    let length = core::mem::size_of_val(&params.dest_mac)
        + core::mem::size_of_val(&params.source_mac)
        + core::mem::size_of_val(&params.ether_type)
        + params.payload.len();

    let mut ret = Vec::with_capacity(length);

    ret.extend_from_slice(&params.dest_mac);
    ret.extend_from_slice(&params.source_mac);
    ret.extend_from_slice(&(params.ether_type as u16).to_be_bytes());
    ret.extend_from_slice(params.payload);
    if ret.len() < MIN_LENGTH {
        ret.resize(MIN_LENGTH, 0);
    }
    ret
}

#[derive(Debug)]
pub struct InvalidEthernetFrame;

pub struct EthernetFrame<'a> {
    packet: &'a [u8],
}

impl<'a> EthernetFrame<'a> {
    fn new(packet: &[u8]) -> Result<EthernetFrame, InvalidEthernetFrame> {
        let frame = EthernetFrame { packet };
        const HEADER_LEN_NO_DOT1Q: usize = 14;
        const DOT1Q_LEN: usize = 4;

        if packet.len() < HEADER_LEN_NO_DOT1Q
            || frame.has_dot1q() && packet.len() < HEADER_LEN_NO_DOT1Q + DOT1Q_LEN
            || packet.len() - 4 <= frame.payload_offset()
        {
            return Err(InvalidEthernetFrame);
        }

        Ok(frame)
    }

    pub fn destination_mac(&self) -> &[u8] {
        &self.packet[0..6]
    }

    pub fn source_mac(&self) -> &[u8] {
        &self.packet[6..12]
    }

    pub fn tag(&self) -> Option<&[u8]> {
        if self.has_dot1q() {
            Some(&self.packet[12..16])
        } else {
            None
        }
    }

    pub fn ether_type(&self) -> u16 {
        let start = self.ether_type_offset();
        let end = start + 2;
        u16::from_be_bytes(
            self.packet[start..end]
                .try_into()
                .expect("Invalid slice size for ether_type"),
        )
    }

    pub fn payload_offset(&self) -> usize {
        self.ether_type_offset() + 2
    }

    pub fn payload(&self) -> &'a [u8] {
        let start = self.ether_type_offset() + 2;
        let end = self.packet.len() - 4;
        &self.packet[start..end]
    }

    pub fn crc(&self) -> u32 {
        u32::from_be_bytes(
            self.packet[self.packet.len() - 4..]
                .try_into()
                .expect("Invalid number of bytes for crc"),
        )
    }

    fn ether_type_offset(&self) -> usize {
        if self.has_dot1q() {
            16
        } else {
            12
        }
    }

    fn has_dot1q(&self) -> bool {
        let tag = u16::from_be_bytes(
            self.packet[12..14]
                .try_into()
                .expect("Incorrect slice size"),
        );
        const DOT1Q_ID: u16 = 0x8100;
        tag == DOT1Q_ID
    }
}

impl core::fmt::Debug for EthernetFrame<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "destination_mac: {:x?}", self.destination_mac())?;
        writeln!(f, "source_mac: {:x?}", self.source_mac())?;
        writeln!(f, "tag: {:?}", self.tag())?;
        writeln!(f, "ether_type: {:#06x}", self.ether_type())?;
        writeln!(f, "payload: {:x?}", self.payload())?;
        writeln!(f, "crc: {:x?}", self.crc())?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct InvalidArpFrame(usize);

pub struct ArpFrame<'a> {
    packet: &'a [u8],
}

impl ArpFrame<'_> {
    pub fn new(packet: &[u8]) -> Result<ArpFrame<'_>, InvalidArpFrame> {
        const FRAME_LEN: usize = 28;
        if packet.len() < FRAME_LEN {
            return Err(InvalidArpFrame(packet.len()));
        }
        let frame = ArpFrame { packet };
        Ok(frame)
    }

    pub fn htype(&self) -> u16 {
        u16::from_be_bytes(
            self.packet[0..2]
                .try_into()
                .expect("Invalid length for htype"),
        )
    }

    pub fn ptype(&self) -> u16 {
        u16::from_be_bytes(
            self.packet[2..4]
                .try_into()
                .expect("Invalid length for ptype"),
        )
    }

    pub fn hardware_address_length(&self) -> u8 {
        self.packet[4]
    }

    pub fn protocol_address_length(&self) -> u8 {
        self.packet[5]
    }

    pub fn operation(&self) -> Result<ArpOperation, UnknownArpOperation> {
        u16::from_be_bytes(
            self.packet[6..8]
                .try_into()
                .expect("Invalid length for operation"),
        )
        .try_into()
    }

    pub fn sender_hardware_address(&self) -> &[u8] {
        &self.packet[8..14]
    }

    pub fn sender_protocol_address(&self) -> &[u8] {
        &self.packet[14..18]
    }

    pub fn target_hardware_address(&self) -> &[u8] {
        &self.packet[18..24]
    }

    pub fn target_protocol_address(&self) -> &[u8] {
        &self.packet[24..28]
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u16)]
pub enum ArpOperation {
    Request = 1,
    Reply = 2,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct UnknownArpOperation(pub u16);

impl TryFrom<u16> for ArpOperation {
    type Error = UnknownArpOperation;

    fn try_from(value: u16) -> Result<Self, UnknownArpOperation> {
        match value {
            1 => Ok(ArpOperation::Request),
            2 => Ok(ArpOperation::Reply),
            v => Err(UnknownArpOperation(v)),
        }
    }
}

impl From<ArpOperation> for u16 {
    fn from(value: ArpOperation) -> Self {
        value as u16
    }
}

pub struct ArpFrameParams {
    pub hardware_type: u16,
    pub protocol_type: u16,
    pub hardware_address_length: u8,
    pub protocol_address_length: u8,
    pub operation: ArpOperation,
    pub sender_hardware_address: [u8; 6],
    pub sender_protocol_address: [u8; 4],
    pub target_hardware_address: [u8; 6],
    pub target_protocol_address: [u8; 4],
}

impl TryFrom<&ArpFrame<'_>> for ArpFrameParams {
    type Error = UnknownArpOperation;
    fn try_from(arp_frame: &ArpFrame<'_>) -> Result<Self, UnknownArpOperation> {
        Ok(ArpFrameParams {
            hardware_type: arp_frame.htype(),
            protocol_type: arp_frame.ptype(),
            hardware_address_length: arp_frame.hardware_address_length(),
            protocol_address_length: arp_frame.protocol_address_length(),
            operation: arp_frame.operation()?,
            sender_hardware_address: arp_frame
                .sender_hardware_address()
                .try_into()
                .expect("Input sender hardware address length wrong"),
            target_hardware_address: arp_frame
                .target_hardware_address()
                .try_into()
                .expect("Input address length wrong"),
            sender_protocol_address: arp_frame
                .sender_protocol_address()
                .try_into()
                .expect("Protoco address should be 4 bytes"),
            target_protocol_address: arp_frame
                .target_protocol_address()
                .try_into()
                .expect("Protoco address should be 4 bytes"),
        })
    }
}

pub fn generate_arp_frame(params: &ArpFrameParams) -> Vec<u8> {
    const ARP_LENGTH: usize = 28;
    let mut ret = Vec::with_capacity(ARP_LENGTH);

    ret.extend_from_slice(&params.hardware_type.to_be_bytes());
    ret.extend_from_slice(&params.protocol_type.to_be_bytes());
    ret.extend_from_slice(&params.hardware_address_length.to_be_bytes());
    ret.extend_from_slice(&params.protocol_address_length.to_be_bytes());
    ret.extend_from_slice(&u16::from(params.operation).to_be_bytes());
    ret.extend_from_slice(&params.sender_hardware_address);
    ret.extend_from_slice(&params.sender_protocol_address);
    ret.extend_from_slice(&params.target_hardware_address);
    ret.extend_from_slice(&params.target_protocol_address);
    ret
}

pub fn generate_arp_request(remote_ip: &[u8; 4], local_ip: &[u8; 4], mac: &[u8; 6]) -> Vec<u8> {
    generate_arp_frame(&ArpFrameParams {
        // FIXME: Name hardware/protocol type, maybe make defaults
        hardware_type: 1,
        protocol_type: 0x0800,
        hardware_address_length: 6,
        protocol_address_length: 4,
        operation: ArpOperation::Request,
        sender_hardware_address: *mac,
        sender_protocol_address: *local_ip,
        target_hardware_address: [0; 6],
        target_protocol_address: *remote_ip,
    })
}

impl core::fmt::Debug for ArpFrame<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "htype: {:?}", self.htype())?;
        writeln!(f, "ptype: {:?}", self.ptype())?;
        writeln!(
            f,
            "hardware_address_length: {:?}",
            self.hardware_address_length()
        )?;
        writeln!(
            f,
            "protocol_address_length: {:?}",
            self.protocol_address_length()
        )?;
        writeln!(f, "operation: {:?}", self.operation())?;
        writeln!(
            f,
            "sender_hardware_address: {:?}",
            self.sender_hardware_address()
        )?;
        writeln!(
            f,
            "sender_protocol_address: {:?}",
            self.sender_protocol_address()
        )?;
        writeln!(
            f,
            "target_hardware_address: {:?}",
            self.target_hardware_address()
        )?;
        writeln!(
            f,
            "target_protocol_address: {:?}",
            self.target_protocol_address()
        )?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct InvalidIpv4Frame;

#[derive(Debug)]
pub struct Ipv4Frame<'a> {
    packet: &'a [u8],
}

impl<'a> Ipv4Frame<'a> {
    fn new(packet: &[u8]) -> Result<Ipv4Frame, InvalidIpv4Frame> {
        let frame = Ipv4Frame { packet };

        if packet.is_empty()
            || frame.header_length() > packet.len()
            || frame.total_length() > packet.len()
        {
            return Err(InvalidIpv4Frame);
        }

        Ok(frame)
    }

    fn ihl(&self) -> u8 {
        self.packet[0].get_bits(0, 4)
    }

    fn total_length(&self) -> usize {
        u16::from_be_bytes(
            self.packet[2..4]
                .try_into()
                .expect("Invalid length for total ipv4 length"),
        ) as usize
    }

    fn protocol(&self) -> Ipv4Protocol {
        match self.packet[9] {
            0x06 => Ipv4Protocol::Tcp,
            0x11 => Ipv4Protocol::Udp,
            v => Ipv4Protocol::Unknown(v),
        }
    }

    fn payload(&self) -> &'a [u8] {
        let ipv4_length = self.ihl() * 4;
        &self.packet[ipv4_length as usize..self.total_length()]
    }

    pub fn source_ip(&self) -> IpAddr {
        self.packet[12..16]
            .try_into()
            .expect("Invalid length for ipv4 source ip")
    }

    fn header_length(&self) -> usize {
        (self.ihl() as usize) * 4
    }
}

fn calculate_ipv4_checksum(data: &[u8]) -> u16 {
    assert_eq!(data.len() % 2, 0);

    let mut checksum = 0u16;
    for slice in data.chunks(2) {
        // Ones compliment add, maybe should be factored out
        let val = u16::from_be_bytes(slice.try_into().expect("Slice should be 2 bytes"));
        let overflow_res;
        (checksum, overflow_res) = checksum.overflowing_add(val);

        if overflow_res {
            checksum += 1;
        }
    }

    !checksum
}

pub fn generate_ipv4_frame(
    payload: &[u8],
    protocol: Ipv4Protocol,
    source_ip: &IpAddr,
    dest_ip: &IpAddr,
) -> Vec<u8> {
    // FIXME: capacity?
    let mut ret: Vec<u8> = Vec::new();

    const HEADER_SIZE: u16 = 20;
    // Version + IHL
    ret.push(0x45);
    // DSCP ECN
    ret.push(0x0);
    // FIXME: usize -> u16 truncation
    ret.extend_from_slice(&(HEADER_SIZE + payload.len() as u16).to_be_bytes());
    // Identification
    ret.extend_from_slice(&0u16.to_be_bytes());
    // Flags + fragment offset
    ret.extend_from_slice(&0u16.to_be_bytes());
    // TTL (copied from wireshark incoming packet)
    ret.push(64);
    ret.push(protocol.into());

    let checksum_loc = ret.len();
    ret.extend_from_slice(&0u16.to_be_bytes());
    ret.extend_from_slice(source_ip);
    ret.extend_from_slice(dest_ip);

    let checksum = calculate_ipv4_checksum(&ret);
    ret[checksum_loc..checksum_loc + 2].copy_from_slice(&checksum.to_be_bytes());

    ret.extend_from_slice(payload);

    ret
}

#[derive(Debug)]
pub struct InvalidUdpFrame(usize, usize);

#[derive(Debug)]
pub struct UdpFrame<'a> {
    packet: &'a [u8],
}

impl UdpFrame<'_> {
    const HEADER_LENGTH: usize = 8;

    fn new(packet: &[u8]) -> Result<UdpFrame, InvalidUdpFrame> {
        let frame = UdpFrame { packet };

        if packet.len() < Self::HEADER_LENGTH || packet.len() < frame.length() as usize {
            return Err(InvalidUdpFrame(packet.len(), frame.length() as usize));
        }

        Ok(frame)
    }

    fn length(&self) -> u16 {
        u16::from_be_bytes(
            self.packet[4..6]
                .try_into()
                .expect("u16 packet size incorrect"),
        )
    }

    pub fn data(&self) -> &[u8] {
        &self.packet[Self::HEADER_LENGTH..self.length() as usize]
    }
}
pub fn generate_udp_frame(dest_port: u16, payload: &[u8]) -> Vec<u8> {
    let length: u16 = UdpFrame::HEADER_LENGTH as u16 + payload.len() as u16;

    let mut ret = Vec::with_capacity(length.into());

    const SOURCE_PORT: &[u8] = &0u16.to_be_bytes();
    ret.extend_from_slice(SOURCE_PORT);
    ret.extend_from_slice(&dest_port.to_be_bytes());
    ret.extend_from_slice(&length.to_be_bytes());
    const CHECKSUM: &[u8] = &0u16.to_be_bytes();
    ret.extend_from_slice(CHECKSUM);
    ret.extend_from_slice(payload);
    ret
}

#[derive(Debug)]
pub enum ParsePacketError {
    Ethernet(InvalidEthernetFrame),
    Arp(InvalidArpFrame),
    Ipv4(InvalidIpv4Frame),
}

pub enum ParsedPacket<'a> {
    Arp(ArpFrame<'a>),
    Ipv4(Ipv4Frame<'a>),
    Unknown(u16),
}

pub struct ParsedEthernetFrame<'a> {
    pub ethernet: EthernetFrame<'a>,
    pub inner: ParsedPacket<'a>,
}

pub fn parse_packet(data: &[u8]) -> Result<ParsedEthernetFrame, ParsePacketError> {
    let frame = EthernetFrame::new(data).map_err(ParsePacketError::Ethernet)?;

    let payload = frame.payload();
    let ret = match frame.ether_type() {
        0x0806 => {
            let arp_frame = ArpFrame::new(payload).map_err(ParsePacketError::Arp)?;
            ParsedPacket::Arp(arp_frame)
        }
        0x0800 => {
            let ipv4_frame = Ipv4Frame::new(payload).map_err(ParsePacketError::Ipv4)?;
            ParsedPacket::Ipv4(ipv4_frame)
        }
        t => ParsedPacket::Unknown(t),
    };

    Ok(ParsedEthernetFrame {
        ethernet: frame,
        inner: ret,
    })
}

#[derive(Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Ipv4Protocol {
    Tcp,
    Udp,
    Unknown(u8),
}

impl core::convert::From<Ipv4Protocol> for u8 {
    fn from(value: Ipv4Protocol) -> Self {
        match value {
            Ipv4Protocol::Tcp => 0x06,
            Ipv4Protocol::Udp => 0x11,
            Ipv4Protocol::Unknown(v) => v,
        }
    }
}

pub enum ParsedIpv4Frame<'a> {
    Udp(UdpFrame<'a>),
    Tcp(TcpFrame<'a>),
    Unknown(Ipv4Protocol),
}

pub fn parse_ipv4<'a>(frame: &Ipv4Frame<'a>) -> Result<ParsedIpv4Frame<'a>, InvalidUdpFrame> {
    debug!(
        "Parsing IPV4 packet with protocol {:#04x?}",
        frame.protocol()
    );
    let ret = match frame.protocol() {
        Ipv4Protocol::Udp => ParsedIpv4Frame::Udp(UdpFrame::new(frame.payload())?),
        Ipv4Protocol::Tcp => ParsedIpv4Frame::Tcp(TcpFrame::new(frame.payload())),
        p => ParsedIpv4Frame::Unknown(p),
    };
    Ok(ret)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;
    use alloc::string::ToString;

    const ARP_REQUEST: &[u8] = &[
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x52, 0x55, 0x0a, 0x00, 0x02, 0x02, 0x08, 0x06, 0x00,
        0x01, 0x08, 0x00, 0x06, 0x04, 0x00, 0x01, 0x52, 0x55, 0x0a, 0x00, 0x02, 0x02, 0x0a, 0x00,
        0x02, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0xa8, 0x7a, 0x37, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    const UDP_REQUEST: &[u8] = &[
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0x52, 0x55, 0x0a, 0x00, 0x02, 0x02, 0x08, 0x00, 0x45,
        0x00, 0x00, 0x21, 0x00, 0x00, 0x00, 0x00, 0x40, 0x11, 0x33, 0xeb, 0x0a, 0x00, 0x02, 0x02,
        0xc0, 0xa8, 0x7a, 0x37, 0x96, 0x1e, 0x17, 0x70, 0x00, 0x0d, 0x19, 0x8a, 0x74, 0x65, 0x73,
        0x74, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    create_test!(test_arp_operation_parse, {
        test_eq!(
            ArpOperation::try_from(1),
            Ok::<ArpOperation, UnknownArpOperation>(ArpOperation::Request)
        );
        test_eq!(
            ArpOperation::try_from(2),
            Ok::<ArpOperation, UnknownArpOperation>(ArpOperation::Reply)
        );
        test_eq!(
            ArpOperation::try_from(3),
            Err::<ArpOperation, UnknownArpOperation>(UnknownArpOperation(3))
        );
        test_eq!(
            ArpOperation::try_from(99),
            Err::<ArpOperation, UnknownArpOperation>(UnknownArpOperation(99))
        );
        Ok(())
    });

    create_test!(test_ethernet_frame_validation, {
        test_ok!(EthernetFrame::new(ARP_REQUEST));

        let mut corrupted = ARP_REQUEST.to_vec();
        corrupted.drain(12..);
        test_err!(EthernetFrame::new(&corrupted));

        // This is just enough for an empty payload + CRC
        let mut corrupted = ARP_REQUEST.to_vec();
        corrupted.drain(20..);
        test_ok!(EthernetFrame::new(&corrupted));

        // However if we're dot1q it's not enough
        corrupted[12..14].copy_from_slice(&[0x81, 0x00]);
        test_err!(EthernetFrame::new(&corrupted));

        // And it should be fine if we extend again
        corrupted.extend_from_slice(&[1, 2, 3, 4]);
        test_ok!(EthernetFrame::new(&corrupted));

        Ok(())
    });

    create_test!(test_ethernet_frame_parsing, {
        let frame =
            EthernetFrame::new(ARP_REQUEST).map_err(|_| "Invalid ethernet frame".to_string())?;
        test_eq!(
            frame.destination_mac(),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
        test_eq!(frame.source_mac(), &[82, 85, 10, 0, 2, 2]);
        test_eq!(frame.tag(), None::<&[u8]>);
        test_eq!(frame.ether_type(), 0x0806);
        // :(, not sure why my wireshark capture has no FCS
        test_eq!(frame.crc(), 0x00);
        Ok(())
    });

    create_test!(test_arp_frame_validation, {
        let frame =
            EthernetFrame::new(ARP_REQUEST).map_err(|_| "Invalid ethernet frame".to_string())?;

        let payload = frame.payload();
        let arp_frame = ArpFrame::new(payload);
        test_ok!(arp_frame);

        let mut shortened = payload.to_vec();
        shortened.resize(28, 0);
        let arp_frame = ArpFrame::new(&shortened);
        test_ok!(arp_frame);

        shortened.pop();
        let arp_frame = ArpFrame::new(&shortened);
        test_err!(arp_frame);

        Ok(())
    });

    create_test!(test_arp_frame_parsing, {
        let frame =
            EthernetFrame::new(ARP_REQUEST).map_err(|_| "Invalid ethernet frame".to_string())?;

        let payload = frame.payload();
        let arp_frame = ArpFrame::new(payload).map_err(|_| "Invalid arp frame".to_string())?;

        test_eq!(arp_frame.htype(), 1);
        test_eq!(arp_frame.ptype(), 0x0800);
        test_eq!(arp_frame.hardware_address_length(), 6);
        test_eq!(arp_frame.protocol_address_length(), 4);
        test_eq!(
            arp_frame.operation(),
            Ok::<_, UnknownArpOperation>(ArpOperation::Request)
        );
        test_eq!(arp_frame.sender_hardware_address(), &[82, 85, 10, 0, 2, 2]);
        test_eq!(arp_frame.sender_protocol_address(), &[10, 0, 2, 2]);
        test_eq!(arp_frame.target_hardware_address(), &[0u8; 6]);
        test_eq!(arp_frame.target_protocol_address(), &[192, 168, 122, 55]);

        Ok(())
    });

    create_test!(test_ipv4_frame_validation, {
        let frame =
            EthernetFrame::new(UDP_REQUEST).map_err(|_| "Invalid ethernet frame".to_string())?;

        let ipv4_frame = Ipv4Frame::new(frame.payload());
        test_ok!(ipv4_frame);

        let ipv4_frame = Ipv4Frame::new(&[]);
        test_err!(ipv4_frame);

        let ipv4_frame = Ipv4Frame::new(&[0xff]);
        test_err!(ipv4_frame);

        Ok(())
    });

    create_test!(test_ipv4_frame_parsing, {
        let frame =
            EthernetFrame::new(UDP_REQUEST).map_err(|_| "Invalid ethernet frame".to_string())?;
        let frame =
            Ipv4Frame::new(frame.payload()).map_err(|_| "Invalid ipv4 frame".to_string())?;
        test_eq!(frame.ihl(), 5);
        test_eq!(frame.protocol(), Ipv4Protocol::Udp);
        test_eq!(frame.header_length(), 20);
        Ok(())
    });

    create_test!(test_udp_frame_validation, {
        let frame =
            EthernetFrame::new(UDP_REQUEST).map_err(|_| "Invalid ethernet frame".to_string())?;
        let frame =
            Ipv4Frame::new(frame.payload()).map_err(|_| "Invalid ipv4 frame".to_string())?;

        let udp_frame = UdpFrame::new(frame.payload());
        test_ok!(udp_frame);

        let mut payload = frame.payload().to_vec();
        payload.resize(12, 0);
        let udp_frame = UdpFrame::new(&payload);
        test_err!(udp_frame);

        payload[4..6].copy_from_slice(&4u16.to_be_bytes());
        let udp_frame = UdpFrame::new(&payload);
        test_ok!(udp_frame);

        payload.resize(7, 0);
        let udp_frame = UdpFrame::new(&payload);
        test_err!(udp_frame);

        Ok(())
    });

    create_test!(test_udp_frame_parsing, {
        let frame =
            EthernetFrame::new(UDP_REQUEST).map_err(|_| "Invalid ethernet frame".to_string())?;
        let frame =
            Ipv4Frame::new(frame.payload()).map_err(|_| "Invalid ipv4 frame".to_string())?;
        let frame = UdpFrame::new(frame.payload()).map_err(|_| "Invalid UDP frame".to_string())?;
        test_eq!(frame.length(), 13);
        test_eq!(frame.data(), b"test\n");

        Ok(())
    });
}
