use crate::{
    net::{self, Ipv4Protocol},
    rng::Rng,
    sleep::WakeupRequester,
    time::MonotonicTime,
    util::{
        async_channel::{self, Receiver, Sender},
        async_mutex::Mutex,
        atomic_cell::AtomicCell,
        bit_manipulation::{GetBits, SetBits},
    },
    IpAddr,
};

use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use hashbrown::HashMap;

pub struct TcpFlags(pub u8);

#[allow(unused)]
impl TcpFlags {
    pub fn cwr(&self) -> bool {
        self.0.get_bit(7)
    }

    pub fn ece(&self) -> bool {
        self.0.get_bit(6)
    }

    pub fn urg(&self) -> bool {
        self.0.get_bit(5)
    }

    pub fn ack(&self) -> bool {
        self.0.get_bit(4)
    }

    pub fn psh(&self) -> bool {
        self.0.get_bit(3)
    }

    pub fn rst(&self) -> bool {
        self.0.get_bit(2)
    }

    pub fn syn(&self) -> bool {
        self.0.get_bit(1)
    }

    pub fn fin(&self) -> bool {
        self.0.get_bit(0)
    }
}

impl core::fmt::Debug for TcpFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "cwr: {}", self.cwr())?;
        write!(f, "ece: {}", self.ece())?;
        write!(f, "urg: {}", self.urg())?;
        write!(f, "ack: {}", self.ack())?;
        write!(f, "psh: {}", self.psh())?;
        write!(f, "rst: {}", self.rst())?;
        write!(f, "syn: {}", self.syn())?;
        write!(f, "fin: {}", self.fin())?;
        Ok(())
    }
}

pub struct TcpFlagsParams {
    pub cwr: bool,
    pub ece: bool,
    pub urg: bool,
    pub ack: bool,
    pub psh: bool,
    pub rst: bool,
    pub syn: bool,
    pub fin: bool,
}

pub fn generate_tcp_flags(params: &TcpFlagsParams) -> TcpFlags {
    let mut ret = 0u8;

    ret.set_bit(7, params.cwr);
    ret.set_bit(6, params.ece);
    ret.set_bit(5, params.urg);
    ret.set_bit(4, params.ack);
    ret.set_bit(3, params.psh);
    ret.set_bit(2, params.rst);
    ret.set_bit(1, params.syn);
    ret.set_bit(0, params.fin);

    TcpFlags(ret)
}

pub struct TcpFrame<'a> {
    data: &'a [u8],
}

impl TcpFrame<'_> {
    pub(super) fn new(data: &[u8]) -> TcpFrame<'_> {
        TcpFrame { data }
    }

    pub fn source_port(&self) -> u16 {
        u16::from_be_bytes(
            self.data[0..2]
                .try_into()
                .expect("tcp source port length wrong"),
        )
    }

    pub fn dest_port(&self) -> u16 {
        u16::from_be_bytes(
            self.data[2..4]
                .try_into()
                .expect("tcp dest port length wrong"),
        )
    }

    pub fn seq_num(&self) -> u32 {
        u32::from_be_bytes(
            self.data[4..8]
                .try_into()
                .expect("tcp seq num length wrong"),
        )
    }

    pub fn ack_num(&self) -> u32 {
        u32::from_be_bytes(
            self.data[8..12]
                .try_into()
                .expect("tcp ack num length wrong"),
        )
    }

    pub fn data_offset_bytes(&self) -> usize {
        let data_offset_words = self.data[12].get_bits(4, 4);
        assert!(data_offset_words != 0);
        (data_offset_words as usize) * 4
    }

    pub fn flags(&self) -> TcpFlags {
        TcpFlags(self.data[13])
    }

    pub fn window_size(&self) -> u16 {
        u16::from_be_bytes(
            self.data[14..16]
                .try_into()
                .expect("tcp window size length wrong"),
        )
    }

    pub fn checksum(&self) -> u16 {
        u16::from_be_bytes(
            self.data[16..18]
                .try_into()
                .expect("tcp checksum length wrong"),
        )
    }

    pub fn urgent_ptr(&self) -> u16 {
        u16::from_be_bytes(
            self.data[18..20]
                .try_into()
                .expect("tcp checksum length wrong"),
        )
    }

    pub fn payload(&self) -> &[u8] {
        &self.data[self.data_offset_bytes()..]
    }
}

