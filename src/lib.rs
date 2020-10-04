//! Capture the screen with DXGI Desktop Duplication

#![cfg(windows)]

extern crate winapi;
extern crate wio;

use std::mem::zeroed;
use std::{mem, ptr, slice};
use winapi::shared::dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGISurface1,
    IID_IDXGIFactory1, DXGI_MAP_READ, DXGI_OUTPUT_DESC, DXGI_RESOURCE_PRIORITY_MAXIMUM,
};
use winapi::shared::dxgi1_2::{IDXGIOutput1, IDXGIOutputDuplication};
use winapi::shared::dxgitype::*;
// use winapi::shared::ntdef::*;
use winapi::shared::windef::*;
use winapi::shared::winerror::*;
use winapi::um::d3d11::*;
use winapi::um::d3dcommon::*;
use winapi::um::unknwnbase::*;
use winapi::um::winuser::*;
use wio::com::ComPtr;

/// Color represented by additive channels: Blue (b), Green (g), Red (r), and Alpha (a).
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, Ord)]
pub struct BGRA8 {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

/// Possible errors when capturing
#[derive(Debug)]
pub enum CaptureError {
    /// Could not duplicate output, access denied. Might be in protected fullscreen.
    AccessDenied,
    /// Access to the duplicated output was lost. Likely, mode was changed e.g. window => full
    AccessLost,
    /// Error when trying to refresh outputs after some failure.
    RefreshFailure,
    /// AcquireNextFrame timed out.
    Timeout,
    /// General/Unexpected failure
    Fail(&'static str),
}

/// Check whether the HRESULT represents a failure
pub fn hr_failed(hr: HRESULT) -> bool {
    hr < 0
}

fn create_dxgi_factory_1() -> ComPtr<IDXGIFactory1> {
    unsafe {
        let mut factory = ptr::null_mut();
        let hr = CreateDXGIFactory1(&IID_IDXGIFactory1, &mut factory);
        if hr_failed(hr) {
            panic!("Failed to create DXGIFactory1, {:x}", hr)
        } else {
            ComPtr::from_raw(factory as *mut IDXGIFactory1)
        }
    }
}

fn d3d11_create_device(
    adapter: *mut IDXGIAdapter,
) -> (ComPtr<ID3D11Device>, ComPtr<ID3D11DeviceContext>) {
    unsafe {
        let (mut d3d11_device, mut device_context) = (ptr::null_mut(), ptr::null_mut());
        let hr = D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            0,
            D3D11_SDK_VERSION,
            &mut d3d11_device,
            &mut D3D_FEATURE_LEVEL_9_1,
            &mut device_context,
        );
        if hr_failed(hr) {
            panic!("Failed to create d3d11 device and device context, {:x}", hr)
        } else {
            (
                ComPtr::from_raw(d3d11_device as *mut ID3D11Device),
                ComPtr::from_raw(device_context),
            )
        }
    }
}

fn get_adapter_outputs(adapter: &IDXGIAdapter1) -> Vec<ComPtr<IDXGIOutput>> {
    let mut outputs = Vec::new();
    for i in 0.. {
        unsafe {
            let mut output = ptr::null_mut();
            if hr_failed(adapter.EnumOutputs(i, &mut output)) {
                break;
            } else {
                let mut out_desc = zeroed();
                (*output).GetDesc(&mut out_desc);
                if out_desc.AttachedToDesktop != 0 {
                    outputs.push(ComPtr::from_raw(output))
                } else {
                    break;
                }
            }
        }
    }
    outputs
}

fn output_is_primary(output: &ComPtr<IDXGIOutput1>) -> bool {
    unsafe {
        let mut output_desc = zeroed();
        output.GetDesc(&mut output_desc);
        let mut monitor_info: MONITORINFO = zeroed();
        monitor_info.cbSize = mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(output_desc.Monitor, &mut monitor_info);
        (monitor_info.dwFlags & 1) != 0
    }
}

fn get_capture_source(
    output_dups: Vec<(ComPtr<IDXGIOutputDuplication>, ComPtr<IDXGIOutput1>)>,
    cs_index: usize,
) -> Option<(ComPtr<IDXGIOutputDuplication>, ComPtr<IDXGIOutput1>)> {
    if cs_index == 0 {
        output_dups
            .into_iter()
            .find(|&(_, ref out)| output_is_primary(out))
    } else {
        output_dups
            .into_iter()
            .filter(|&(_, ref out)| !output_is_primary(out))
            .nth(cs_index - 1)
    }
}

fn duplicate_outputs(
    mut device: ComPtr<ID3D11Device>,
    outputs: Vec<ComPtr<IDXGIOutput>>,
) -> Result<
    (
        ComPtr<ID3D11Device>,
        Vec<(ComPtr<IDXGIOutputDuplication>, ComPtr<IDXGIOutput1>)>,
    ),
    HRESULT,
> {
    let mut out_dups = Vec::new();
    for output in outputs
        .into_iter()
        .map(|out| out.cast::<IDXGIOutput1>().unwrap())
    {
        let dxgi_device = device.up::<IUnknown>();
        let output_duplication = unsafe {
            let mut output_duplication = ptr::null_mut();
            let hr = output.DuplicateOutput(dxgi_device.as_raw(), &mut output_duplication);
            if hr_failed(hr) {
                return Err(hr);
            }
            ComPtr::from_raw(output_duplication)
        };
        device = dxgi_device.cast().unwrap();
        out_dups.push((output_duplication, output));
    }
    Ok((device, out_dups))
}

struct DuplicatedOutput {
    device: ComPtr<ID3D11Device>,
    device_context: ComPtr<ID3D11DeviceContext>,
    output: ComPtr<IDXGIOutput1>,
    output_duplication: ComPtr<IDXGIOutputDuplication>,
}
impl DuplicatedOutput {
    fn get_desc(&self) -> DXGI_OUTPUT_DESC {
        unsafe {
            let mut desc = zeroed();
            self.output.GetDesc(&mut desc);
            desc
        }
    }

