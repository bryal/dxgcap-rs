fn main() {
	// Dynamic linking
	println!("cargo:rustc-link-search=native={}", "C:/Windows/System32/");
}