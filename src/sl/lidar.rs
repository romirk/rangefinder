use crate::sl::cmd::ScanModeConfEntry::*;
use crate::sl::cmd::SlLidarCmd::{GetDeviceHealth, GetDeviceInfo, GetLidarConf, GetSampleRate, HQMotorSpeedCtrl, Reset, Scan, Stop};
use crate::sl::cmd::{ScanModeConfEntry, SlLidarResponseDeviceHealthT, SlLidarResponseDeviceInfoT, SlLidarResponseGetLidarConf, SlLidarResponseSampleRateT};
use crate::sl::error::RxError;
use crate::sl::error::RxError::{Corrupted, PortError};
use crate::sl::lidar::LidarState::{Idle, Scanning};
use crate::sl::serial::SerialPortChannel;
use crate::sl::{Channel, Response, ResponseDescriptor};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

const S1_BAUD: u32 = 256000;

#[derive(Debug, Clone)]
enum LidarState {
    Idle,
    Processing,
    Scanning,
    ProtectionStop,
}

#[derive(Debug, Clone)]
pub struct Sample {
    start: bool,
    intensity: u8,
    pub(crate) angle: u16,
    pub(crate) distance: u16,
}

pub struct Lidar {
    state: Arc<Mutex<LidarState>>,
    channel: Arc<Mutex<SerialPortChannel>>,

    thread_handle: Option<thread::JoinHandle<()>>,
    scan_buffer: Arc<Mutex<Vec<Sample>>>,
}

impl Lidar {
    pub fn init(port: String) -> Lidar {
        match SerialPortChannel::bind(port, S1_BAUD) {
            Ok(channel) => Lidar {
                state: Arc::new(Mutex::new(Idle)),
                channel: Arc::new(Mutex::from(*channel)),
                thread_handle: None,
                scan_buffer: Arc::new(Mutex::new(Vec::with_capacity(2048))),
            },
            Err(e) => panic!("Unable to bind serial port: {}", e),
        }
    }

    fn checksum(payload: &[u8]) -> u8 {
        payload.iter().fold(0, |acc, x| acc ^ x)
    }

    fn single_req(&mut self, req: &[u8]) -> Result<Response, RxError> {
        let mut channel = self.channel.lock().unwrap();
        match channel.write(&req) {
            Ok(()) => Lidar::rx(channel),
            Err(e) => Err(PortError(e))
        }
    }

    fn rx(mut channel: MutexGuard<SerialPortChannel>) -> Result<Response, RxError> {
        // response header
        let mut descriptor_bytes = [0u8; 7];
        match channel.read(&mut descriptor_bytes) {
            Ok(()) => {}
            Err(e) => return Err(PortError(e))
        }

        if descriptor_bytes[0..2] != [0xa5, 0x5a] {
            return Err(Corrupted(descriptor_bytes));
        }

        let send_mode = (descriptor_bytes[5] & 0b11000000) >> 6;
        let data_type = descriptor_bytes[6];

        descriptor_bytes[5] = descriptor_bytes[5] ^ (descriptor_bytes[5] & 0b11000000);
        let len = crate::util::read_le_u32(&mut &descriptor_bytes[2..6]);

        let descriptor = ResponseDescriptor {
            len,
            send_mode,
            data_type,
        };

        // data
        let mut data = vec![0u8; descriptor.len as usize];
        match channel.read(&mut data) {
            Ok(()) => {}
            Err(e) => return Err(PortError(e))
        }

        Ok(Response {
            descriptor,
            data,
        })
    }

    pub fn stop(&mut self, reset: bool) {
        match self.channel.lock().unwrap().write(&[0xa5, (if reset { Reset } else { Stop }) as u8]) {
            Ok(()) => {
                *self.state.lock().unwrap() = Idle;
                sleep(Duration::from_millis(2));
            }
            Err(e) => panic!("Unable to stop lidar: {}", e),
        }
    }

    pub fn reset(&mut self) { self.stop(true); }

    fn set_motor_speed(&mut self, speed: u16) {
        let speed_bytes = speed.to_le_bytes();
        let mut req = [0xa5, HQMotorSpeedCtrl as u8, 0x02, speed_bytes[0], speed_bytes[1], 0];
        req[5] = Lidar::checksum(&req);
        self.channel.lock().unwrap().write(&req).expect("Set motor speed failed");
    }

    pub fn get_info(&mut self) -> SlLidarResponseDeviceInfoT {
        let res = self.single_req(&[0xa5, GetDeviceInfo as u8]).expect("Could not read device info");
        let data = res.data;

        SlLidarResponseDeviceInfoT {
            model: data[0],
            firmware_version: ((data[2] as u16) << 8) | data[1] as u16,
            hardware_version: data[3],
            serial_number: data[4..20].try_into().unwrap(),
        }
    }

    pub fn get_health(&mut self) -> SlLidarResponseDeviceHealthT {
        let res = self.single_req(&[0xa5, GetDeviceHealth as u8]).expect("Could not read device health");
        let data = res.data;

        SlLidarResponseDeviceHealthT {
            status: data[0],
            error_code: ((data[2] as u16) << 8) | data[1] as u16,
        }
    }