    fn capture_frame_to_surface(
        &mut self,
        timeout_ms: u32,
    ) -> Result<ComPtr<IDXGISurface1>, HRESULT> {
        let frame_resource = unsafe {
            let mut frame_resource = ptr::null_mut();
            let mut frame_info = zeroed();
            let hr = self.output_duplication.AcquireNextFrame(
                timeout_ms,
                &mut frame_info,
                &mut frame_resource,
            );
            if hr_failed(hr) {
                return Err(hr);
            }
            ComPtr::from_raw(frame_resource)
        };
        let frame_texture = frame_resource.cast::<ID3D11Texture2D>().unwrap();
        let mut texture_desc = unsafe {
            let mut texture_desc = zeroed();
            frame_texture.GetDesc(&mut texture_desc);
            texture_desc
        };
        // Configure the description to make the texture readable
        texture_desc.Usage = D3D11_USAGE_STAGING;
        texture_desc.BindFlags = 0;
        texture_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        texture_desc.MiscFlags = 0;
        let readable_texture = unsafe {
            let mut readable_texture = ptr::null_mut();
            let hr =
                self.device
                    .CreateTexture2D(&mut texture_desc, ptr::null(), &mut readable_texture);
            if hr_failed(hr) {
                return Err(hr);
            }
            ComPtr::from_raw(readable_texture)
        };
        // Lower priorities causes stuff to be needlessly copied from gpu to ram,
        // causing huge ram usage on some systems.
        unsafe { readable_texture.SetEvictionPriority(DXGI_RESOURCE_PRIORITY_MAXIMUM) };
        let readable_surface = readable_texture.up::<ID3D11Resource>();
        unsafe {
            self.device_context.CopyResource(
                readable_surface.as_raw(),
                frame_texture.up::<ID3D11Resource>().as_raw(),
            );
            self.output_duplication.ReleaseFrame();
        }
        readable_surface.cast()
    }
}

/// Manager of DXGI duplicated outputs
pub struct DXGIManager {
    duplicated_output: Option<DuplicatedOutput>,
    capture_source_index: usize,
    timeout_ms: u32,
}

struct SharedPtr<T>(*const T);

unsafe impl<T> Send for SharedPtr<T> {}

unsafe impl<T> Sync for SharedPtr<T> {}

impl DXGIManager {
    /// Construct a new manager with capture timeout
    pub fn new(timeout_ms: u32) -> Result<DXGIManager, &'static str> {
        let mut manager = DXGIManager {
            duplicated_output: None,
            capture_source_index: 0,
            timeout_ms: timeout_ms,
        };