impl core::fmt::Debug for TcpFrame<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "source_port: {}", self.source_port())?;
        writeln!(f, "dest_port: {}", self.dest_port())?;
        writeln!(f, "seq_num: {}", self.seq_num())?;
        writeln!(f, "ack_num: {}", self.ack_num())?;
        writeln!(f, "data_offset_bytes: {}", self.data_offset_bytes())?;
        writeln!(f, "flags: {:?}", self.flags())?;
        writeln!(f, "window_size: {}", self.window_size())?;
        writeln!(f, "checksum: {:x}", self.checksum())?;
        writeln!(f, "urgent_ptr: {}", self.urgent_ptr())?;
        Ok(())
    }
}

pub struct TcpFrameParams {
    pub source_address: IpAddr,
    pub dest_address: IpAddr,
    pub source_port: u16,
    pub dest_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub flags: TcpFlags,
    pub window_size: u16,
    pub urgent_ptr: u16,
    pub payload: Arc<[u8]>,
}

pub fn generate_tcp_frame(params: &TcpFrameParams) -> Vec<u8> {
    const HEADER_LEN: usize = 20;

    let mut ret = Vec::with_capacity(HEADER_LEN + params.payload.len());

    ret.extend_from_slice(&params.source_port.to_be_bytes());
    ret.extend_from_slice(&params.dest_port.to_be_bytes());
    ret.extend_from_slice(&params.seq_num.to_be_bytes());
    ret.extend_from_slice(&params.ack_num.to_be_bytes());
    ret.push(((HEADER_LEN / 4) << 4) as u8);
    ret.push(params.flags.0);
    ret.extend_from_slice(&params.window_size.to_be_bytes());
    const CHECKSUM: u16 = 0;
    let checksum_idx = ret.len();
    ret.extend_from_slice(&CHECKSUM.to_be_bytes());
    ret.extend_from_slice(&params.urgent_ptr.to_be_bytes());
    ret.extend_from_slice(&params.payload);

    let mut checksum_frame = Vec::new();
    checksum_frame.extend_from_slice(&params.source_address);
    checksum_frame.extend_from_slice(&params.dest_address);
    checksum_frame.push(0);
    checksum_frame.push(Ipv4Protocol::Tcp.into());
    checksum_frame.extend_from_slice(&(ret.len() as u16).to_be_bytes());

    checksum_frame.extend_from_slice(&ret);

    if checksum_frame.len() % 2 != 0 {
        checksum_frame.push(0);
    }
    let checksum = net::calculate_ipv4_checksum(&checksum_frame);
    ret[checksum_idx..checksum_idx + 2].copy_from_slice(&checksum.to_be_bytes());

    ret
}

fn generate_tcp_push(
    tcp_key: &TcpKey,
    state: &mut ConnectedState,
    data: Arc<[u8]>,
) -> TcpFrameParams {
    let payload_length = data.len();
    let ret = TcpFrameParams {
        source_address: tcp_key.local_ip,
        dest_address: tcp_key.remote_ip,
        source_port: tcp_key.local_port,
        dest_port: tcp_key.remote_port,
        seq_num: state.seq_num,
        ack_num: state.outgoing_ack_num,
        flags: generate_tcp_flags(&TcpFlagsParams {
            cwr: false,
            ece: false,
            urg: false,
            ack: true,
            psh: false,
            rst: false,
            syn: false,
            fin: false,
        }),
        // FIXME: Set this to something sane?
        window_size: 512,
        urgent_ptr: 0,
        payload: data,
    };

    state.seq_num += payload_length as u32;

    ret
}

