use crate::{
    acpi::{Madt, MadtEntry},
    time::MonotonicTime,
    util::{
        async_mutex::Mutex,
        atomic_cell::AtomicCell,
        bit_manipulation::{GetBits, SetBits},
        spinlock::SpinLock,
    },
};

use alloc::{boxed::Box, collections::VecDeque, sync::Arc};

use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

use hashbrown::HashMap;

extern "C" {
    static max_num_cpus: u32;
    static ap_trampoline_size: u32;
    fn ap_trampoline();
}

pub fn cpuid() -> u8 {
    let mut ebx: u32;
    unsafe {
        core::arch::asm!(
            r#"
                mov $1, %eax
                cpuid
            "#,
            out("eax") _,
            out("ebx") ebx,
            out("ecx") _,
            out("edx") _,
            options(att_syntax),
        );
    }
    (ebx >> 24) as u8
}

unsafe fn select_ap(id: u8, icr_high: *mut u32) {
    let mut high_val = icr_high.read_volatile();
    high_val.set_bits(24, 8, id as u32);
    icr_high.write_volatile(high_val);
}

unsafe fn wait_ipi_delivered(icr_low: *mut u32) {
    while icr_low.read_volatile().get_bit(12) {}
}

#[allow(unused)]
enum DestinationShorthand {
    None,
    OnlySelf,
    AllIncludingSelf,
    AllExcludingSelf,
}

#[allow(unused)]
enum DeliveryMode {
    Fixed,
    LowestPriority,
    Smi,
    Nmi,
    Init,
    StartUp,
}

#[allow(unused)]
enum DestinationMode {
    Physical,
    Logical,
}

#[allow(unused)]
enum DeliveryStatus {
    Idle,
    Pending,
}

#[allow(unused)]
enum Level {
    Deassert,
    Assert,
}

#[allow(unused)]
enum TriggerMode {
    Edge,
    Level,
}

struct InterruptCommand {
    destination_shorthand: DestinationShorthand,
    delivery_mode: DeliveryMode,
    destination_mode: DestinationMode,
    delivery_status: DeliveryStatus,
    level: Level,
    trigger_mode: TriggerMode,
    vector: u8,
}

impl InterruptCommand {
    fn to_u32(&self, initial_val: u32) -> u32 {
        let mut ret = initial_val;

        let delivery_mode = match self.delivery_mode {
            DeliveryMode::Fixed => 0b000,
            DeliveryMode::LowestPriority => 0b001,
            DeliveryMode::Smi => 0b010,
            DeliveryMode::Nmi => 0b100,
            DeliveryMode::Init => 0b101,
            DeliveryMode::StartUp => 0b110,
        };

        let destination_mode = match self.destination_mode {
            DestinationMode::Physical => 0,
            DestinationMode::Logical => 1,
        };

        let delivery_status = match self.delivery_status {
            DeliveryStatus::Idle => 0,
            DeliveryStatus::Pending => 1,
        };

        let level = match self.level {
            Level::Deassert => 0,
            Level::Assert => 1,
        };

        let trigger_mode = match self.trigger_mode {
            TriggerMode::Edge => 0,
            TriggerMode::Level => 1,
        };

        let destination_shorthand = match self.destination_shorthand {
            DestinationShorthand::None => 0b00,
            DestinationShorthand::OnlySelf => 0b01,
            DestinationShorthand::AllIncludingSelf => 0b10,
            DestinationShorthand::AllExcludingSelf => 0b11,
        };

        ret.set_bits(0, 8, self.vector as u32);
        ret.set_bits(8, 3, delivery_mode);
        ret.set_bits(11, 1, destination_mode);
        ret.set_bits(12, 1, delivery_status);
        ret.set_bits(14, 1, level);
        ret.set_bits(15, 1, trigger_mode);
        ret.set_bits(18, 2, destination_shorthand);

        ret
    }
}

unsafe fn send_init_ipi(id: u8, icr_high: *mut u32, icr_low: *mut u32) {
    select_ap(id, icr_high);

    let low_val = icr_low.read_volatile();
    let command = InterruptCommand {
        delivery_mode: DeliveryMode::Init,
        delivery_status: DeliveryStatus::Idle,
        destination_shorthand: DestinationShorthand::None,
        destination_mode: DestinationMode::Physical,
        level: Level::Assert,
        trigger_mode: TriggerMode::Level,
        vector: 0x00,
    };
    icr_low.write_volatile(command.to_u32(low_val));
    wait_ipi_delivered(icr_low);
}

unsafe fn send_deinit_ipi(id: u8, icr_high: *mut u32, icr_low: *mut u32) {
    select_ap(id, icr_high);

    let low_val = icr_low.read_volatile();
    let command = InterruptCommand {
        delivery_mode: DeliveryMode::Init,
        delivery_status: DeliveryStatus::Idle,
        destination_shorthand: DestinationShorthand::None,
        destination_mode: DestinationMode::Physical,
        level: Level::Deassert,
        trigger_mode: TriggerMode::Level,
        vector: 0x00,
    };
    icr_low.write_volatile(command.to_u32(low_val));
    wait_ipi_delivered(icr_low);
}

unsafe fn send_startup_ipi(id: u8, icr_high: *mut u32, icr_low: *mut u32) {
    select_ap(id, icr_high);

    let command = InterruptCommand {
        delivery_status: DeliveryStatus::Idle,
        level: Level::Deassert,
        destination_mode: DestinationMode::Physical,
        destination_shorthand: DestinationShorthand::None,
        trigger_mode: TriggerMode::Edge,
        delivery_mode: DeliveryMode::StartUp,
        vector: 0x8,
    };
    let low_val = icr_low.read_volatile();
    icr_low.write_volatile(command.to_u32(low_val));
}

