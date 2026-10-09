//! Hardware discovery for optional indexing acceleration (T212). Call on a worker.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedicatedGpu {
    /// DXGI enumeration index, also used by ONNX Runtime's DirectML provider.
    pub adapter: u32,
    pub name: String,
    pub memory_mib: u64,
    pub identity: String,
}

/// Discrete D3D12-capable hardware only; unknown architectures are excluded.
///
/// # Errors
/// DXGI discovery is unavailable.
pub fn dedicated_gpus() -> Result<Vec<DedicatedGpu>, String> {
    imp::dedicated_gpus()
}

/// Current-process local video-memory usage on this adapter, if measurable.
#[must_use]
pub fn memory_mib(adapter: u32) -> Option<f64> {
    imp::memory_mib(adapter)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D10::ID3D10Device;
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_FEATURE_ARCHITECTURE, D3D12_FEATURE_DATA_ARCHITECTURE, D3D12CreateDevice,
        ID3D12Device,
    };
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND,
        DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter1,
        IDXGIAdapter3, IDXGIFactory1,
    };
    use windows_core::Interface;

    use super::DedicatedGpu;

    pub(super) fn dedicated_gpus() -> Result<Vec<DedicatedGpu>, String> {
        // SAFETY: COM result is owned by the returned interface; DXGI needs no COM apartment.
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for index in 0..32 {
            // SAFETY: bounded adapter index, interface owns its reference.
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(a) => a,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(e.to_string()),
            };
            if let Some(gpu) = describe(&adapter, index) {
                out.push(gpu);
            }
        }
        out.sort_by_key(|g| std::cmp::Reverse(g.memory_mib));
        Ok(out)
    }

    fn describe(adapter: &IDXGIAdapter1, index: u32) -> Option<DedicatedGpu> {
        // SAFETY: owned live adapter; output struct returned by value.
        let desc = unsafe { adapter.GetDesc1() }.ok()?;
        #[allow(clippy::cast_sign_loss)]
        if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            return None;
        }
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: valid adapter and out-pointer to owned optional COM interface.
        unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &raw mut device) }.ok()?;
        let device = device?;
        let mut architecture = D3D12_FEATURE_DATA_ARCHITECTURE::default();
        // SAFETY: correctly sized, live architecture output struct; node zero is valid.
        unsafe {
            device.CheckFeatureSupport(
                D3D12_FEATURE_ARCHITECTURE,
                (&raw mut architecture).cast(),
                u32::try_from(size_of_val(&architecture)).ok()?,
            )
        }
        .ok()?;
        if architecture.UMA.as_bool() || desc.DedicatedVideoMemory == 0 {
            return None;
        }
        let name_len = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
        // SAFETY: valid adapter and stable documented interface IID; failure stays unknown.
        let driver = unsafe { adapter.CheckInterfaceSupport(&ID3D10Device::IID) }.ok();
        Some(DedicatedGpu {
            adapter: index,
            name: String::from_utf16_lossy(&desc.Description[..name_len]),
            memory_mib: u64::try_from(desc.DedicatedVideoMemory / (1024 * 1024)).ok()?,
            identity: format!(
                "{:x}:{:x}:{:x}:{:x}:{:x}:{driver:?}",
                desc.VendorId,
                desc.DeviceId,
                desc.SubSysId,
                desc.AdapterLuid.HighPart,
                desc.AdapterLuid.LowPart,
            ),
        })
    }

    #[allow(clippy::cast_precision_loss)]
    pub(super) fn memory_mib(index: u32) -> Option<f64> {
        // SAFETY: interfaces own references; enumeration index is supplied by discovery.
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
        let adapter: IDXGIAdapter3 = unsafe { factory.EnumAdapters1(index) }.ok()?.cast().ok()?;
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        // SAFETY: valid output pointer, local memory group, valid node zero.
        unsafe { adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &raw mut info) }
            .ok()?;
        Some(info.CurrentUsage as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(not(windows))]
mod imp {
    pub(super) fn dedicated_gpus() -> Result<Vec<super::DedicatedGpu>, String> {
        Ok(Vec::new())
    }
    pub(super) fn memory_mib(_: u32) -> Option<f64> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn discovery_only_returns_named_discrete_adapters() {
        for gpu in super::dedicated_gpus().unwrap_or_default() {
            assert!(!gpu.name.is_empty());
            assert!(gpu.memory_mib > 0);
            assert!(!gpu.identity.is_empty());
        }
    }
}