        match manager.acquire_output_duplication() {
            Ok(_) => Ok(manager),
            Err(_) => Err("Failed to acquire output duplication"),
        }
    }

    /// Set index of capture source to capture from
    pub fn set_capture_source_index(&mut self, cs: usize) {
        self.capture_source_index = cs;
        self.acquire_output_duplication().unwrap()
    }

    pub fn get_capture_source_index(&self) -> usize {
        self.capture_source_index
    }

    /// Set timeout to use when capturing
    pub fn set_timeout_ms(&mut self, timeout_ms: u32) {
        self.timeout_ms = timeout_ms
    }

    /// Duplicate and acquire output selected by `capture_source_index`
    pub fn acquire_output_duplication(&mut self) -> Result<(), ()> {
        self.duplicated_output = None;
        let factory = create_dxgi_factory_1();
        for (outputs, adapter) in (0..)
            .map(|i| {
                let mut adapter = ptr::null_mut();
                unsafe {
                    if factory.EnumAdapters1(i, &mut adapter) != DXGI_ERROR_NOT_FOUND {
                        Some(ComPtr::from_raw(adapter))
                    } else {
                        None
                    }
                }
            })
            .take_while(Option::is_some)
            .map(Option::unwrap)
            .map(|mut adapter| (get_adapter_outputs(&mut adapter), adapter))
            .filter(|&(ref outs, _)| !outs.is_empty())
        {
            // Creating device for each adapter that has the output
            let (d3d11_device, device_context) = d3d11_create_device(adapter.up().as_raw());
            let (d3d11_device, output_duplications) =
                duplicate_outputs(d3d11_device, outputs).map_err(|_| ())?;
            if let Some((output_duplication, output)) =
                get_capture_source(output_duplications, self.capture_source_index)
            {
                self.duplicated_output = Some(DuplicatedOutput {
                    device: d3d11_device,
                    device_context: device_context,
                    output: output,
                    output_duplication: output_duplication,
                });
                return Ok(());
            }
        }
        Err(())
    }

    fn capture_frame_to_surface(&mut self) -> Result<ComPtr<IDXGISurface1>, CaptureError> {
        if let None = self.duplicated_output {
            if let Ok(_) = self.acquire_output_duplication() {
                return Err(CaptureError::Fail("No valid duplicated output"));
            } else {
                return Err(CaptureError::RefreshFailure);
            }
        }
        let timeout_ms = self.timeout_ms;
        match self
            .duplicated_output
            .as_mut()
            .unwrap()
            .capture_frame_to_surface(timeout_ms)
        {
            Ok(surface) => Ok(surface),
            Err(DXGI_ERROR_ACCESS_LOST) => {
                if let Ok(_) = self.acquire_output_duplication() {
                    Err(CaptureError::AccessLost)
                } else {
                    Err(CaptureError::RefreshFailure)
                }
            }
            Err(E_ACCESSDENIED) => Err(CaptureError::AccessDenied),
            Err(DXGI_ERROR_WAIT_TIMEOUT) => Err(CaptureError::Timeout),
            Err(_) => {
                if let Ok(_) = self.acquire_output_duplication() {
                    Err(CaptureError::Fail("Failure when acquiring frame"))
                } else {
                    Err(CaptureError::RefreshFailure)
                }
            }
        }
    }

