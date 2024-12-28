use rust_decimal::prelude::*;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, Networks, RefreshKind, System};

trait ToGbRounded {
    fn to_gb_rounded(self) -> Decimal;
}

impl ToGbRounded for u64 {
    fn to_gb_rounded(self) -> Decimal {
        Decimal::from_f64(self as f64 / 1024.0 / 1024.0 / 1024.0)
            .unwrap()
            .round_dp(3)
    }
}

fn main() {
    let sys = System::new_with_specifics(
        RefreshKind::default()
            .with_memory(MemoryRefreshKind::default().with_ram())
            .with_cpu(CpuRefreshKind::default().with_frequency()),
    );

    println!("System:");
    println!("name: {:?}", System::name());
    println!("kernel version: {:?}", System::kernel_version());
    println!("os version: {:?}", System::os_version());
    println!("long os version: {:?}", System::long_os_version());
    println!("distribution id: {}", System::distribution_id());
    println!("host name: {:?}", System::host_name());

    let cpus = sys.cpus();
    let cpu0 = &cpus[0];
    println!("");
    println!("Cpu:");
    println!("physical core count: {:?}", sys.physical_core_count());
    println!("logical core count: {}", cpus.len());
    println!("arch: {}", System::cpu_arch());
    println!("frequency: {} MHz", cpu0.frequency());
    println!("vendor id: {}", cpu0.vendor_id());
    println!("brand: {}", cpu0.brand());

    println!("");
    println!("Memory:");
    println!("total memory: {} GB", sys.total_memory().to_gb_rounded());
    println!("used memory: {} GB", sys.used_memory().to_gb_rounded());

    let networks = Networks::new_with_refreshed_list();
    println!("");
    println!("Networks:");
    println!("interface count: {}", networks.len());

    for (i, (k, v)) in networks.iter().enumerate() {
        println!("");
        println!("Network interface {}:", i + 1);
        println!("name: {}", k);
        println!("MAC address: {}", v.mac_address());
        println!("MTU: {}", v.mtu());

        for (i, ip) in v.ip_networks().iter().enumerate() {
            println!("Ip Network {}: {}", i + 1, ip);
        }
    }
}
