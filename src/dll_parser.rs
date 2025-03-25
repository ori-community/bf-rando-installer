use std::ops::Range;

pub struct DllHeaps<'a> {
    pub strings: &'a [u8],
    pub us: &'a [u8],
}

pub fn parse_dll(data: &[u8]) -> Result<DllHeaps<'_>, &'static str> {
    let lfanew = read_u32(data, 60, "EOF lfanew")? as usize;

    let pe_header = data.get(lfanew..).ok_or("Invalid lfanew")?;

    if pe_header.get(0..4) != Some(b"PE\0\0") {
        return Err("Invalid PE magic");
    }

    let num_sections = read_u16(pe_header, 6, "EOF num_sections")? as usize;
    let opt_header_size = read_u16(pe_header, 20, "EOF opt_header_size")? as usize;

    let optional_header = pe_header
        .get(24..24 + opt_header_size)
        .ok_or("EOF optional_header")?;

    let sections: &[_] = &{
        let mut sections = Vec::with_capacity(num_sections);

        for i in 0..num_sections {
            let section_start = 24 + opt_header_size + i * 40;

            let virtual_size = read_u32(pe_header, section_start + 8, "EOF section")?;
            let virtual_start = read_u32(pe_header, section_start + 12, "EOF section")?;
            let file_size = read_u32(pe_header, section_start + 16, "EOF section")? as usize;
            let file_start = read_u32(pe_header, section_start + 20, "EOF section")? as usize;

            let file_bytes = data
                .get(file_start..file_start + file_size)
                .ok_or("EOF section data")?;

            sections.push(DllSection {
                virtual_range: virtual_start..virtual_start + virtual_size,
                file_bytes,
            });
        }

        sections
    };

    let cli_header_rva = read_u32(optional_header, 208, "opt_header too small")?;
    let cli_header = resolve_rva(cli_header_rva, sections, "Invalid CLI header RVA")?;

    let metadata_rva = read_u32(cli_header, 8, "EOF metadata rva")?;
    let metadata = resolve_rva(metadata_rva, sections, "Invalid metadata RVA")?;

    if metadata.get(0..4) != Some(b"BSJB") {
        return Err("Invalid metadata magic");
    }

    let version_length =
        read_u32(metadata, 12, "EOF metadata version length")?.next_multiple_of(4) as usize;
    let num_streams = read_u16(metadata, 16 + version_length + 2, "EOF num_streams")? as usize;

    let streams: &[_] = &{
        let mut stream_header = metadata
            .get(16 + version_length + 4..)
            .ok_or("EOF streams")?;

        let mut streams = Vec::with_capacity(num_streams);

        for _ in 0..num_streams {
            let offset = read_u32(stream_header, 0, "EOF stream offset")? as usize;
            let size = read_u32(stream_header, 4, "EOF stream size")? as usize;

            let name_length = stream_header[8..]
                .iter()
                .position(|&c| c == 0)
                .ok_or("EOF stream name")?;

            streams.push(CliStream {
                name: &stream_header[8..8 + name_length],
                data: metadata
                    .get(offset..offset + size)
                    .ok_or("EOF stream data")?,
            });

            let rounded_name_length = (name_length + 1).next_multiple_of(4);

            stream_header = stream_header
                .get(8 + rounded_name_length..)
                .ok_or("EOF next stream")?;
        }

        streams
    };

    let strings_heap = streams
        .iter()
        .find(|&s| s.name == b"#Strings")
        .ok_or("No #Strings heap")?;

    let us_heap = streams
        .iter()
        .find(|&s| s.name == b"#US")
        .ok_or("No #US heap")?;

    Ok(DllHeaps {
        strings: strings_heap.data,
        us: us_heap.data,
    })
}

fn read_u16(data: &[u8], offset: usize, error_msg: &'static str) -> Result<u16, &'static str> {
    match data.get(offset..offset + 2) {
        None => Err(error_msg),
        Some(bytes) => Ok(u16::from_le_bytes(bytes.try_into().unwrap())),
    }
}

fn read_u32(data: &[u8], offset: usize, error_msg: &'static str) -> Result<u32, &'static str> {
    match data.get(offset..offset + 4) {
        None => Err(error_msg),
        Some(bytes) => Ok(u32::from_le_bytes(bytes.try_into().unwrap())),
    }
}

struct DllSection<'a> {
    virtual_range: Range<u32>,
    file_bytes: &'a [u8],
}

fn resolve_rva<'a>(
    rva: u32,
    sections: &[DllSection<'a>],
    error_msg: &'static str,
) -> Result<&'a [u8], &'static str> {
    for section in sections {
        if section.virtual_range.contains(&rva) {
            let section_offset = rva - section.virtual_range.start;
            return section
                .file_bytes
                .get(section_offset as usize..)
                .ok_or(error_msg);
        }
    }

    Err(error_msg)
}

struct CliStream<'a> {
    name: &'a [u8],
    data: &'a [u8],
}
