// The MIT License (MIT)
//
// Copyright (c) 2015 Johan Johansson
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! Capture the screen with DXGI in rust

#![feature(libc, unique, unsafe_destructor, std_misc)]
#![allow(dead_code, non_snake_case)]

extern crate libc;
extern crate winapi;
#[macro_use(c_mtdcall)]
extern crate dxgi;
extern crate d3d11;

use std::sync::{Arc, Mutex};
use std::mem;
use std::ptr::{ self, Unique };
use std::time::duration::Duration;
use libc::c_void;
use winapi::{ HRESULT, IID, DWORD, RECT, HMONITOR, BOOL };
use dxgi::constants::*;
use dxgi::interfaces::*;
use dxgi::{ CreateDXGIFactory1, DXGI_OUTPUT_DESC };
use d3d11::constants::*;
use d3d11::interfaces::*;
use d3d11::{ D3D11_USAGE, D3D11_CPU_ACCESS_FLAG, D3D_DRIVER_TYPE,
	D3D_FEATURE_LEVEL, D3D11CreateDevice };

#[repr(C)] struct MONITORINFO {
	cbSize: DWORD,
	rcMonitor: RECT,
	rcWork: RECT,
	dwFlags: DWORD,
}

#[link(name = "user32")]
extern "C" {
	fn GetMonitorInfoW(monitor: HMONITOR, monitor_info: *mut MONITORINFO) -> BOOL;
}

/// A unique pointer to a COM object. Handles refcounting.
pub struct UniqueCOMPtr<T: IUnknownT> {
	ptr: Unique<T>,
}
impl<T: IUnknownT> UniqueCOMPtr<T> {
	/// Construct a new unique COM pointer from a pointer to a COM object.
	/// It is the users responsibility to guarantee that no copies of the pointer exists beforehand 
	pub unsafe fn from_ptr(ptr: *mut T) -> UniqueCOMPtr<T> {
		UniqueCOMPtr{ ptr: Unique::new(ptr) }
	}

	pub unsafe fn query_interface<U>(mut self, interface_identifier: &IID)
		-> Result<UniqueCOMPtr<U>, HRESULT> where U: IUnknownT
	{
		let mut interface: *mut c_void = ptr::null_mut();
		let hr = self.QueryInterface(interface_identifier, &mut interface);
		if hr_failed(hr) {
			Err(hr)
		} else {
			Ok(UniqueCOMPtr::from_ptr(interface as *mut U))
		}
	}
}
impl<T: IUnknownT> std::ops::Deref for UniqueCOMPtr<T> {
	type Target = T;

	fn deref(&self) -> &T {
		unsafe { self.ptr.get() }
	}
}
impl<T: IUnknownT> std::ops::DerefMut for UniqueCOMPtr<T> {
	fn deref_mut(&mut self) -> &mut T {
		unsafe { self.ptr.get_mut() }
	}
}
#[unsafe_destructor]
impl<T: IUnknownT> std::ops::Drop for UniqueCOMPtr<T> {
	fn drop(&mut self) {
		self.Release();
	}
}
/// This is not actually necessarily thread safe. It's up to the user to guarantee that all
/// pointers are uniquely owned.
unsafe impl<T> Send for UniqueCOMPtr<T> { }

pub fn hr_failed(hr: HRESULT) -> bool { hr < 0 }

fn c_utf16_to_string(chars: &[u16]) -> String {
	String::from_utf16_lossy(
		&chars.iter().cloned().take_while(|&b| b != 0).collect::<Vec<_>>()[..])
}

fn max<T: PartialOrd>(a: T, b: T) -> T { if a > b { a } else { b } }

fn create_dxgi_factory_1() -> UniqueCOMPtr<IDXGIFactory1> {
	unsafe {
		let mut factory: *mut c_void = ptr::null_mut();

		let hr = CreateDXGIFactory1(&IID_IDXGIFactory1, &mut factory);
		if hr_failed(hr) {
			panic!("Failed to create DXGIFactory1, {:x}", hr)
		} else {
			UniqueCOMPtr::from_ptr(factory as *mut IDXGIFactory1) } }
}

