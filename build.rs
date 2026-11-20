use std::io;
use winresource::WindowsResource;

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=icon.ico");
    WindowsResource::new().set_icon("icon.ico").compile()?;
    Ok(())
}