unsafe fn busy_wait(time_s: f32, time: &MonotonicTime) {
    let start_time = time.get();
    let end_time = start_time as f32 + time_s * time.tick_freq();
    let end_time = (end_time + 0.999999) as usize;
    while time.get() < end_time {}
}

pub fn boot_apic(id: u8, apic_ptr: *mut u8, time: &MonotonicTime) {
    unsafe {
        let icr_high = apic_ptr.add(0x310) as *mut u32;
        let icr_low = apic_ptr.add(0x300) as *mut u32;

        send_init_ipi(id, icr_high, icr_low);
        send_deinit_ipi(id, icr_high, icr_low);

        // NOTE: There are some missed checks in here, and timing is not correct
        busy_wait(0.1, time);
        send_startup_ipi(id, icr_high, icr_low);
        busy_wait(0.1, time);
        send_startup_ipi(id, icr_high, icr_low);
    }
}

pub fn prepare_trampoline() {
    unsafe {
        // Why would we do this?
        // * Startup IPI needs to use an address from a low memory region
        // * We attempted to use the linker script to put our trampoline at some low address
        //   automagically, but did not see the code actually there when we booted
        // * The OSDev wiki page on symmetric multiprocessing does this
        crate::libc::memcpy(
            0x8000 as *mut u8,
            ap_trampoline as *const u8,
            ap_trampoline_size as usize,
        );
    }
}

type FnQueue = VecDeque<Box<dyn FnOnce() + Send>>;
type SharedFnQueue = Arc<SpinLock<FnQueue>>;
struct BootInfo {
    id: u32,
    queue: SharedFnQueue,
}

static BOOT_INFO_QUEUE: SpinLock<VecDeque<BootInfo>> = SpinLock::new(VecDeque::new());

#[no_mangle]
pub extern "C" fn ap_startup() {
    let fn_queue = Arc::new(SpinLock::new(FnQueue::new()));

    let cpu_id = cpuid();

    {
        let mut boot_info_queue = BOOT_INFO_QUEUE.lock();
        boot_info_queue.push_back(BootInfo {
            id: cpu_id as u32,
            queue: Arc::clone(&fn_queue),
        });

        if let Some(waker) = WAKER.get() {
            waker.wake_by_ref();
        }
    }

    info!("cpu {cpu_id} booted");

    loop {
        let mut fns = fn_queue.lock();
        if let Some(f) = fns.pop_front() {
            f();
        }

        // FIXME: Halt cpu and wait for something interesting to happen
    }
}

#[derive(Debug)]
pub struct ExecuteError;

#[derive(Debug)]
pub struct CpuFnDispatcherError;

static DISPATCHER_ACTIVE: AtomicBool = AtomicBool::new(false);
static WAKER: AtomicCell<Waker> = AtomicCell::new();

pub struct CpuFnDispatcher {
    cpus: Mutex<HashMap<u32, SharedFnQueue>>,
}

impl CpuFnDispatcher {
    pub fn new() -> Result<CpuFnDispatcher, CpuFnDispatcherError> {
        loop {
            let active = DISPATCHER_ACTIVE.load(Ordering::Relaxed);

            if DISPATCHER_ACTIVE.load(Ordering::Relaxed) {
                return Err(CpuFnDispatcherError);
            }

            if DISPATCHER_ACTIVE
                .compare_exchange_weak(active, true, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }

        Ok(CpuFnDispatcher {
            cpus: Mutex::new(HashMap::new()),
        })
    }

    pub async fn cpus(&self) -> impl Iterator<Item = u32> {
        let cpus = self.cpus.lock().await;
        let keys: alloc::vec::Vec<_> = cpus.keys().cloned().collect();
        keys.into_iter()
    }

    pub async fn execute<F: FnOnce() + Send + 'static>(
        &self,
        cpu_id: u32,
        f: F,
    ) -> Result<(), ExecuteError> {
        let cpus = self.cpus.lock().await;
        let queue = cpus.get(&cpu_id).ok_or(ExecuteError)?;
        let mut queue = queue.lock();
        queue.push_back(Box::new(f));
        Ok(())
    }

    pub async fn service(&self) {
        struct BootInfoQueueAvailable;

        impl Future for BootInfoQueueAvailable {
            type Output = BootInfo;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let mut queue = BOOT_INFO_QUEUE.lock();
                match queue.pop_front() {
                    Some(v) => Poll::Ready(v),
                    None => {
                        WAKER.store(cx.waker().clone());
                        Poll::Pending
                    }
                }
            }
        }

        loop {
            let info = BootInfoQueueAvailable.await;
            info!("Found boot info for cpu {}", info.id);
            let mut cpus = self.cpus.lock().await;
            cpus.insert(info.id, info.queue);
        }
    }
}

pub const BSP_ID: u8 = 0;

pub fn boot_all_cpus(madt: &Madt, time: &MonotonicTime) {
    let bsp_id = cpuid();
    assert_eq!(bsp_id, BSP_ID);
    prepare_trampoline();

    for entry in madt.entries() {
        let MadtEntry::LocalApic { apic_id, .. } = entry;

        unsafe {
            if apic_id as u32 >= max_num_cpus {
                error!("Cannot boot processor with ID {apic_id} >= {max_num_cpus}");
                continue;
            }
        }

        if apic_id != bsp_id {
            crate::multiprocessing::boot_apic(apic_id, madt.local_apic_addr(), time);
        }
    }
}