fn d3d11_create_device<T: IDXGIAdapterT>(adapter: &mut T)
	-> (UniqueCOMPtr<ID3D11Device>, UniqueCOMPtr<ID3D11DeviceContext>)
{
	unsafe {
		let mut d3d11_device: *mut ID3D11Device = ptr::null_mut();
		let mut device_context: *mut ID3D11DeviceContext = ptr::null_mut();

		let hr = D3D11CreateDevice(mem::transmute(adapter),
			D3D_DRIVER_TYPE::D3D_DRIVER_TYPE_UNKNOWN,
			ptr::null_mut(), 0, ptr::null_mut(), 0,
			D3D11_SDK_VERSION,
			&mut d3d11_device,
			&mut D3D_FEATURE_LEVEL::D3D_FEATURE_LEVEL_9_1,
			&mut device_context);
		if hr_failed(hr) {
			panic!("Failed to create d3d11 device and device context, {:x}", hr)
		} else {
			(UniqueCOMPtr::from_ptr(d3d11_device as *mut ID3D11Device),
				UniqueCOMPtr::from_ptr(device_context)) } }
}

pub fn get_adater_outputs(adapter: &mut IDXGIAdapter1) -> Vec<UniqueCOMPtr<IDXGIOutput>> {
	(0..).map(|i| {
			let mut output = ptr::null_mut();
			if hr_failed(adapter.EnumOutputs(i, &mut output)) {
				None
			} else {
				let mut out_desc = unsafe { mem::zeroed() };
				unsafe { (*output).GetDesc(&mut out_desc) };

				if out_desc.AttachedToDesktop != 0 {
					Some(unsafe { UniqueCOMPtr::from_ptr(output) })
				} else { None } } })
		.take_while(Option::is_some).map(Option::unwrap)
		.collect()
}

struct DuplicatedOutput {
	device: Arc<Mutex<UniqueCOMPtr<ID3D11Device>>>,
	device_context: Arc<Mutex<UniqueCOMPtr<ID3D11DeviceContext>>>,
	output: UniqueCOMPtr<IDXGIOutput1>,
	dxgi_output_dup: UniqueCOMPtr<IDXGIOutputDuplication>,
}
impl DuplicatedOutput {
	fn get_desc(&self) -> DXGI_OUTPUT_DESC {
		unsafe {
			let mut desc = mem::zeroed();
			mem::transmute::<_, &mut Self>(self).output.GetDesc(&mut desc);
			desc }
	}

	fn get_frame(&mut self, timeout: Duration) -> Result<UniqueCOMPtr<IDXGISurface1>, HRESULT> {
		let frame_resource = unsafe {
			let mut frame_resource = ptr::null_mut();
			let mut frame_info = mem::zeroed();
			let hr = self.dxgi_output_dup.AcquireNextFrame(timeout.num_milliseconds() as u32,
				&mut frame_info,
				&mut frame_resource);
			if hr_failed(hr) {
				return Err(hr);
			}
			UniqueCOMPtr::from_ptr(frame_resource) };

		let mut frame_texture: UniqueCOMPtr<ID3D11Texture2D> = unsafe {
			frame_resource.query_interface(&IID_ID3D11Texture2D).unwrap() };

		let mut texture_desc = unsafe { mem::zeroed() };
		frame_texture.GetDesc(&mut texture_desc);

		// Configure the description to make the texture readable
		texture_desc.Usage = D3D11_USAGE::D3D11_USAGE_STAGING;
		texture_desc.BindFlags = 0;
		texture_desc.CPUAccessFlags = D3D11_CPU_ACCESS_FLAG::D3D11_CPU_ACCESS_READ as u32;
		texture_desc.MiscFlags = 0;

		let mut readable_texture = unsafe {
			let mut readable_texture = ptr::null_mut();
			let hr = self.device.lock().unwrap()
				.CreateTexture2D(&mut texture_desc, ptr::null(), &mut readable_texture);
			if hr_failed(hr) {
				return Err(hr);
			}
			UniqueCOMPtr::from_ptr(readable_texture) };

		// Lower priorities causes stuff to be needlessly copied from gpu to ram, causing huge
		// fluxuations on some systems.
		readable_texture.SetEvictionPriority(DXGI_RESOURCE_PRIORITY_MAXIMUM);

		let mut readable_surface = unsafe {
			readable_texture.query_interface(&IID_ID3D11Resource).unwrap() };
		self.device_context.lock().unwrap()
			.CopyResource(&mut *readable_surface,
				&mut *unsafe { frame_texture.query_interface(&IID_ID3D11Resource).unwrap() });

		unsafe { readable_surface.query_interface(&IID_IDXGISurface1) }
	}

	fn release_frame(&mut self) -> Result<(), HRESULT> {
		let hr = self.dxgi_output_dup.ReleaseFrame();
		if hr_failed(hr) { Err(hr) } else { Ok(()) }
	}

