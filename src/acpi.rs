use core::marker::PhantomData;

#[derive(Debug)]
#[repr(C, packed)]
pub struct Rsdp {
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,
}

impl Rsdp {
    pub fn validate_checksum(&self) -> bool {
        let mut acc = 0u8;

        let p = self as *const Rsdp as *const u8;

        unsafe {
            for i in 0..core::mem::size_of::<Rsdp>() {
                acc = acc.wrapping_add(*p.add(i))
            }
        }

        acc == 0
    }

    pub fn rsdt(&self) -> &Rsdt {
        unsafe { &*(self.rsdt_address as *const Rsdt) }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum MadtEntry {
    LocalApic {
        acpi_id: u8,
        apic_id: u8,
        flags: u32,
    },
}

pub struct MadtEntryIter {
    it: *const u8,
    end: *const u8,
}

impl Iterator for MadtEntryIter {
    type Item = MadtEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.it >= self.end {
            return None;
        }

        unsafe {
            let record_type = *self.it;
            let record_length = *self.it.add(1);

            debug!("record type: {record_type}, record_length: {record_length}");

            let ret = match record_type {
                0 => {
                    // Local APIC
                    let mut flags: u32 = 0;
                    self.it
                        .add(4)
                        .copy_to_nonoverlapping(&mut flags as *mut u32 as *mut u8, 4);

                    let acpi_id = *self.it.add(2);
                    let apic_id = *self.it.add(3);

                    debug!("acpi id: {acpi_id}, apic_id: {apic_id}, flags: {flags:#x}");

                    Some(MadtEntry::LocalApic {
                        acpi_id,
                        apic_id,
                        flags,
                    })
                }
                _ => None,
            };

            self.it = self.it.add(record_length as usize);

            ret
        }
    }
}

#[repr(C, packed)]
pub struct Madt {
    header: AcpiSdtHeader,
    local_apic_addr: u32,
    flags: u32,
    entries: (),
}

impl Madt {
    pub fn entries(&self) -> MadtEntryIter {
        unsafe {
            let it = &self.entries as *const () as *const u8;
            let entry_length = self.header.length as usize - core::mem::size_of::<Madt>();
            let end = it.add(entry_length);
            MadtEntryIter { it, end }
        }
    }

    pub fn local_apic_addr(&self) -> *mut u8 {
        self.local_apic_addr as *mut u8
    }
}

impl core::fmt::Debug for Madt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Madt {{ header: {:?},", self.header)?;
        let local_apic_addr = self.local_apic_addr;
        write!(f, " local_apic_addr: {:#x},", local_apic_addr)?;
        let flags = self.flags;
        write!(f, " flags: {:#x} }}", flags)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum AcpiTable<'a> {
    Madt(&'a Madt),
    Unknown(Option<&'a str>),
}

#[derive(Debug, Eq, PartialEq)]
#[repr(C, packed)]
pub struct AcpiSdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

impl AcpiSdtHeader {
    pub fn upgrade(&self) -> AcpiTable<'_> {
        match &self.signature {
            b"APIC" => unsafe {
                let madt = core::mem::transmute::<_, *const Madt>(self as *const AcpiSdtHeader);
                AcpiTable::Madt(&*madt)
            },
            _ => AcpiTable::Unknown(core::str::from_utf8(&self.signature).ok()),
        }
    }
}

pub struct RsdtIterator<'a> {
    pointer: *const u32,
    num_items: usize,
    idx: usize,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> Iterator for RsdtIterator<'a> {
    type Item = &'a AcpiSdtHeader;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.num_items {
            return None;
        }

        unsafe {
            let child_pointer_pointer = self.pointer.add(self.idx);
            let mut child_u32: u32 = 0;
            (child_pointer_pointer as *mut u8)
                .copy_to_nonoverlapping(&mut child_u32 as *mut u32 as *mut u8, 4);
            let child_pointer = child_u32 as *const AcpiSdtHeader;
            self.idx += 1;
            Some(&*child_pointer)
        }
    }
}