#[derive(Debug, Hash, Eq, PartialEq)]
struct TcpListenerKey {
    ip: IpAddr,
    port: u16,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct TcpKey {
    remote_ip: IpAddr,
    local_ip: IpAddr,
    remote_port: u16,
    local_port: u16,
}

struct UnackedPacket {
    #[allow(unused)]
    timestamp: usize,
    params: TcpFrameParams,
}

struct ConnectedState {
    seq_num: u32,          // Incoming seq num
    outgoing_ack_num: u32, // Outgoing ack num
    incoming_ack_num: u32,
    window_size: u16,
    dup_ack_counter: u8,
    unacknowledged: VecDeque<UnackedPacket>,
    to_send: VecDeque<Arc<[u8]>>,
    tx: Sender<Vec<u8>>,
    rx: Receiver<Arc<[u8]>>,
}

enum TcpState {
    Uninit,
    SynAckSent {
        seq_num: u32,
        ack_num: u32,
        timeout: usize,
        sent_frame: OutgoingTcpPacket,
    },
    Connected(ConnectedState),
}

pub struct TcpConnection {
    rx: Receiver<Vec<u8>>,
    tx: Sender<Arc<[u8]>>,
}

impl TcpConnection {
    pub async fn read(&self) -> Vec<u8> {
        self.rx.recv().await
    }

    pub async fn write<T>(&self, data: T)
    where
        T: Into<Arc<[u8]>>,
    {
        self.tx.send(data.into()).await;
    }
}

pub struct TcpListener {
    rx: Receiver<TcpConnection>,
}

impl TcpListener {
    pub async fn connection(&self) -> TcpConnection {
        self.rx.recv().await
    }
}

pub struct Tcp {
    listeners: Mutex<HashMap<TcpListenerKey, Sender<TcpConnection>>>,
    tcp_states: Mutex<HashMap<TcpKey, TcpState>>,
    time: Arc<MonotonicTime>,
    service_waker: AtomicCell<Waker>,
    wakeup_list: WakeupRequester,
}

impl Tcp {
    pub fn new(time: Arc<MonotonicTime>, wakeup_list: WakeupRequester) -> Tcp {
        Tcp {
            listeners: Mutex::new(Default::default()),
            tcp_states: Mutex::new(Default::default()),
            service_waker: AtomicCell::new(),
            time,
            wakeup_list,
        }
    }

    pub async fn listen(&self, ip: IpAddr, port: u16) -> TcpListener {
        let (tx, rx) = async_channel::channel();
        let ret = TcpListener { rx };
        self.listeners
            .lock()
            .await
            .insert(TcpListenerKey { ip, port }, tx);
        ret
    }