    fn capture_frame_t<T: Copy + Send + Sync + Sized>(&mut self) -> Result<(Vec<T>, (usize, usize)), CaptureError> {
        let frame_surface = match self.capture_frame_to_surface() {
            Ok(surface) => surface,
            Err(e) => return Err(e),
        };
        let mapped_surface = unsafe {
            let mut mapped_surface = zeroed();
            if hr_failed(frame_surface.Map(&mut mapped_surface, DXGI_MAP_READ)) {
                frame_surface.Release();
                return Err(CaptureError::Fail("Failed to map surface"));
            }
            mapped_surface
        };
        let byte_size = |x| x * mem::size_of::<BGRA8>() / mem::size_of::<T>();
        let output_desc = self.duplicated_output.as_mut().unwrap().get_desc();
        let stride = mapped_surface.Pitch as usize / mem::size_of::<BGRA8>();
        let byte_stride = byte_size(stride);
        let (mut output_width, mut output_height) = {
            let RECT {
                left,
                top,
                right,
                bottom,
            } = output_desc.DesktopCoordinates;
            ((right - left) as usize, (bottom - top) as usize)
        };
        let mut pixel_buf = Vec::with_capacity(byte_size(output_width * output_height));
        
        match output_desc.Rotation {
            DXGI_MODE_ROTATION_ROTATE90 | DXGI_MODE_ROTATION_ROTATE270 => {
                mem::swap(&mut output_width, &mut output_height);
            }
            _ => {}
        };
        // let pixel_index: Box<dyn Fn(usize, usize) -> usize> = match output_desc.Rotation {
        //     DXGI_MODE_ROTATION_IDENTITY | DXGI_MODE_ROTATION_UNSPECIFIED => {
        //         Box::new(|row, col| row * map_pitch_n_pixels + col)
        //     }
        //     DXGI_MODE_ROTATION_ROTATE90 => {
        //         Box::new(|row, col| (output_width - 1 - col) * map_pitch_n_pixels + row)
        //     }
        //     DXGI_MODE_ROTATION_ROTATE180 => Box::new(|row, col| {
        //         (output_height - 1 - row) * map_pitch_n_pixels + (output_width - col - 1)
        //     }),
        //     DXGI_MODE_ROTATION_ROTATE270 => {
        //         Box::new(|row, col| col * map_pitch_n_pixels + (output_height - row - 1))
        //     }
        //     n => unreachable!("Undefined DXGI_MODE_ROTATION: {}", n),
        // };
        let mapped_pixels = unsafe {
            slice::from_raw_parts(
                mapped_surface.pBits as *const T,
                byte_stride * output_height,
            )
        };
        // for row in 0..output_height {
        //     for col in 0..output_width {
        //         pixel_buf.push(mapped_pixels[row * map_pitch_n_pixels + col]);
        //     }
        // }
        let now = Instant::now();
        match output_desc.Rotation {
            DXGI_MODE_ROTATION_IDENTITY | DXGI_MODE_ROTATION_UNSPECIFIED =>
                pixel_buf.extend_from_slice(mapped_pixels),
            DXGI_MODE_ROTATION_ROTATE90 => {
                unsafe {
                    let mut buf = Vec::new();
                    mem::swap(&mut pixel_buf, &mut buf);
                    let len = buf.capacity();
                    let ptr = SharedPtr(buf.as_ptr() as *const BGRA8);
                    mapped_pixels.chunks(byte_stride).rev().enumerate().for_each(|(column, chunk)| {
                        let mut src = chunk.as_ptr() as *const BGRA8;
                        let mut dst = ptr.0 as *mut BGRA8;
                        dst = dst.add(column);
                        let stop = src.add(output_height);
                        while src != stop {
                            dst.write(*src);
                            src = src.add(1);
                            dst = dst.add(output_width);
                        }
                    });
                    pixel_buf = Vec::from_raw_parts(buf.as_mut_ptr(), len, len);
                    mem::forget(buf);
                }
            }
            DXGI_MODE_ROTATION_ROTATE180 => {
                unsafe {
                    let mut buf = Vec::new();
                    mem::swap(&mut pixel_buf, &mut buf);
                    let len = buf.capacity();
                    let ptr = SharedPtr(buf.as_ptr() as *const BGRA8);
                    mapped_pixels.chunks(byte_stride).rev().enumerate().for_each(|(scan_line, chunk)| {
                        let mut src = chunk.as_ptr() as *const BGRA8;
                        let mut dst = ptr.0 as *mut BGRA8;
                        dst = dst.add(scan_line * output_width);
                        let stop = src;
                        src = src.add(output_width);
                        while src != stop {
                            src = src.sub(1);
                            dst.write(*src);
                            dst = dst.add(1);
                        }
                    });
                    pixel_buf = Vec::from_raw_parts(buf.as_mut_ptr(), len, len);
                    mem::forget(buf);
                }
            }
            DXGI_MODE_ROTATION_ROTATE270 => {
                unsafe {
                    let mut buf = Vec::new();
                    mem::swap(&mut pixel_buf, &mut buf);
                    let len = buf.capacity();
                    let ptr = SharedPtr(buf.as_ptr() as *const BGRA8);
                    mapped_pixels.chunks(byte_stride).enumerate().for_each(|(column, chunk)| {
                        let mut src = chunk.as_ptr() as *const BGRA8;
                        let mut dst = ptr.0 as *mut BGRA8;
                        dst = dst.add(column);
                        let stop = src;
                        src = src.add(output_height);
                        while src != stop {
                            src = src.sub(1);
                            dst.write(*src);
                            dst = dst.add(output_width);
                        }
                    });
                    pixel_buf = Vec::from_raw_parts(buf.as_mut_ptr(), len, len);
                    mem::forget(buf);
                }
            }
            _ => unimplemented!(),
        }
        dbg!(Instant::now() - now);
        unsafe { frame_surface.Unmap() };
        Ok((pixel_buf, (output_width, output_height)))
    }