#[derive(Eq)]
#[repr(C, packed)]
pub struct Rsdt {
    header: AcpiSdtHeader,
    pointers: (),
}

impl core::fmt::Debug for Rsdt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "header: {:?}", self.header)?;
        Ok(())
    }
}

impl PartialEq for Rsdt {
    fn eq(&self, other: &Self) -> bool {
        self.header == other.header
    }
}

impl Rsdt {
    pub fn iter(&self) -> RsdtIterator<'_> {
        RsdtIterator {
            pointer: &self.pointers as *const () as *const u32,
            idx: 0,
            num_items: (self.header.length as usize - core::mem::size_of::<AcpiSdtHeader>())
                / core::mem::size_of::<u32>(),
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use crate::testing::*;
    use alloc::vec::Vec;

    const RSDP: &[u8] = include_bytes!("../res/acpi/rsdp.bin");

    const MADT: &[u8] = include_bytes!("../res/acpi/madt.bin");

    const RSDT: &[u8] = include_bytes!("../res/acpi/rsdt.bin");
    const RSDT_1: &[u8] = include_bytes!("../res/acpi/rsdt/1.bin");
    const RSDT_2: &[u8] = include_bytes!("../res/acpi/rsdt/2.bin");
    const RSDT_3: &[u8] = include_bytes!("../res/acpi/rsdt/3.bin");
    const RSDT_4: &[u8] = include_bytes!("../res/acpi/rsdt/4.bin");

    create_test!(test_rsdp, {
        let mut rsdp = RSDP.to_vec();
        rsdp.resize(16, 0);
        rsdp.extend((RSDT.as_ptr() as u32).to_le_bytes());

        unsafe {
            let rsdp = &*(rsdp.as_ptr() as *const Rsdp);

            test_eq!(rsdp.signature, [82, 83, 68, 32, 80, 84, 82, 32]);
            // NOTE: checksum wrong as we healed addys
            test_eq!(rsdp.checksum, 48);
            test_eq!(rsdp.revision, 0);
            let rsdt_address = rsdp.rsdt_address;
            test_eq!(rsdt_address, RSDT.as_ptr() as u32);
            let rsdt = &*(RSDT.as_ptr() as *const Rsdt);
            test_eq!(rsdp.rsdt(), rsdt);
        }
        Ok(())
    });

    create_test!(test_madt, {
        unsafe {
            let madt = &*(MADT.as_ptr() as *const Madt);
            let entries: Vec<_> = madt.entries().collect();
            test_eq!(
                &entries,
                &[
                    MadtEntry::LocalApic {
                        acpi_id: 0,
                        apic_id: 0,
                        flags: 1
                    },
                    MadtEntry::LocalApic {
                        acpi_id: 1,
                        apic_id: 1,
                        flags: 1
                    },
                    MadtEntry::LocalApic {
                        acpi_id: 2,
                        apic_id: 2,
                        flags: 1
                    },
                    MadtEntry::LocalApic {
                        acpi_id: 3,
                        apic_id: 3,
                        flags: 1
                    },
                ]
            );

            test_eq!(madt.local_apic_addr(), 0xfee00000 as *mut u8);
        }

        Ok(())
    });

    create_test!(test_rsdt, {
        unsafe {
            // RSDT header dumped from gdb, we need to append pointers to each of the sub sections
            // since they won't be in the same memory location
            let mut rsdt = RSDT.to_vec();
            rsdt.resize(36, 0);
            rsdt.extend((RSDT_1.as_ptr() as u32).to_le_bytes());
            rsdt.extend((RSDT_2.as_ptr() as u32).to_le_bytes());
            rsdt.extend((RSDT_3.as_ptr() as u32).to_le_bytes());
            rsdt.extend((RSDT_4.as_ptr() as u32).to_le_bytes());

            let rsdt = &*(rsdt.as_ptr() as *const Rsdt);

            let signatures: Vec<_> = rsdt.iter().map(|item| &item.signature).collect();
            test_eq!(signatures, &[b"FACP", b"APIC", b"HPET", b"WAET"]);
            Ok(())
        }
    });
}
