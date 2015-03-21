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

#![feature(unique, libc)]

extern crate libc;
extern crate winapi;
#[macro_use(c_mtdcall)]
extern crate dxgi;

use std::ptr::Unique;

/// A unique pointer to a COM object. Handles refcounting.
pub struct UniqueCOMPtr<T> {
	ptr: Unique<T>,
}
impl<T> UniqueCOMPtr<T> {
	/// Create a new 
	pub unsafe fn new(ptr: *mut T) -> UniqueCOMPtr<T> {
		UniqueCOMPtr{ ptr: Unique::new(ptr) }
	}
}

#[test]
fn test() {
	use std::ptr;
	use libc::{ c_void };
	use dxgi::{ CreateDXGIFactory1, IID_IDXGIFactory1, IDXGIFactory1 };

	let factory = {
		let mut factory: *mut c_void = ptr::null_mut();
		assert_eq!(0, unsafe { CreateDXGIFactory1(&IID_IDXGIFactory1, &mut factory) });
		factory as *mut IDXGIFactory1 };

	assert!(factory as usize != 0);

	println!("IsCurrent: {}", unsafe { c_mtdcall!(factory->IsCurrent()) } != 0);
	assert_eq!(unsafe { c_mtdcall!(factory->AddRef()) }, 2);
	assert_eq!(unsafe { c_mtdcall!(factory->Release()) }, 1);
}