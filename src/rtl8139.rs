use crate::io::pci::{InvalidHeaderError, Pci, PciDevice};

#[derive(Debug)]
pub enum Rtl8139InitError {
    PciProbeFailed(InvalidHeaderError),
    DeviceNotFound,
    PciHeaderTypeIncorrect(PciDevice),
    MmapRangeUnexpected(usize),
}

pub struct Rtl8139 {
    mapped_memory: &'static mut [u8],
}

impl Rtl8139 {
    pub fn new(pci: &mut Pci) -> Result<Rtl8139, Rtl8139InitError> {
        let device = pci
            .find_device(0x10ec, 0x8139)
            .map_err(Rtl8139InitError::PciProbeFailed)?
            .ok_or(Rtl8139InitError::DeviceNotFound)?;

        let rtl_device = match device {
            PciDevice::General(v) => v,
            _ => {
                return Err(Rtl8139InitError::PciHeaderTypeIncorrect(device));
            }
        };
        let mmap_range = rtl_device.find_mmap_range(pci).unwrap();
        if mmap_range.length != 256 {
            return Err(Rtl8139InitError::MmapRangeUnexpected(mmap_range.length));
        }

        let mapped_memory =
            unsafe { core::slice::from_raw_parts_mut(mmap_range.start, mmap_range.length) };

        Ok(Rtl8139 { mapped_memory })
    }

    pub fn log_mac(&self) {
        info!("Mac address: {:x?}", &self.mapped_memory[0..6]);
    }
}
