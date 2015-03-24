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

#![feature(libc)]
#![feature(unsafe_destructor)]

extern crate libc;
extern crate winapi;
#[macro_use(c_mtdcall)]
extern crate dxgi;
extern crate d3d11;

use libc::c_void;
use std::mem;
use std::ptr;
use winapi::{ HRESULT, IID };
use dxgi::interfaces::*;

/// A unique pointer to a COM object. Handles refcounting.
pub struct UniqueCOMPtr<T: IUnknownT> {
	ptr: *mut T,
}
impl<T: IUnknownT> UniqueCOMPtr<T> {
	/// Construct a new unique COM pointer from a pointer to a COM object.
	/// It is the users responsibility to guarantee that no copies of the pointer exists beforehand 
	pub unsafe fn from_ptr(ptr: *mut T) -> UniqueCOMPtr<T> {
		UniqueCOMPtr{ ptr: ptr }
	}

	pub unsafe fn query_interface<U>(&mut self, interface_identifier: &IID)
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
		unsafe { &*self.ptr }
	}
}
impl<T: IUnknownT> std::ops::DerefMut for UniqueCOMPtr<T> {
	fn deref_mut(&mut self) -> &mut T {
		unsafe { &mut*self.ptr }
	}
}
#[unsafe_destructor]
impl<T: IUnknownT> std::ops::Drop for UniqueCOMPtr<T> {
	fn drop(&mut self) {
		self.Release();
	}
}

fn hr_failed(hr: HRESULT) -> bool {
	hr < 0
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

#[test]
fn test() {
	use libc::{ c_void };
	use dxgi::{ CreateDXGIFactory1, IID_IDXGIFactory1, IID_IDXGIOutput1,
		IID_IDXGIDevice1, DXGI_ERROR_NOT_FOUND };
	use d3d11::{ D3D_DRIVER_TYPE, D3D11_SDK_VERSION, D3D_FEATURE_LEVEL,
		D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext };

	let mut factory = unsafe {
		let mut factory: *mut c_void = ptr::null_mut();
		assert_eq!(0, CreateDXGIFactory1(&IID_IDXGIFactory1, &mut factory));
		UniqueCOMPtr::from_ptr(factory as *mut IDXGIFactory1) };

	assert!(&factory as *const _ as usize != 0);

	println!("IsCurrent: {}", factory.IsCurrent() != 0);
	assert_eq!(factory.AddRef(), 2);
	assert_eq!(factory.Release(), 1);

	let adapters: Vec<_> = (0..).map(|i| {
			let mut adapter = ptr::null_mut();
			if factory.EnumAdapters1(i, &mut adapter) != DXGI_ERROR_NOT_FOUND {
				Some(unsafe { UniqueCOMPtr::from_ptr(adapter) })
			} else { None } })
		.take_while(Option::is_some).map(Option::unwrap)
		.collect();

	for (mut outputs, mut adapter) in adapters.into_iter()
		.map(|mut adapter| (get_adater_outputs(&mut adapter), adapter))
		.filter(|&(ref outs, _)| !outs.is_empty())
	{
		// Creating device for each adapter that has the output
		let (mut d3d11_device, device_context) = unsafe {
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

		for mut output in outputs.into_iter()
			.map(|mut o| unsafe { o.query_interface::<IDXGIOutput1>(&IID_IDXGIOutput1).unwrap() })
		{
			let mut dxgi_device = unsafe {
				d3d11_device.query_interface::<IDXGIDevice1>(&IID_IDXGIDevice1).unwrap() };

			let duplicated_output = unsafe {
				let mut duplicated_output: *mut IDXGIOutputDuplication = ptr::null_mut();
				assert_eq!(0,
					output.DuplicateOutput(mem::transmute::<&mut IDXGIDevice1, _>(&mut dxgi_device),
						&mut duplicated_output));
				UniqueCOMPtr::from_ptr(duplicated_output) };
		}
	}
}