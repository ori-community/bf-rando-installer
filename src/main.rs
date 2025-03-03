use std::env::args;

use dll_classifier::classify_file;

mod dll_classifier;
mod dll_parser;

fn main() {
    match main_impl() {
        Ok(results) => {
            for msg in results {
                println!("{msg}");
            }
        }
        Err(msg) => eprintln!("{msg}"),
    }
}

fn main_impl() -> Result<Vec<String>, &'static str> {
    let dir_path = args().skip(1).next().ok_or("Missing dir path argument")?;

    let dir = std::fs::read_dir(dir_path).map_err(|_| "Couldn't read dir")?;

    let mut results = Vec::new();

    for file in dir {
        let file = file.map_err(|_| "Couldn't step file")?;

        if !file
            .file_type()
            .map_err(|_| "Couldn't file type file")?
            .is_file()
        {
            continue;
        }

        let file_name = file.file_name();

        let file_data = std::fs::read(file.path()).map_err(|_| "Couldn't read file")?;

        let result = classify_file(&file_data);
        results.push(format!("{}: {:?}", file_name.to_string_lossy(), result));
    }

    Ok(results)
}
