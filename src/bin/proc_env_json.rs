use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr, IfIsHumanReadable};
use std::{fmt, net::IpAddr, str::FromStr, time::Instant};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, Networks, RefreshKind, System};
use tokio::task::{spawn_blocking, JoinError};
use wgpu::{Backend, Backends, Dx12Compiler, Instance, InstanceDescriptor, InstanceFlags};

// Need to gate this under `experimental` feature flag.
#[doc(hidden)]
pub fn current_exe_name() -> std::io::Result<String> {
    Ok(std::env::current_exe()?
        .file_name()
        .expect("this should not happen here")
        .to_string_lossy()
        .into())
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct MacAddr(#[serde_as(as = "IfIsHumanReadable<DisplayFromStr>")] MacIntenal);

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[serde(transparent)]
struct MacIntenal([u8; 6]);

impl fmt::Display for MacIntenal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        mac_address::MacAddress::new(self.0).fmt(f)
    }
}

impl FromStr for MacIntenal {
    type Err = mac_address::MacParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(mac_address::MacAddress::from_str(s)?.bytes()))
    }
}

impl MacAddr {
    pub fn bytes(&self) -> [u8; 6] {
        self.0 .0
    }
}

impl From<sysinfo::MacAddr> for MacAddr {
    fn from(mac: sysinfo::MacAddr) -> Self {
        Self(MacIntenal(mac.0))
    }
}