    #[allow(clippy::type_complexity)]
    pub fn handle_frame<'a>(
        &'a self,
        frame: &'a TcpFrame<'_>,
        source_ip: &'a IpAddr,
        dest_ip: &'a IpAddr,
        rng: &'a Mutex<Rng>,
    ) -> Pin<Box<dyn Future<Output = Option<Arc<[u8]>>> + 'a + Send>> {
        Box::pin(async move {
            let tcp_key = TcpKey {
                remote_ip: *source_ip,
                local_ip: *dest_ip,
                remote_port: frame.source_port(),
                local_port: frame.dest_port(),
            };

            let mut tcp_states = self.tcp_states.lock().await;
            let state = tcp_states.entry(tcp_key).or_insert(TcpState::Uninit);
            let flags = frame.flags();

            let ret = match state {
                TcpState::Uninit => {
                    if !flags.syn() {
                        return None;
                    }

                    let seq_num = rng.lock().await.u64() as u32;
                    let ack_num = frame.seq_num() + 1;
                    let response_frame = net::tcp::generate_tcp_frame(&TcpFrameParams {
                        source_address: *dest_ip,
                        dest_address: *source_ip,
                        ack_num,
                        seq_num,
                        dest_port: frame.source_port(),
                        source_port: frame.dest_port(),
                        window_size: frame.window_size(),
                        flags: net::tcp::generate_tcp_flags(&TcpFlagsParams {
                            cwr: false,
                            ece: false,
                            urg: false,
                            ack: true,
                            psh: false,
                            rst: false,
                            syn: true,
                            fin: false,
                        }),
                        urgent_ptr: 0,
                        payload: Arc::new([]),
                    })
                    .into();

                    let sent_frame = OutgoingTcpPacket {
                        local_ip: *dest_ip,
                        remote_ip: *source_ip,
                        payload: Arc::clone(&response_frame),
                    };

                    let timeout = (self.time.get() as f32 + 1.0 * self.time.tick_freq()) as usize;
                    self.wakeup_list.register_wakeup_time(timeout).await;
                    *state = TcpState::SynAckSent {
                        seq_num,
                        ack_num,
                        sent_frame,
                        timeout,
                    };

                    Some(response_frame)
                }
                TcpState::SynAckSent {
                    ack_num, seq_num, ..
                } => {
                    if flags.syn() {
                        debug!("Resetting connection, unexpected syn");
                        *state = TcpState::Uninit;
                        drop(tcp_states);
                        return self.handle_frame(frame, source_ip, dest_ip, rng).await;
                    }

                    if flags.psh() {
                        debug!("Unexpected push in syn-ack state");
                        return None;
                    }

                    if !flags.ack() {
                        debug!("Ack unset, ignoring");
                        return None;
                    }

                    if frame.seq_num() != *ack_num {
                        error!(
                            "Sequence number not expected in syn ack: ack_num: {}, seq_num: {}",
                            ack_num,
                            frame.seq_num()
                        );
                        return None;
                    }

                    let (tx_in, rx_in) = async_channel::channel();
                    let (tx_out, rx_out) = async_channel::channel();
                    let connection = TcpConnection {
                        rx: rx_in,
                        tx: tx_out,
                    };

                    let listener_key = TcpListenerKey {
                        ip: *dest_ip,
                        port: frame.dest_port(),
                    };

                    let listeners = self.listeners.lock().await;
                    let listener = match listeners.get(&listener_key) {
                        Some(x) => x,
                        None => {
                            error!("Syn ack ack for non existent listener");
                            return None;
                        }
                    };

                    *state = TcpState::Connected(ConnectedState {
                        seq_num: *seq_num + 1,
                        outgoing_ack_num: *ack_num + frame.payload().len() as u32,
                        incoming_ack_num: frame.ack_num(),
                        window_size: frame.window_size(),
                        dup_ack_counter: 0,
                        unacknowledged: VecDeque::new(),
                        to_send: VecDeque::new(),
                        tx: tx_in,
                        rx: rx_out,
                    });

                    listener.send(connection).await;

                    None
                }
                TcpState::Connected(ref mut state) => {
                    if state.outgoing_ack_num != frame.seq_num() {
                        debug!(
                            "ack num did not match seq num: {} {}",
                            state.outgoing_ack_num,
                            frame.seq_num()
                        );
                        return None;
                    }

                    state.outgoing_ack_num = frame.seq_num() + frame.payload().len() as u32;
                    if frame.ack_num() == state.incoming_ack_num {
                        state.dup_ack_counter = state.dup_ack_counter.saturating_add(1);
                    } else {
                        state.dup_ack_counter = 0;
                    }

                    state.incoming_ack_num = frame.ack_num();

                    if let Some(unacked_packet) = state.unacknowledged.front() {
                        if unacked_packet.params.seq_num < frame.ack_num() {
                            state.unacknowledged.pop_front();
                        }
                    }

                    state.window_size = frame.window_size();

                    let response_frame = net::tcp::generate_tcp_frame(&TcpFrameParams {
                        source_address: *dest_ip,
                        dest_address: *source_ip,
                        ack_num: state.outgoing_ack_num,
                        seq_num: state.seq_num,
                        dest_port: frame.source_port(),
                        source_port: frame.dest_port(),
                        window_size: frame.window_size(),
                        flags: net::tcp::generate_tcp_flags(&TcpFlagsParams {
                            cwr: false,
                            ece: false,
                            urg: false,
                            ack: true,
                            psh: false,
                            rst: false,
                            syn: false,
                            fin: false,
                        }),
                        urgent_ptr: 0,
                        payload: Arc::new([]),
                    })
                    .into();

                    if frame.flags().psh() {
                        state.tx.send(frame.payload().to_vec()).await;
                        return Some(response_frame);
                    }

                    None
                }
            };

            if let Some(service_waker) = self.service_waker.get() {
                service_waker.wake_by_ref();
            }

            ret
        })
    }

    pub async fn service(&self) -> OutgoingTcpPacket {
        OutgoingPoller {
            tcp_states: &self.tcp_states,
            time: &self.time,
            waker: &self.service_waker,
        }
        .await
    }
}

struct OutgoingPoller<'a> {
    tcp_states: &'a Mutex<HashMap<TcpKey, TcpState>>,
    time: &'a MonotonicTime,
    waker: &'a AtomicCell<Waker>,
}

