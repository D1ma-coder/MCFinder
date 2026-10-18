//! OpenCL 1.2 GPU placement filter; no runtime is loaded for CPU searches.
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};
type Handle = *mut c_void;
type Status = i32;
type R<T> = std::result::Result<T, String>;
macro_rules! api {
    ($($name:ident($($arg:ty),*) -> $ret:ty;)*) => {
        #[allow(non_snake_case)]
        struct Api { $($name: unsafe extern "system" fn($($arg),*) -> $ret,)* _library: Library }
        impl Api {
            fn load() -> R<Self> {
                #[cfg(target_os = "macos")]
                let path = "/System/Library/Frameworks/OpenCL.framework/OpenCL";
                #[cfg(target_os = "linux")]
                let path = "libOpenCL.so.1";
                #[cfg(target_os = "windows")]
                let path = "OpenCL.dll";
                #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
                let path = "libOpenCL.so";
                // SAFETY: loading the system OpenCL loader; symbols use the ABI
                // and signatures specified by OpenCL 1.2. Library outlives them.
                unsafe {
                    let library = Library::new(path).map_err(|e| format!("GPU runtime unavailable: {e}"))?;
                    Ok(Self { $($name: *library.get(concat!(stringify!($name), "\0").as_bytes()).map_err(|e| e.to_string())?,)* _library: library })
                }
            }
        }
    };
}
api! {
    clGetPlatformIDs(u32, *mut Handle, *mut u32) -> Status;
    clGetDeviceIDs(Handle, u64, u32, *mut Handle, *mut u32) -> Status;
    clGetDeviceInfo(Handle, u32, usize, *mut c_void, *mut usize) -> Status;
    clCreateContext(*const isize, u32, *const Handle, Handle, Handle, *mut Status) -> Handle;
    clCreateCommandQueue(Handle, Handle, u64, *mut Status) -> Handle;
    clCreateProgramWithSource(Handle, u32, *const *const c_char, *const usize, *mut Status) -> Handle;
    clBuildProgram(Handle, u32, *const Handle, *const c_char, Handle, Handle) -> Status;
    clGetProgramBuildInfo(Handle, Handle, u32, usize, *mut c_void, *mut usize) -> Status;
    clCreateKernel(Handle, *const c_char, *mut Status) -> Handle;
    clCreateBuffer(Handle, u64, usize, Handle, *mut Status) -> Handle;
    clSetKernelArg(Handle, u32, usize, *const c_void) -> Status;
    clEnqueueWriteBuffer(Handle, Handle, u32, usize, usize, *const c_void, u32, *const Handle, *mut Handle) -> Status;
    clEnqueueReadBuffer(Handle, Handle, u32, usize, usize, *mut c_void, u32, *const Handle, *mut Handle) -> Status;
    clEnqueueNDRangeKernel(Handle, Handle, u32, *const usize, *const usize, *const usize, u32, *const Handle, *mut Handle) -> Status;
    clGetKernelWorkGroupInfo(Handle, Handle, u32, usize, *mut c_void, *mut usize) -> Status;
    clReleaseMemObject(Handle) -> Status;
    clReleaseKernel(Handle) -> Status;
    clReleaseProgram(Handle) -> Status;
    clReleaseCommandQueue(Handle) -> Status;
    clReleaseContext(Handle) -> Status;
}
fn check(s: Status, action: &str) -> R<()> {
    if s == 0 {
        Ok(())
    } else {
        Err(format!("OpenCL {action} failed ({s})"))
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Profile {
    Auto,
    Apple,
    Nvidia,
    Generic,
}
impl Profile {
    fn detect(vendor: &str) -> Self {
        let vendor = vendor.to_ascii_lowercase();
        if vendor.contains("apple") {
            Self::Apple
        } else if vendor.contains("nvidia") {
            Self::Nvidia
        } else {
            Self::Generic
        }
    }
}
#[derive(Clone)]
struct Device {
    id: Handle,
    name: String,
    vendor: String,
}
impl Api {
    fn device_string(&self, device: Handle, param: u32) -> R<String> {
        let mut n = 0;
        unsafe {
            check(
                (self.clGetDeviceInfo)(device, param, 0, ptr::null_mut(), &mut n),
                "device info size",
            )?;
            let mut data = vec![0_u8; n.max(1)];
            check(
                (self.clGetDeviceInfo)(
                    device,
                    param,
                    data.len(),
                    data.as_mut_ptr().cast(),
                    ptr::null_mut(),
                ),
                "device info",
            )?;
            Ok(CStr::from_bytes_until_nul(&data)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned())
        }
    }
    fn devices(&self) -> R<Vec<Device>> {
        unsafe {
            let mut n = 0;
            check(
                (self.clGetPlatformIDs)(0, ptr::null_mut(), &mut n),
                "platform enumeration",
            )?;
            if n == 0 {
                return Ok(Vec::new());
            }
            let mut platforms = vec![ptr::null_mut(); n as usize];
            check(
                (self.clGetPlatformIDs)(n, platforms.as_mut_ptr(), ptr::null_mut()),
                "platform enumeration",
            )?;
            let mut result = Vec::new();
            for platform in platforms {
                n = 0;
                let status = (self.clGetDeviceIDs)(
                    platform,
                    4, /* CL_DEVICE_TYPE_GPU */
                    0,
                    ptr::null_mut(),
                    &mut n,
                );
                if status == -1 || n == 0 {
                    continue;
                }
                check(status, "GPU enumeration")?;
                let mut devices = vec![ptr::null_mut(); n as usize];
                check(
                    (self.clGetDeviceIDs)(platform, 4, n, devices.as_mut_ptr(), ptr::null_mut()),
                    "GPU enumeration",
                )?;
                for id in devices {
                    result.push(Device {
                        id,
                        name: self.device_string(id, 0x102b)?,
                        vendor: self.device_string(id, 0x102c)?,
                    });
                }
            }
            Ok(result)
        }
    }
}
pub fn list() -> R<Vec<String>> {
    Ok(Api::load()?
        .devices()?
        .into_iter()
        .enumerate()
        .map(|(i, d)| {
            format!(
                "gpu:{i} {} ({}) | automatic profile={:?}",
                d.name,
                d.vendor,
                Profile::detect(&d.vendor)
            )
        })
        .collect())
}
pub struct Gpu {
    api: Api,
    context: Handle,
    queue: Handle,
    program: Handle,
    kernel: Handle,
    rules: Handle,
    seeds: Handle,
    passed: Handle,
    capacity: usize,
    rule_count: i32,
    workgroup: usize,
    pub description: String,
}
impl Gpu {
    pub fn new(index: usize, profile: Profile, rules: &[i32]) -> R<Self> {
        let api = Api::load()?;
        let devices = api.devices()?;
        let d = devices
            .get(index)
            .ok_or_else(|| format!("GPU index {index} not found; run `mcfinder devices`"))?;
        let profile = match profile {
            Profile::Auto => Profile::detect(&d.vendor),
            p => p,
        };
        let device = d.id;
        let description = format!("{} | profile={profile:?}", d.name);
        let mut result = Self {
            api,
            context: ptr::null_mut(),
            queue: ptr::null_mut(),
            program: ptr::null_mut(),
            kernel: ptr::null_mut(),
            rules: ptr::null_mut(),
            seeds: ptr::null_mut(),
            passed: ptr::null_mut(),
            capacity: 0,
            rule_count: (rules.len() / 12) as i32,
            workgroup: 1,
            description,
        };
        unsafe {
            let mut status = 0;
            result.context = (result.api.clCreateContext)(
                ptr::null(),
                1,
                &device,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut status,
            );
            check(status, "context creation")?;
            result.queue =
                (result.api.clCreateCommandQueue)(result.context, device, 0, &mut status);
            check(status, "queue creation")?;
            let source = CString::new(include_str!("filter.cl")).unwrap();
            result.program = (result.api.clCreateProgramWithSource)(
                result.context,
                1,
                &source.as_ptr(),
                ptr::null(),
                &mut status,
            );
            check(status, "program creation")?;
            let opts = CString::new(format!(
                "-cl-std=CL1.2 -D MCF_NVIDIA={}",
                i32::from(matches!(profile, Profile::Nvidia))
            ))
            .unwrap();
            status = (result.api.clBuildProgram)(
                result.program,
                1,
                &device,
                opts.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            if status != 0 {
                let mut n = 0;
                (result.api.clGetProgramBuildInfo)(
                    result.program,
                    device,
                    0x1183,
                    0,
                    ptr::null_mut(),
                    &mut n,
                );
                let mut log = vec![0_u8; n.max(1)];
                (result.api.clGetProgramBuildInfo)(
                    result.program,
                    device,
                    0x1183,
                    log.len(),
                    log.as_mut_ptr().cast(),
                    ptr::null_mut(),
                );
                return Err(format!(
                    "OpenCL build failed ({status}): {}",
                    String::from_utf8_lossy(&log)
                ));
            }
            result.kernel =
                (result.api.clCreateKernel)(result.program, c"filter".as_ptr(), &mut status);
            check(status, "kernel creation")?;
            let mut max_group = 1_usize;
            check(
                (result.api.clGetKernelWorkGroupInfo)(
                    result.kernel,
                    device,
                    0x11b0,
                    size_of::<usize>(),
                    (&mut max_group as *mut usize).cast(),
                    ptr::null_mut(),
                ),
                "workgroup info",
            )?;
            result.workgroup = max_group
                .min(if matches!(profile, Profile::Nvidia) {
                    128
                } else {
                    64
                })
                .max(1);
            result.rules = result.buffer(size_of_val(rules), 4 /* read only */)?;
            result.write(result.rules, rules)?;
        }
        Ok(result)
    }
    fn buffer(&self, bytes: usize, flags: u64) -> R<Handle> {
        let mut status = 0;
        let b = unsafe {
            (self.api.clCreateBuffer)(self.context, flags, bytes, ptr::null_mut(), &mut status)
        };
        check(status, "buffer allocation")?;
        Ok(b)
    }
    fn write<T>(&self, buffer: Handle, data: &[T]) -> R<()> {
        unsafe {
            check(
                (self.api.clEnqueueWriteBuffer)(
                    self.queue,
                    buffer,
                    1,
                    0,
                    size_of_val(data),
                    data.as_ptr().cast(),
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                ),
                "buffer upload",
            )
        }
    }
    pub fn filter(&mut self, seeds: &[u64]) -> R<Vec<u8>> {
        self.filter_counted(seeds).map(|(mask, _)| mask)
    }
    pub(crate) fn filter_counted(&mut self, seeds: &[u64]) -> R<(Vec<u8>, u64)> {
        if seeds.is_empty() {
            return Ok((Vec::new(), 0));
        }
        unsafe {
            if seeds.len() > self.capacity {
                if !self.seeds.is_null() {
                    (self.api.clReleaseMemObject)(self.seeds);
                    self.seeds = ptr::null_mut();
                }
                if !self.passed.is_null() {
                    (self.api.clReleaseMemObject)(self.passed);
                    self.passed = ptr::null_mut();
                }
                self.seeds = self.buffer(size_of_val(seeds), 4)?;
                self.passed = self.buffer(size_of_val(seeds), 2 /* write only */)?;
                self.capacity = seeds.len();
            }
            self.write(self.seeds, seeds)?;
            let count = seeds.len() as i32;
            for (i, buffer) in [self.seeds, self.rules].iter().enumerate() {
                check(
                    (self.api.clSetKernelArg)(
                        self.kernel,
                        i as u32,
                        size_of::<Handle>(),
                        (buffer as *const Handle).cast(),
                    ),
                    "kernel buffer argument",
                )?;
            }
            check(
                (self.api.clSetKernelArg)(
                    self.kernel,
                    2,
                    size_of::<i32>(),
                    (&self.rule_count as *const i32).cast(),
                ),
                "rule count",
            )?;
            check(
                (self.api.clSetKernelArg)(
                    self.kernel,
                    3,
                    size_of::<i32>(),
                    (&count as *const i32).cast(),
                ),
                "seed count",
            )?;
            check(
                (self.api.clSetKernelArg)(
                    self.kernel,
                    4,
                    size_of::<Handle>(),
                    (&self.passed as *const Handle).cast(),
                ),
                "output buffer",
            )?;
            let global = seeds.len().div_ceil(self.workgroup) * self.workgroup;
            check(
                (self.api.clEnqueueNDRangeKernel)(
                    self.queue,
                    self.kernel,
                    1,
                    ptr::null(),
                    &global,
                    &self.workgroup,
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                ),
                "kernel dispatch",
            )?;
            let mut passed = vec![0_u64; seeds.len()];
            check(
                (self.api.clEnqueueReadBuffer)(
                    self.queue,
                    self.passed,
                    1,
                    0,
                    size_of_val(passed.as_slice()),
                    passed.as_mut_ptr().cast(),
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                ),
                "result download",
            )?;
            let checks = passed.iter().map(|v| v >> 1).sum();
            Ok((passed.into_iter().map(|v| (v & 1) as u8).collect(), checks))
        }
    }
}
impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            for b in [self.seeds, self.passed, self.rules] {
                if !b.is_null() {
                    (self.api.clReleaseMemObject)(b);
                }
            }
            if !self.kernel.is_null() {
                (self.api.clReleaseKernel)(self.kernel);
            }
            if !self.program.is_null() {
                (self.api.clReleaseProgram)(self.program);
            }
            if !self.queue.is_null() {
                (self.api.clReleaseCommandQueue)(self.queue);
            }
            if !self.context.is_null() {
                (self.api.clReleaseContext)(self.context);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::Structure,
        plan::{Plan, Query, Rule},
    };
    #[test]
    #[ignore = "Requires a real OpenCL GPU; run explicitly with --ignored"]
    fn gpu_matches_cpu_geometry_all_profiles() {
        let seeds: Vec<u64> = (0..257)
            .map(|i| (i * 0x9e3779b97f4a7c15_u128) as u64)
            .collect();
        for version in ["1.8.9", "1.12.2", "1.16.5", "1.18.2", "1.21.4", "26.3"] {
            let b = if version == "1.8.9" {
                Structure::Monument
            } else {
                Structure::Mansion
            };
            for rules in [
                vec![Rule::Near {
                    a: Structure::Village,
                    b,
                    distance: 384,
                }],
                vec![Rule::Near {
                    a: Structure::Shipwreck,
                    b: Structure::OceanRuin,
                    distance: 200,
                }],
                vec![Rule::SpawnNear {
                    structure: Structure::DesertPyramid,
                    distance: 24,
                }],
                vec![Rule::Near {
                    a: Structure::Village,
                    b: Structure::Village,
                    distance: 0,
                }],
            ] {
                let Ok(plan) = Plan::new(&Query {
                    version: version.into(),
                    radius: 1536,
                    rules,
                }) else {
                    continue;
                };
                let expected: Vec<u8> = seeds
                    .iter()
                    .map(|&s| u8::from(plan.geometry(s).is_some()))
                    .collect();
                for profile in [Profile::Apple, Profile::Nvidia, Profile::Generic] {
                    let mut gpu = Gpu::new(0, profile, &plan.gpu_rules()).unwrap();
                    let (mask, checks) = gpu.filter_counted(&seeds).unwrap();
                    assert!(checks >= seeds.len() as u64);
                    assert_eq!(mask, expected, "{version} {profile:?}");
                    // Reuse buffers with a smaller unaligned final batch.
                    assert_eq!(gpu.filter(&seeds[..3]).unwrap(), expected[..3]);
                }
            }
        }
    }
}