	fn is_primary(&self) -> bool {
		unsafe {
			let mut output_desc = mem::zeroed();
			mem::transmute::<_, &mut Self>(self).output.GetDesc(&mut output_desc);
			let mut monitor_info: MONITORINFO = mem::zeroed();
			monitor_info.cbSize = mem::size_of::<MONITORINFO>() as u32;
			 GetMonitorInfoW(output_desc.Monitor, &mut monitor_info);

			(monitor_info.dwFlags & 1) != 0
		}
	}
}

pub struct DXGIManager {
	duplicated_output_i: Option<usize>,
	duplicated_outputs: Vec<DuplicatedOutput>,
	capture_source: usize,
	output_rect: RECT,
	frame_buf: Vec<u8>,
	timeout_ms: u32,
}
impl DXGIManager {
	fn new(timeout: Duration) -> Result<DXGIManager, &'static str> {
		let mut manager = DXGIManager{ duplicated_output_i: None,
			duplicated_outputs: Vec::with_capacity(3),
			capture_source: 0,
			output_rect: RECT{ left: 0, top: 0, right: 0, bottom: 0 },
			frame_buf: Vec::new(),
			timeout_ms: max(timeout.num_milliseconds(), 0) as u32 };

		match manager.refresh_output() {
			Ok(_) => Ok(manager),
			Err(e) => Err(e)
		}
	}

	fn set_capture_source(&mut self, cs: usize) {
		self.capture_source = cs;
		self.refresh_output().unwrap()
	}

	fn get_capture_source(&self) -> usize { self.capture_source }

	fn set_timeout(&mut self, t: Duration) { self.timeout_ms = max(t.num_milliseconds(), 0) as u32 }

	fn gather_output_duplications(&mut self) {
		// clear output duplications
		self.duplicated_output_i = None;
		self.duplicated_outputs.clear();

		let mut factory = create_dxgi_factory_1();

		for (outputs, mut adapter) in (0..).map(|i| {
				let mut adapter = ptr::null_mut();
				if factory.EnumAdapters1(i, &mut adapter) != DXGI_ERROR_NOT_FOUND {
					Some(unsafe { UniqueCOMPtr::from_ptr(adapter) })
				} else {
					None } })
			.take_while(Option::is_some).map(Option::unwrap)
			.map(|mut adapter| (get_adater_outputs(&mut adapter), adapter))
			.filter(|&(ref outs, _)| !outs.is_empty())
		{
			// Creating device for each adapter that has the output
			let (d3d11_device, device_context) = d3d11_create_device(&mut *adapter);

			let (d3d11_device, output_duplications) = outputs.into_iter()
				.map(|out| unsafe { out.query_interface::<IDXGIOutput1>(&IID_IDXGIOutput1).unwrap() })
				.fold((d3d11_device, Vec::new()), |(d3d11_device, mut out_dups), mut output| {
					let mut dxgi_device = unsafe {
						d3d11_device.query_interface::<IDXGIDevice1>(&IID_IDXGIDevice1).unwrap() };

					let duplicated_output = unsafe {
						let mut duplicated_output: *mut IDXGIOutputDuplication = ptr::null_mut();
						assert_eq!(0,
							output.DuplicateOutput(
								mem::transmute::<&mut IDXGIDevice1, _>(&mut dxgi_device),
								&mut duplicated_output));
						UniqueCOMPtr::from_ptr(duplicated_output) };
					out_dups.push((duplicated_output, output));
					(unsafe { dxgi_device.query_interface::<ID3D11Device>(&IID_ID3D11Device).unwrap() },
						out_dups)
				});

			let d3d11_device = Arc::new(Mutex::new(d3d11_device));
			let device_context = Arc::new(Mutex::new(device_context));

			for duplicated_output in output_duplications.into_iter()
				.map(|(duplicated_output, output)| {
					let (d3d11_device, device_context) = (d3d11_device.clone(), device_context.clone());

					DuplicatedOutput { device: d3d11_device,
						device_context: device_context,
						output: output,
						dxgi_output_dup: duplicated_output }
				})
			{
				self.duplicated_outputs.push(duplicated_output);
			}
		}

		for dup_out in & self.duplicated_outputs {
			let desc = dup_out.get_desc();
			println!("name: {}, coords: {:?}, attached: {:?}, rotation: {}",
				c_utf16_to_string(&desc.DeviceName),
				((desc.DesktopCoordinates.left, desc.DesktopCoordinates.top),
					(desc.DesktopCoordinates.right, desc.DesktopCoordinates.bottom)),
				desc.AttachedToDesktop, desc.Rotation as usize);
		}
	}

	fn refresh_output(&mut self) -> Result<(), &'static str> {
		self.gather_output_duplications();
		self.duplicated_output_i = self.active_output_duplication_index();
		if let None = self.duplicated_output_i {
			Err("Failed to get out")
		} else {
			Ok(()) }
	}

	fn active_output_duplication_index(&self) -> Option<usize> {
		if self.capture_source == 0 {
			self.duplicated_outputs.iter().position(|o| o.is_primary())
		} else {
			self.duplicated_outputs.iter()
				.enumerate()
				.filter_map(|(i, out)| if out.is_primary() {
					None
				} else {
					Some(i) })
				.nth(self.capture_source - 1)
		}
	}
}