impl Future for OutgoingPoller<'_> {
    type Output = OutgoingTcpPacket;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.waker.store(cx.waker().clone());

        let guard = core::pin::pin!(self.tcp_states.lock()).poll(cx);

        let mut guard = match guard {
            Poll::Ready(v) => v,
            Poll::Pending => return Poll::Pending,
        };

        for (tcp_key, tcp_state) in &mut *guard {
            match tcp_state {
                TcpState::Connected(connection) => {
                    if connection.dup_ack_counter >= 2 {
                        if let Some(packet) = connection.unacknowledged.pop_front() {
                            connection.seq_num =
                                packet.params.seq_num + packet.params.payload.len() as u32;
                            while let Some(packet) = connection.unacknowledged.pop_front() {
                                connection.to_send.push_back(packet.params.payload);
                            }
                            let payload = generate_tcp_frame(&packet.params).into();
                            let ret = OutgoingTcpPacket {
                                local_ip: tcp_key.local_ip,
                                remote_ip: tcp_key.remote_ip,
                                payload,
                            };
                            return Poll::Ready(ret);
                        }
                    }

                    //// FIXME: Check window size before sending
                    //if let Some(unacked_packet) = connection.unacknowledged.back() {
                    //    unimplemented!();
                    //}

                    let data = if let Some(data) = connection.to_send.pop_front() {
                        data
                    } else if let Poll::Ready(data) = core::pin::pin!(connection.rx.recv()).poll(cx)
                    {
                        data
                    } else {
                        continue;
                    };

                    return Poll::Ready(write_request_to_outgoing_packet(
                        tcp_key, connection, self.time, data,
                    ));
                }
                TcpState::SynAckSent {
                    ref mut timeout,
                    sent_frame,
                    ..
                } => {
                    if self.time.get() > *timeout {
                        *timeout += (self.time.tick_freq() * 1.0) as usize;
                        return Poll::Ready(sent_frame.clone());
                    }
                }
                _ => (),
            }
        }

        Poll::Pending
    }
}

fn write_request_to_outgoing_packet(
    tcp_key: &TcpKey,
    connected_state: &mut ConnectedState,
    time: &MonotonicTime,
    data: Arc<[u8]>,
) -> OutgoingTcpPacket {
    // FIXME: Hidden mutation of connected state
    let params = generate_tcp_push(tcp_key, connected_state, data);
    let payload = generate_tcp_frame(&params).into();
    connected_state.unacknowledged.push_back(UnackedPacket {
        timestamp: time.get(),
        params,
    });

    OutgoingTcpPacket {
        local_ip: tcp_key.local_ip,
        remote_ip: tcp_key.remote_ip,
        payload,
    }
}

#[derive(Clone)]
pub struct OutgoingTcpPacket {
    pub local_ip: IpAddr,
    pub remote_ip: IpAddr,
    pub payload: Arc<[u8]>,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;
    use crate::MonotonicTime;
    use alloc::string::{String, ToString};

    struct TcpFixture {
        time: Arc<MonotonicTime>,
        tcp: Tcp,
        rng: Mutex<Rng>,
    }

    fn gen_fixture() -> TcpFixture {
        let time = Arc::new(MonotonicTime::new(10.0));
        let (wakeup_list, _, _) = crate::sleep::construct_wakeup_handlers();
        let rng = Mutex::new(Rng::new(0));

        let tcp = Tcp::new(Arc::clone(&time), wakeup_list);

        TcpFixture { time, tcp, rng }
    }

    struct MockClient {
        client_ip: IpAddr,
        server_ip: IpAddr,
        client_port: u16,
        server_port: u16,
        window_size: u16,
        seq: u32,
        ack: u32,
    }

    impl MockClient {
        fn syn(&mut self) -> Arc<[u8]> {
            let ret = generate_tcp_frame(&TcpFrameParams {
                source_address: self.client_ip,
                dest_address: self.server_ip,
                source_port: self.client_port,
                dest_port: self.server_port,
                seq_num: self.seq,
                // Syn should always have ack 0 as we have nothing to ack
                ack_num: 0,
                flags: generate_tcp_flags(&TcpFlagsParams {
                    cwr: false,
                    ece: false,
                    urg: false,
                    ack: false,
                    psh: false,
                    rst: false,
                    syn: true,
                    fin: false,
                }),
                window_size: self.window_size,
                urgent_ptr: 0,
                payload: Arc::new([]),
            })
            .into();
            self.seq += 1;
            ret
        }

