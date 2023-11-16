use crate::{
    interrupts::IrqId,
    io::io_allocator::{IoAllocator, IoOffset, IoRange},
    util::bit_manipulation::{GetBits, SetBits},
};

use hashbrown::HashSet;

const PCI_CONFIG_OFFSET: IoOffset = IoOffset::new(0);
const PCI_DATA_OFFSET: IoOffset = IoOffset::new(4);

#[derive(Debug)]
pub struct PciIoUnavailable;

pub struct PciDeviceIter<'a> {
    pci: &'a mut Pci,
    finished_iterating: bool,
    bus_num: u8,
    device_num: u8,
    function: u8,
}

impl PciDeviceIter<'_> {
    fn increment_iter(&mut self) {
        self.function = (self.function + 1) % 8;
        if self.function != 0 {
            return;
        }

        self.device_num = (self.device_num + 1) % 32;
        if self.device_num != 0 {
            return;
        }

        self.bus_num = self.bus_num.wrapping_add(1);
        if self.bus_num != 0 {
            return;
        }

        self.finished_iterating = true;
    }
}

impl Iterator for PciDeviceIter<'_> {
    type Item = Result<PciDevice, InvalidHeaderError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.finished_iterating {
                return None;
            }

            let device_vendor =
                self.pci
                    .config_read(self.bus_num, self.device_num, self.function, 0);

            let probed_vendor = device_vendor.get_bits(0, 16) as u16;

            if probed_vendor == 0xffff {
                self.increment_iter();
                continue;
            }

            let ret = Some(
                PciAddress {
                    bus: self.bus_num,
                    slot: self.device_num,
                    function: self.function,
                }
                .upgrade(self.pci),
            );

            self.increment_iter();
            return ret;
        }
    }
}

pub struct Pci {
    pci_io: IoRange,
    allocated_devs: HashSet<PciAddress>,
}

impl Pci {
    pub fn new(io_allocator: &mut IoAllocator) -> Result<Pci, PciIoUnavailable> {
        let pci_io = io_allocator
            .request_io_range(0xCF8, 8)
            .ok_or(PciIoUnavailable)?;
        let allocated_devs = HashSet::new();
        Ok(Pci {
            pci_io,
            allocated_devs,
        })
    }

    fn select_pci_address(&mut self, bus: u8, slot: u8, func: u8, offset: u8) {
        assert_eq!(offset & 0b11, 0, "PCI reads must be 4 byte aligned");
        let mut address = offset as u32;
        address.set_bits(8, 3, func as u32);
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

    pub fn devices(&mut self) -> PciDeviceIter {
        PciDeviceIter {
            pci: self,
            finished_iterating: false,
            bus_num: 0,
            device_num: 0,
            function: 0,
        }
    }
}

#[derive(Debug)]
pub struct InvalidHeaderError;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct PciAddress {
    bus: u8,
    slot: u8,
    function: u8,
}

impl PciAddress {
    fn read_register(&mut self, pci: &mut Pci, register: u8) -> u32 {
        pci.config_read(self.bus, self.slot, self.function, register * 4)
    }

    fn write_register(&mut self, pci: &mut Pci, register: u8, value: u32) {
        pci.config_write(self.bus, self.slot, self.function, register * 4, value)
    }

    fn enable_bus_mastering(&mut self, pci: &mut Pci) {
        const STATUS_COMMAND_OFFSET: u8 = 4;
        // Read offset 4 == register 1 where the command register is
        let mut status_command =
            pci.config_read(self.bus, self.slot, self.function, STATUS_COMMAND_OFFSET);
        // Bus mastering bit is bit 2
        status_command.set_bit(2, true);
        pci.config_write(
            self.bus,
            self.slot,
            self.function,
            STATUS_COMMAND_OFFSET,
            status_command,
        );
    }

    fn upgrade(mut self, pci: &mut Pci) -> Result<PciDevice, InvalidHeaderError> {
        let header_type = self.read_register(pci, 3).get_bits(16, 8);

        if pci.allocated_devs.contains(&self) {
            panic!("Device already in use");
        }

        pci.allocated_devs.insert(self.clone());
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
pub struct PciInterfaceId {
    pub class: u8,
    pub subclass: u8,
    pub interface: u8,
    pub revision: u8,
}

#[derive(Debug)]
pub struct InvalidIrq;

#[derive(Debug)]
pub struct GeneralPciDevice {
    addr: PciAddress,
}

impl GeneralPciDevice {
    pub fn id(&mut self, pci: &mut Pci) -> (u16, u16) {
        let reg = pci.config_read(self.addr.bus, self.addr.slot, self.addr.function, 0);
        let vendor = reg.get_bits(0, 16) as u16;
        let device = reg.get_bits(16, 16) as u16;
        (vendor, device)
    }

    pub fn interface_id(&mut self, pci: &mut Pci) -> PciInterfaceId {
        let reg = pci.config_read(self.addr.bus, self.addr.slot, self.addr.function, 0x8);
        PciInterfaceId {
            class: reg.get_bits(24, 8) as u8,
            subclass: reg.get_bits(16, 8) as u8,
            interface: reg.get_bits(8, 8) as u8,
            revision: reg.get_bits(0, 8) as u8,
        }
    }

    #[allow(unused)]
    pub fn find_io_base(&mut self, pci: &mut Pci) -> Option<u32> {
        for i in 0..=5 {
            let base_address = self.addr.read_register(pci, 4 + i);
            if base_address.get_bit(0) && base_address.get_bits(2, 30) > 0 {
                return Some(base_address & !0b11);
            }
        }

        None
    }

    pub fn find_mmap_range(&mut self, pci: &mut Pci) -> Option<MmapRange> {
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

    pub fn enable_bus_mastering(&mut self, pci: &mut Pci) {
        self.addr.enable_bus_mastering(pci);
    }

    pub fn get_irq_num(&mut self, pci: &mut Pci) -> Result<IrqId, InvalidIrq> {
        let reg = self.addr.read_register(pci, 0xf);
        let irq = reg.get_bits(0, 8) as u8;

        if irq < 8 {
            Ok(IrqId::Pic1(irq))
        } else if irq < 16 {
            Ok(IrqId::Pic2(irq - 8))
        } else {
            Err(InvalidIrq)
        }
    }
}

#[derive(Debug)]
pub enum PciDevice {
    General(GeneralPciDevice),
    PciPciBridge,
    PciCardBusBridge,
}