impl From<MacAddr> for sysinfo::MacAddr {
    fn from(mac: MacAddr) -> Self {
        Self(mac.bytes())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IpNetwork {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl From<sysinfo::IpNetwork> for IpNetwork {
    fn from(ip: sysinfo::IpNetwork) -> Self {
        Self {
            addr: ip.addr,
            prefix: ip.prefix,
        }
    }
}

impl From<IpNetwork> for sysinfo::IpNetwork {
    fn from(ip: IpNetwork) -> Self {
        Self {
            addr: ip.addr,
            prefix: ip.prefix,
        }
    }
}

#[repr(u8)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum DeviceType {
    Other,
    IntegratedGpu,
    DiscreteGpu,
    VirtualGpu,
    Cpu,
}

impl From<wgpu::DeviceType> for DeviceType {
    fn from(device_type: wgpu::DeviceType) -> Self {
        match device_type {
            wgpu::DeviceType::Other => Self::Other,
            wgpu::DeviceType::IntegratedGpu => Self::IntegratedGpu,
            wgpu::DeviceType::DiscreteGpu => Self::DiscreteGpu,
            wgpu::DeviceType::VirtualGpu => Self::VirtualGpu,
            wgpu::DeviceType::Cpu => Self::Cpu,
        }
    }
}

impl From<DeviceType> for wgpu::DeviceType {
    fn from(device_type: DeviceType) -> Self {
        match device_type {
            DeviceType::Other => Self::Other,
            DeviceType::IntegratedGpu => Self::IntegratedGpu,
            DeviceType::DiscreteGpu => Self::DiscreteGpu,
            DeviceType::VirtualGpu => Self::VirtualGpu,
            DeviceType::Cpu => Self::Cpu,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct SystemEnv {
    pub name: Option<String>,
    pub kernel_version: Option<String>,
    pub os_version: Option<String>,
    pub long_os_version: Option<String>,
    pub distribution_id: String,
}

impl SystemEnv {
    pub fn create() -> Self {
        Self {
            name: System::name(),
            kernel_version: System::kernel_version(),
            os_version: System::os_version(),
            long_os_version: System::long_os_version(),
            distribution_id: System::distribution_id(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct CpuEnv {
    pub physical_core_count: Option<usize>,
    pub logical_core_count: usize,
    pub arch: String,
    pub frequency: u64,
    pub vendor_id: String,
    pub brand: String,
}

impl CpuEnv {
    pub fn create(sys: &System) -> Self {
        let cpus = sys.cpus();
        let cpu0 = &cpus[0];

        Self {
            physical_core_count: sys.physical_core_count(),
            logical_core_count: cpus.len(),
            arch: System::cpu_arch(),
            frequency: cpu0.frequency(),
            vendor_id: cpu0.vendor_id().into(),
            brand: cpu0.brand().into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct MemoryEnv {
    pub total_memory: u64,
    pub used_memory: u64,
}

impl MemoryEnv {
    pub fn create(sys: &System) -> Self {
        Self {
            total_memory: sys.total_memory(),
            used_memory: sys.used_memory(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct NetworkEnv {
    pub mac_address: MacAddr,
    pub mtu: u64,
    pub ip_networks: Vec<IpNetwork>,
}

impl NetworkEnv {
    pub fn create_map() -> IndexMap<String, Self> {
        let mut map: IndexMap<String, Self> = Networks::new_with_refreshed_list()
            .iter()
            .map(|(k, v)| {
                let mut ip_networks = v.ip_networks().to_vec();
                ip_networks.sort();
                (
                    k.into(),
                    Self {
                        mac_address: v.mac_address().into(),
                        mtu: v.mtu(),
                        ip_networks: ip_networks.iter().map(|ip| (*ip).into()).collect(),
                    },
                )
            })
            .collect();
        map.sort_unstable_keys();
        map
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct GpuEnv {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: DeviceType,
    pub driver: String,
    pub driver_info: String,
    pub backend: Backend,
}

impl GpuEnv {
    pub fn create_list() -> Vec<Self> {
        let adapters = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            flags: InstanceFlags::from_build_config(),
            dx12_shader_compiler: Dx12Compiler::Dxc {
                dxil_path: None,
                dxc_path: None,
            },
            gles_minor_version: Default::default(),
        })
        .enumerate_adapters(Backends::all());

        adapters
            .iter()
            .map(|adapter| {
                let info = adapter.get_info();
                Self {
                    name: info.name,
                    vendor_id: info.vendor,
                    device_id: info.device,
                    device_type: info.device_type.into(),
                    driver: info.driver,
                    driver_info: info.driver_info,
                    backend: info.backend,
                }
            })
            .collect()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct ProcEnv {
    pub proc_id: u32,
    pub proc_name: Option<String>,
    pub host_name: Option<String>,
    pub system: SystemEnv,
    pub cpu: CpuEnv,
    pub memory: MemoryEnv,
    pub networks: IndexMap<String, NetworkEnv>,
    pub wgpu_adapters: Vec<GpuEnv>,
}

impl ProcEnv {
    pub fn create() -> Self {
        let total_time = Instant::now();

        let sys_time = Instant::now();
        let proc_id = std::process::id();
        let proc_name = current_exe_name().ok();
        let host_name = System::host_name();
        let system = SystemEnv::create();
        println!("sys_time: {} s", sys_time.elapsed().as_secs_f64());

        let cpu_mem_create_time = Instant::now();
        let sys = System::new_with_specifics(
            RefreshKind::default()
                .with_memory(MemoryRefreshKind::default().with_ram())
                .with_cpu(CpuRefreshKind::default().with_frequency()),
        );
        println!(
            "cpu_mem_create_time: {} s",
            cpu_mem_create_time.elapsed().as_secs_f64()
        );

        let cpu_mem_fetch_time = Instant::now();
        let cpu = CpuEnv::create(&sys);
        let memory = MemoryEnv::create(&sys);
        println!(
            "cpu_mem_fetch_time: {} s",
            cpu_mem_fetch_time.elapsed().as_secs_f64()
        );

        let net_time = Instant::now();
        let networks = NetworkEnv::create_map();
        println!("net_time: {} s", net_time.elapsed().as_secs_f64());

        let wgpu_time = Instant::now();
        let wgpu_adapters = GpuEnv::create_list();
        println!("wgpu_time: {} s", wgpu_time.elapsed().as_secs_f64());

        println!("total_time: {} s", total_time.elapsed().as_secs_f64());

        Self {
            proc_id,
            proc_name,
            host_name,
            system,
            cpu,
            memory,
            networks,
            wgpu_adapters,
        }
    }

    pub async fn create_async() -> Result<Self, JoinError> {
        spawn_blocking(ProcEnv::create).await
    }
}

#[tokio::main]
async fn main() {
    let proc_env = ProcEnv::create_async().await.unwrap();
    let json = serde_json::to_string_pretty(&proc_env).unwrap();

    assert_eq!(
        json,
        serde_json::to_string_pretty(&serde_json::to_value(&proc_env).unwrap()).unwrap()
    );
    assert_eq!(proc_env, serde_json::from_str(&json).unwrap());

    println!("{}", json);
}