        fn ack(&self) -> Arc<[u8]> {
            generate_tcp_frame(&TcpFrameParams {
                source_address: self.client_ip,
                dest_address: self.server_ip,
                source_port: self.client_port,
                dest_port: self.server_port,
                seq_num: self.seq,
                ack_num: self.ack,
                flags: generate_tcp_flags(&TcpFlagsParams {
                    cwr: false,
                    ece: false,
                    urg: false,
                    ack: true,
                    psh: false,
                    rst: false,
                    syn: false,
                    fin: false,
                }),
                window_size: self.window_size,
                urgent_ptr: 0,
                payload: Arc::new([]),
            })
            .into()
        }

        async fn handshake(&mut self, fixture: &TcpFixture) -> Result<(), String> {
            let syn = self.syn();

            let syn_ack = match fixture
                .tcp
                .handle_frame(
                    &TcpFrame::new(&syn),
                    &self.client_ip,
                    &self.server_ip,
                    &fixture.rng,
                )
                .await
            {
                Some(v) => v,
                None => {
                    return Err("No syn ack for syn".into());
                }
            };

            self.handle_frame(&syn_ack);

            let ack = self.ack();

            let response = fixture
                .tcp
                .handle_frame(
                    &TcpFrame::new(&ack),
                    &self.client_ip,
                    &self.server_ip,
                    &fixture.rng,
                )
                .await;

            test_true!(response.is_none());

            Ok(())
        }

        fn handle_frame(&mut self, buf: &[u8]) {
            let frame = TcpFrame::new(buf);
            let seq = frame.seq_num();
            let payload_len = frame.payload().len();

            if self.ack == seq {
                self.ack = seq + payload_len as u32;
            }
        }
    }

    create_test!(test_tcp_frame_parsing, {
        const TCP_SYN: &[u8] = &[
            0x80, 0xd8, 0x17, 0x70, 0x5a, 0x5b, 0x14, 0x47, 0x00, 0x00, 0x00, 0x00, 0xa0, 0x02,
            0xfa, 0xf0, 0x7e, 0xa4, 0x00, 0x00, 0x02, 0x04, 0x05, 0xb4, 0x04, 0x02, 0x08, 0x0a,
            0x41, 0xcf, 0x00, 0x5d, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03, 0x03, 0x07,
        ];

        let frame = TcpFrame::new(TCP_SYN);
        test_eq!(frame.source_port(), 32984);
        test_eq!(frame.dest_port(), 6000);
        test_eq!(frame.seq_num(), 1515918407);
        test_eq!(frame.ack_num(), 0);
        test_eq!(frame.data_offset_bytes(), 40);
        test_eq!(frame.flags().0, 2);
        test_eq!(frame.window_size(), 64240);
        test_eq!(frame.checksum(), 0x7ea4);
        test_eq!(frame.urgent_ptr(), 0);

        Ok(())
    });

    create_test!(test_tcp_flags_back_and_forth, {
        let flags = generate_tcp_flags(&TcpFlagsParams {
            cwr: true,
            ece: false,
            urg: false,
            ack: true,
            psh: false,
            rst: true,
            syn: true,
            fin: true,
        });

        test_eq!(flags.cwr(), true);
        test_eq!(flags.ece(), false);
        test_eq!(flags.urg(), false);
        test_eq!(flags.ack(), true);
        test_eq!(flags.psh(), false);
        test_eq!(flags.rst(), true);
        test_eq!(flags.syn(), true);
        test_eq!(flags.fin(), true);

        Ok(())
    });

