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

use dxgi::{ IUnknownT };

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

#[test]
fn test() {
	use std::ptr;
	use libc::{ c_void };
	use dxgi::interfaces::*;
	use dxgi::{ CreateDXGIFactory1, IID_IDXGIFactory1, DXGI_ERROR_NOT_FOUND };

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
				println!("{}", i);
				Some(unsafe { UniqueCOMPtr::from_ptr(adapter) })
			} else { None }
		})
		.take_while(|v| if let &None = v { false } else { true })
		.map(|v| v.unwrap())
		.collect();

		
}