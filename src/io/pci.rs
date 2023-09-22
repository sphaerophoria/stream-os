use crate::{
    io::io_allocator::{IoAllocator, IoOffset, IoRange},
    util::bit_manipulation::{GetBits, SetBits},
};

const PCI_CONFIG_OFFSET: IoOffset = IoOffset::new(0);
const PCI_DATA_OFFSET: IoOffset = IoOffset::new(4);

#[derive(Debug)]
pub struct PciIoUnavailable;

pub struct Pci {
    pci_io: IoRange,
}

impl Pci {
    pub fn new(io_allocator: &mut IoAllocator) -> Result<Pci, PciIoUnavailable> {
        let pci_io = io_allocator
            .request_io_range(0xCF8, 8)
            .ok_or(PciIoUnavailable)?;
        Ok(Pci { pci_io })
    }

    fn select_pci_address(&mut self, bus: u8, slot: u8, func: u8, offset: u8) {
        assert_eq!(offset & 0b11, 0, "PCI reads must be 4 byte aligned");
        let mut address = offset as u32;
        address.set_bits(8, 2, func as u32);
        address.set_bits(11, 5, slot as u32);
        address.set_bits(16, 8, bus as u32);
        address.set_bit(31, true);

        self.pci_io.write_32(PCI_CONFIG_OFFSET, address).unwrap();
    }

    fn config_read(&mut self, bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
        self.select_pci_address(bus, slot, func, offset);
        self.pci_io.read_32(PCI_DATA_OFFSET).unwrap()
    }

    fn config_write(&mut self, bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
        self.select_pci_address(bus, slot, func, offset);
        self.pci_io.write_32(PCI_DATA_OFFSET, value).unwrap()
    }

    pub fn find_device(
        &mut self,
        vendor: u16,
        device: u16,
    ) -> Result<Option<PciDevice>, InvalidHeaderError> {
        for bus_num in 0..=255 {
            for device_num in 0..=255 {
                let device_vendor = self.config_read(bus_num, device_num, 0, 0);
                let probed_vendor = device_vendor.get_bits(0, 16) as u16;
                let probed_device = device_vendor.get_bits(16, 16) as u16;
                if vendor == probed_vendor && device == probed_device {
                    return Some(
                        PciAddress {
                            bus: bus_num,
                            slot: device_num,
                        }
                        .upgrade(self),
                    )
                    .transpose();
                }
            }
        }

        Ok(None)
    }
}

#[derive(Debug)]
pub struct InvalidHeaderError;

#[derive(Debug)]
struct PciAddress {
    bus: u8,
    slot: u8,
}

impl PciAddress {
    pub fn read_register(&self, pci: &mut Pci, register: u8) -> u32 {
        pci.config_read(self.bus, self.slot, 0, register * 4)
    }

    pub fn write_register(&self, pci: &mut Pci, register: u8, value: u32) {
        pci.config_write(self.bus, self.slot, 0, register * 4, value)
    }

    fn upgrade(self, pci: &mut Pci) -> Result<PciDevice, InvalidHeaderError> {
        let header_type = self.read_register(pci, 3).get_bits(16, 8);
        match header_type {
            0 => Ok(PciDevice::General(GeneralPciDevice { addr: self })),
            1 => Ok(PciDevice::PciPciBridge),
            2 => Ok(PciDevice::PciCardBusBridge),
            _ => Err(InvalidHeaderError),
        }
    }
}

#[derive(Debug)]
pub struct MmapRange {
    pub start: *mut u8,
    pub length: usize,
}

#[derive(Debug)]
pub struct GeneralPciDevice {
    addr: PciAddress,
}

impl GeneralPciDevice {
    #[allow(unused)]
    pub fn find_io_base(&self, pci: &mut Pci) -> Option<u32> {
        for i in 0..=5 {
            let base_address = self.addr.read_register(pci, 4 + i);
            if base_address.get_bit(0) && base_address.get_bits(2, 30) > 0 {
                return Some(base_address & !0b11);
            }
        }

        None
    }

    pub fn find_mmap_range(&self, pci: &mut Pci) -> Option<MmapRange> {
        for i in 0..=5 {
            let register_offset = 4 + i;
            let base_address = self.addr.read_register(pci, register_offset);
            if !base_address.get_bit(0) && base_address > 0 {
                let start = (base_address & !0b1111) as *mut u8;

                self.addr.write_register(pci, register_offset, !0);

                let end_address = self.addr.read_register(pci, register_offset);
                let length: u32 = !(end_address & !0b1111) + 1;
                let length = length as usize;

                self.addr.write_register(pci, register_offset, base_address);

                return Some(MmapRange { start, length });
            }
        }

        None
    }
}

#[derive(Debug)]
pub enum PciDevice {
    General(GeneralPciDevice),
    PciPciBridge,
    PciCardBusBridge,
}