    create_test!(test_dropped_syn_ack_ack, {
        const TCP_SYN: &[u8] = b"\x89\x06\x27\x0f\xcc\x6b\x38\x32\x00\x00\x00\x00\xa0\x02\xfa\xf0\x22\xb5\x00\x00\x02\x04\x05\xb4\x04\x02\x08\x0a\xc3\x8b\x2c\xc7\x00\x00\x00\x00\x01\x03\x03\x07";
        const TCP_ACK: &[u8] =
            b"\x89\x06\x27\x0f\xcc\x6b\x38\x33\x00\x39\x84\x21\x50\x10\xfa\xf0\xf6\x80\x00\x00";
        const SOURCE_IP: IpAddr = [192, 168, 2, 1];
        const DEST_IP: IpAddr = [192, 168, 2, 2];

        let fixture = gen_fixture();

        let listener = fixture.tcp.listen(DEST_IP, 9999).await;

        let frame = TcpFrame::new(TCP_SYN);
        fixture
            .tcp
            .handle_frame(&frame, &SOURCE_IP, &DEST_IP, &fixture.rng)
            .await;

        // We should get a syn-ack response from the initial syn
        if crate::future::poll_immediate(fixture.tcp.service())
            .await
            .is_some()
        {
            return Err("Unexpected serivce response".into());
        }

        // After 2 seconds we should have waited enough to trigger a syn-ack resend
        fixture
            .time
            .set_tick((fixture.time.tick_freq() * 2.0) as usize);

        let syn_ack = match crate::future::poll_immediate(fixture.tcp.service()).await {
            Some(v) => v,
            None => return Err("Syn ack retransmit missing".into()),
        };

        let syn_ack = TcpFrame::new(&syn_ack.payload);

        test_true!(syn_ack.flags().syn());
        test_true!(syn_ack.flags().ack());

        let frame = TcpFrame::new(TCP_ACK);
        fixture
            .tcp
            .handle_frame(&frame, &SOURCE_IP, &DEST_IP, &fixture.rng)
            .await;

        if crate::future::poll_immediate(listener.connection())
            .await
            .is_none()
        {
            return Err("Connection not ready".into());
        }

        Ok(())
    });

    create_test!(test_dup_ack_retransmission, {
        const CLIENT_IP: IpAddr = [192, 168, 2, 1];
        const SERVER_IP: IpAddr = [192, 168, 2, 2];
        const CLIENT_PORT: u16 = 1234;
        const SERVER_PORT: u16 = 5678;

        let fixture = gen_fixture();

        let listener = fixture.tcp.listen(SERVER_IP, SERVER_PORT).await;

        let mut mock_client = MockClient {
            client_ip: CLIENT_IP,
            server_ip: SERVER_IP,
            client_port: CLIENT_PORT,
            server_port: SERVER_PORT,
            window_size: 5000,
            seq: 150,
            ack: 0,
        };

        mock_client.handshake(&fixture).await?;

        let connection = crate::future::poll_immediate(listener.connection())
            .await
            .ok_or("Connection not ready".to_string())?;

        connection.write(Arc::<str>::from("hello world")).await;
        connection.write(Arc::<str>::from("hello world 2")).await;

        let frame = crate::future::poll_immediate(fixture.tcp.service())
            .await
            .ok_or("tcp service did not return a value".to_string())?;

        mock_client.handle_frame(&frame.payload);

        let frame = TcpFrame::new(&frame.payload);
        test_eq!(frame.payload(), b"hello world");

        let data1_ack = mock_client.ack();

        let response = fixture
            .tcp
            .handle_frame(
                &TcpFrame::new(&data1_ack),
                &CLIENT_IP,
                &SERVER_IP,
                &fixture.rng,
            )
            .await;

        test_true!(response.is_none());

        let frame = crate::future::poll_immediate(fixture.tcp.service())
            .await
            .ok_or("tcp service did not return a value".to_string())?;

        let frame = TcpFrame::new(&frame.payload);
        test_eq!(frame.payload(), b"hello world 2");

        // Intentionally do not inform the mock of the second frame

        // After we've sent first two packets, nothing to do
        test_true!(crate::future::poll_immediate(fixture.tcp.service())
            .await
            .is_none());

        for _ in 0..2 {
            // ACK first segment 2 more times
            let response = fixture
                .tcp
                .handle_frame(
                    &TcpFrame::new(&data1_ack),
                    &CLIENT_IP,
                    &SERVER_IP,
                    &fixture.rng,
                )
                .await;
            test_true!(response.is_none());
        }

        // 3 acks, retransmission please
        test_true!(crate::future::poll_immediate(fixture.tcp.service())
            .await
            .is_some());

        Ok(())
    });
}
