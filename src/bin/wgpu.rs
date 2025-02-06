use wgpu::{
    BackendOptions, Backends, Dx12BackendOptions, Dx12Compiler, Instance, InstanceDescriptor,
    InstanceFlags,
};

fn main() {
    let adapters = Instance::new(&InstanceDescriptor {
        backends: Backends::all(),
        flags: InstanceFlags::from_build_config(),
        backend_options: BackendOptions {
            gl: Default::default(),
            dx12: Dx12BackendOptions {
                shader_compiler: Dx12Compiler::StaticDxc,
            },
        },
    })
    .enumerate_adapters(Backends::all());

    println!("Gpu:");
    println!("wgpu adapter count: {}", adapters.len());

    for (i, adapter) in adapters.iter().enumerate() {
        let info = adapter.get_info();
        println!("");
        println!("Wgpu adapter {}:", i + 1);
        println!("adapter name: {}", info.name);
        println!("vendor id: {}", info.vendor);
        println!("device id: {}", info.device);
        println!("device type: {:?}", info.device_type);
        println!("driver: {}", info.driver);
        println!("driver info: {}", info.driver_info);
        println!("backend: {}", info.backend);
    }
}