    pub fn get_sample_rate(&mut self) -> SlLidarResponseSampleRateT {
        let res = self.single_req(&[0xa5, GetSampleRate as u8]).expect("Could not read sample rate");
        let data = res.data;

        SlLidarResponseSampleRateT {
            std_sample_duration_us: ((data[1] as u16) << 8) | data[0] as u16,
            express_sample_duration_us: ((data[3] as u16) << 8) | data[2] as u16,
        }
    }

    pub fn get_lidar_conf(&mut self, entry: ScanModeConfEntry, payload: Option<u16>) -> SlLidarResponseGetLidarConf {
        let mut req = [0u8; 12];

        req[0] = 0xa5;
        req[1] = GetLidarConf as u8;
        req[3..7].copy_from_slice((entry as u32).to_le_bytes().as_ref());

        match entry {
            Count | Typical => {
                req[2] = 4;
                req[7] = Lidar::checksum(&req[..7]);
            }
            _ => {
                req[2] = 8;
                req[7..9].copy_from_slice(payload.unwrap().to_le_bytes().as_ref());
                req[11] = Lidar::checksum(&req[..11]);
            }
        }

        let res = self.single_req(&req[..(match entry {
            Count | Typical => 8,
            _ => 12
        })]).expect("Could not read lidar conf");
        let data = res.data;

        SlLidarResponseGetLidarConf {
            conf_type: u32::from_le_bytes(data[..4].try_into().unwrap()),
            payload: data[4..].to_owned(),
        }
    }

    pub fn start_scan(&mut self) {
        // signal lidar to begin a scan
        let buffer = Arc::clone(&self.scan_buffer);
        let channel_arc = self.channel.clone();

        match (|| { return self.channel.lock().unwrap().write(&[0xa5, Scan as u8]); })() {
            Ok(()) => {
                *self.state.lock().unwrap() = Scanning;

                let state = Arc::clone(&self.state);

                sleep(Duration::from_millis(1000));

                // start reader thread
                self.thread_handle = Some(thread::spawn(move || {
                    Self::reader_thread(buffer, channel_arc, state);
                }));
            }
            Err(e) => { panic!("{:?}", e) }
        }
    }

    fn reader_thread(buffer: Arc<Mutex<Vec<Sample>>>, channel_arc: Arc<Mutex<SerialPortChannel>>, state: Arc<Mutex<LidarState>>) {
        let mut seeking = true;
        let mut descriptor = [0u8; 7];
        {
            channel_arc.lock().unwrap().read(&mut descriptor).expect("missing descriptor");
        }

        loop {
            let mode = state.lock().unwrap().clone();

            match mode {
                Scanning => {}
                mode => {
                    println!("Not scanning: {:?}", mode);
                    break;
                }
            }
            let mut data = [0u8; 5];

            match channel_arc.try_lock() {
                Err(_) => {
                    sleep(Duration::from_millis(100));
                    continue;
                }
                Ok(mut channel) =>
                    match channel.read(&mut data) {
                        Err(err) => {
                            println!("{}", err);
                            continue;
                        }
                        Ok(()) => {}
                    },
            }

            // checks
            if !(data[0] & 0b01 == !data[0] & 0b10 && data[1] & 0b01 == 1) {
                println!("parity failed: {:x?}", data);
            }

            let sample = Sample {
                start: (data[0] & 1) != 0,
                intensity: data[0] >> 2,
                angle: (((data[2] as u16) << 8) | (data[1] as u16 >> 1)) / 64,
                distance: (((data[4] as u16) << 8) | data[3] as u16) / 4,
            };

            if seeking && !sample.start { continue; }

            seeking = false;
            match buffer.lock() {
                Ok(mut buf) => { buf.push(sample); }
                Err(_) => { println!("Failed to lock buffer"); }
            }
        }
    }
    // pub fn get_sample(&self) -> Result<Sample, RxError> {
    //     let start = Instant::now();
    //     let timeout = Duration::from_millis(10000);
    //     loop {
    //         {
    //             let head = self.scan_buffer.lock().unwrap().pop_front();
    //             match head {
    //                 None => {
    //                     if start.elapsed() > timeout { return Err(TimedOut); }
    //                 }
    //                 Some(sample) => { return Ok(sample); }
    //             }
    //         }
    //         sleep(Duration::from_millis(1000));
    //     }
    // }

    pub fn get_n_samples(&self, n: u32) -> Vec<Sample> {
        loop {
            {
                let buffer = self.scan_buffer.lock().unwrap();
                if buffer.len() >= n as usize { break; }
            }
            sleep(Duration::from_secs(1));
        }
        (*self.scan_buffer.lock().unwrap()).clone().into_iter().take(n as usize).collect()
    }

    pub fn join(&mut self) {
        if let Some(handle) = self.thread_handle.take() {
            handle.join().unwrap();
        }
    }
}