#[test]
fn test() {
	let mut factory = unsafe {
		let mut factory: *mut c_void = ptr::null_mut();
		assert_eq!(0, CreateDXGIFactory1(&IID_IDXGIFactory1, &mut factory));
		UniqueCOMPtr::from_ptr(factory as *mut IDXGIFactory1) };

	let adapters: Vec<_> = (0..).map(|i| {
			let mut adapter = ptr::null_mut();
			if factory.EnumAdapters1(i, &mut adapter) != DXGI_ERROR_NOT_FOUND {
				Some(unsafe { UniqueCOMPtr::from_ptr(adapter) })
			} else { None } })
		.take_while(Option::is_some).map(Option::unwrap)
		.collect();

	let mut duplicated_outputs = Vec::with_capacity(4);

	for (outputs, mut adapter) in adapters.into_iter()
		.map(|mut adapter| (get_adater_outputs(&mut adapter), adapter))
		.filter(|&(ref outs, _)| !outs.is_empty())
	{
		// Creating device for each adapter that has the output
		let (d3d11_device, device_context) = unsafe {
			let mut d3d11_device: *mut ID3D11Device = ptr::null_mut();
			let mut device_context: *mut ID3D11DeviceContext = ptr::null_mut();
			assert_eq!(0,
				D3D11CreateDevice(mem::transmute::<&mut IDXGIAdapter1, _>(&mut adapter),
					D3D_DRIVER_TYPE::D3D_DRIVER_TYPE_UNKNOWN,
					ptr::null_mut(), 0, ptr::null_mut(), 0,
					D3D11_SDK_VERSION,
					&mut d3d11_device,
					&mut D3D_FEATURE_LEVEL::D3D_FEATURE_LEVEL_9_1,
					&mut device_context));
			(UniqueCOMPtr::from_ptr(d3d11_device as *mut ID3D11Device),
				UniqueCOMPtr::from_ptr(device_context)) };

		let (d3d11_device, output_duplications) = outputs.into_iter()
			.map(|out| unsafe { out.query_interface::<IDXGIOutput1>(&IID_IDXGIOutput1).unwrap() })
			.fold((d3d11_device, Vec::new()), |(d3d11_device, mut out_dups), mut output| {
				let mut dxgi_device = unsafe {
					d3d11_device.query_interface::<IDXGIDevice1>(&IID_IDXGIDevice1).unwrap() };

				let duplicated_output = unsafe {
					let mut duplicated_output: *mut IDXGIOutputDuplication = ptr::null_mut();
					assert_eq!(0,
						output.DuplicateOutput(
							mem::transmute::<&mut IDXGIDevice1, _>(&mut dxgi_device),
							&mut duplicated_output));
					UniqueCOMPtr::from_ptr(duplicated_output) };
				out_dups.push((duplicated_output, output));
				(unsafe { dxgi_device.query_interface::<ID3D11Device>(&IID_ID3D11Device).unwrap() },
					out_dups)
			});

		let d3d11_device = Arc::new(Mutex::new(d3d11_device));
		let device_context = Arc::new(Mutex::new(device_context));

		for duplicated_output in output_duplications.into_iter()
			.map(|(duplicated_output, output)| {
				let (d3d11_device, device_context) = (d3d11_device.clone(), device_context.clone());

				DuplicatedOutput { device: d3d11_device,
					device_context: device_context,
					output: output,
					dxgi_output_dup: duplicated_output }
			})
		{
			duplicated_outputs.push(duplicated_output);
		}
	}

	for dup_out in &mut duplicated_outputs {
		let desc = dup_out.get_desc();
		println!("name: {}, coords: {:?}, attached: {:?}, rotation: {}",
			c_utf16_to_string(&desc.DeviceName),
			((desc.DesktopCoordinates.left, desc.DesktopCoordinates.top),
				(desc.DesktopCoordinates.right, desc.DesktopCoordinates.bottom)),
			desc.AttachedToDesktop, desc.Rotation as usize);
	}
}