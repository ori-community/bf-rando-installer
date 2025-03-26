use std::io;
use winresource::WindowsResource;

fn main() -> io::Result<()> {
    WindowsResource::new().set_icon("icon.ico").compile()?;
    Ok(())
}