    /// Capture a frame
    ///
    /// On success, return Vec with pixels and width and height of frame.
    /// On failure, return CaptureError.
    pub fn capture_frame(&mut self) -> Result<(Vec<BGRA8>, (usize, usize)), CaptureError> {
        self.capture_frame_t()
    }

    /// Capture a frame
    ///
    /// On success, return Vec with pixel components and width and height of frame.
    /// On failure, return CaptureError.
    pub fn capture_frame_components(&mut self) -> Result<(Vec<u8>, (usize, usize)), CaptureError> {
        self.capture_frame_t()
    }
}

use std::time::Instant;
#[test]
fn test() {
    let mut manager = DXGIManager::new(300).unwrap();
    for _ in 0..100 {
        match manager.capture_frame() {
            Ok((pixels, (_, _))) => {
                let len = pixels.len() as u64;
                let (r, g, b) = pixels.into_iter().fold((0u64, 0u64, 0u64), |(r, g, b), p| {
                    (r + p.r as u64, g + p.g as u64, b + p.b as u64)
                });
                println!("avg: {} {} {}", r / len, g / len, b / len)
            }
            Err(e) => println!("error: {:?}", e),
        }
    }
}

#[test]
fn compare_frame_dims() {
    let mut manager = DXGIManager::new(300).unwrap();
    let (frame, (fw, fh)) = manager.capture_frame().unwrap();
    let (frame_u8, (fu8w, fu8h)) = manager.capture_frame_components().unwrap();
    assert_eq!(fw, fu8w);
    assert_eq!(fh, fu8h);
    assert_eq!(4 * frame.len(), frame_u8.len());
}