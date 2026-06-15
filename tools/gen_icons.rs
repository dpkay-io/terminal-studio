// Standalone icon generator for Terminal Studio.
// Compile and run: rustc tools/gen_icons.rs -o tools/gen_icons.exe && tools\gen_icons.exe
// Generates PNG, ICO, and ICNS icon files in assets/.

use std::fs;
use std::path::Path;

const BG: [u8; 4] = [30, 30, 46, 255];
const FG: [u8; 4] = [137, 180, 250, 255];

// TS monogram rectangles on 32x32 grid: (x0, x1, y0, y1)
const RECTS: &[(u32, u32, u32, u32)] = &[
    (2, 13, 5, 8),   // T horizontal bar
    (6, 9, 8, 27),   // T vertical stem
    (16, 28, 5, 8),  // S top bar
    (16, 19, 8, 15), // S top-left
    (16, 28, 14, 17),// S middle bar
    (25, 28, 17, 24),// S bottom-right
    (16, 28, 24, 27),// S bottom bar
];

fn render_icon(size: u32) -> Vec<u8> {
    let n = size as usize;
    let scale = size as f64 / 32.0;
    let mut rgba = vec![0u8; n * n * 4];

    for y in 0..n {
        for x in 0..n {
            rgba[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&BG);
        }
    }

    for &(x0, x1, y0, y1) in RECTS {
        let sx0 = (x0 as f64 * scale).round() as usize;
        let sx1 = (x1 as f64 * scale).round() as usize;
        let sy0 = (y0 as f64 * scale).round() as usize;
        let sy1 = (y1 as f64 * scale).round() as usize;
        for y in sy0..sy1.min(n) {
            for x in sx0..sx1.min(n) {
                rgba[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&FG);
            }
        }
    }
    rgba
}

// ── Bit-level writer (LSB-first, as required by deflate) ──

struct BitWriter {
    bytes: Vec<u8>,
    buf: u32,
    count: u8,
}

impl BitWriter {
    fn new() -> Self { Self { bytes: Vec::new(), buf: 0, count: 0 } }

    fn write(&mut self, value: u32, bits: u8) {
        self.buf |= value << self.count;
        self.count += bits;
        while self.count >= 8 {
            self.bytes.push(self.buf as u8);
            self.buf >>= 8;
            self.count -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 { self.bytes.push(self.buf as u8); }
        self.bytes
    }
}

fn reverse_bits(mut code: u32, len: u8) -> u32 {
    let mut r = 0u32;
    for _ in 0..len {
        r = (r << 1) | (code & 1);
        code >>= 1;
    }
    r
}

// Fixed Huffman: literal/length encoding
fn emit_literal(bw: &mut BitWriter, byte: u8) {
    let v = byte as u32;
    if v <= 143 {
        bw.write(reverse_bits(0x30 + v, 8), 8);
    } else {
        bw.write(reverse_bits(0x190 + v - 144, 9), 9);
    }
}

fn emit_end_of_block(bw: &mut BitWriter) {
    bw.write(reverse_bits(0, 7), 7); // code 256 = 0000000 (7 bits)
}

fn emit_length(bw: &mut BitWriter, length: u32) {
    let (code, extra_bits, extra_val) = match length {
        3..=10   => (257 + length - 3, 0u8, 0u32),
        11..=18  => (265 + (length - 11) / 2, 1, (length - 11) % 2),
        19..=34  => (269 + (length - 19) / 4, 2, (length - 19) % 4),
        35..=66  => (273 + (length - 35) / 8, 3, (length - 35) % 8),
        67..=130 => (277 + (length - 67) / 16, 4, (length - 67) % 16),
        131..=257=> (281 + (length - 131) / 32, 5, (length - 131) % 32),
        258      => (285, 0, 0),
        _ => unreachable!(),
    };

    if code <= 279 {
        bw.write(reverse_bits(code - 256, 7), 7);
    } else {
        bw.write(reverse_bits(0xC0 + code - 280, 8), 8);
    }
    if extra_bits > 0 {
        bw.write(extra_val, extra_bits);
    }
}

fn emit_distance_1(bw: &mut BitWriter) {
    bw.write(reverse_bits(0, 5), 5);
}

// Deflate using fixed Huffman codes with byte-level RLE (distance=1 matches)
fn deflate_rle(data: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter::new();
    bw.write(0b011, 3); // BFINAL=1, BTYPE=01 (fixed Huffman)

    let mut i = 0;
    while i < data.len() {
        let byte = data[i];
        let mut run_end = i + 1;
        while run_end < data.len() && data[run_end] == byte {
            run_end += 1;
        }

        emit_literal(&mut bw, byte);
        let mut remaining = run_end - i - 1;

        while remaining >= 3 {
            let chunk = remaining.min(258);
            emit_length(&mut bw, chunk as u32);
            emit_distance_1(&mut bw);
            remaining -= chunk;
        }
        for _ in 0..remaining {
            emit_literal(&mut bw, byte);
        }

        i = run_end;
    }

    emit_end_of_block(&mut bw);
    bw.finish()
}

// ── Checksums ──

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b): (u32, u32) = (1, 0);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

// ── PNG encoder ──

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;

    // Apply Sub filter (type 1): filtered[x] = raw[x] - raw[x-4] for x>=4
    let row_bytes = w * 4;
    let mut filtered = Vec::with_capacity(h * (1 + row_bytes));
    for y in 0..h {
        filtered.push(1); // filter type: Sub
        let row = &rgba[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..row_bytes {
            if x < 4 {
                filtered.push(row[x]);
            } else {
                filtered.push(row[x].wrapping_sub(row[x - 4]));
            }
        }
    }

    // Compress with deflate RLE
    let deflated = deflate_rle(&filtered);

    // Wrap in zlib: CMF + FLG + deflated + adler32
    let mut zlib_data = Vec::new();
    zlib_data.push(0x78);
    zlib_data.push(0x01);
    zlib_data.extend_from_slice(&deflated);
    zlib_data.extend_from_slice(&adler32(&filtered).to_be_bytes());

    // Build PNG
    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]); // signature

    // IHDR
    write_chunk(&mut out, b"IHDR", &{
        let mut d = Vec::new();
        d.extend_from_slice(&width.to_be_bytes());
        d.extend_from_slice(&height.to_be_bytes());
        d.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace
        d
    });

    // IDAT
    write_chunk(&mut out, b"IDAT", &zlib_data);

    // IEND
    write_chunk(&mut out, b"IEND", &[]);

    out
}

fn write_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc_input);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

// ── ICO encoder (BMP entries, no compression needed) ──

fn encode_ico(sizes: &[u32]) -> Vec<u8> {
    let entries: Vec<(u32, Vec<u8>)> = sizes.iter().map(|&s| {
        let rgba = render_icon(s);
        (s, encode_ico_bmp(&rgba, s))
    }).collect();

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    let header_size = 6 + entries.len() * 16;
    let mut offset = header_size;

    for (size, bmp) in &entries {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(bmp.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += bmp.len();
    }

    for (_, bmp) in &entries {
        out.extend_from_slice(bmp);
    }
    out
}

fn encode_ico_bmp(rgba: &[u8], size: u32) -> Vec<u8> {
    let n = size as usize;
    let mut out = Vec::with_capacity(40 + n * n * 4 + ((n + 31) / 32) * 4 * n);

    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 20]); // compression through biClrImportant = 0

    // BGRA pixels, bottom-up
    for y in (0..n).rev() {
        for x in 0..n {
            let off = (y * n + x) * 4;
            out.extend_from_slice(&[rgba[off + 2], rgba[off + 1], rgba[off], rgba[off + 3]]);
        }
    }

    // AND mask (all zeros)
    let mask_row = vec![0u8; ((n + 31) / 32) * 4];
    for _ in 0..n {
        out.extend_from_slice(&mask_row);
    }
    out
}

// ── ICNS encoder (wraps PNG entries) ──

fn encode_icns(entries: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&0u32.to_be_bytes()); // placeholder

    for (size, png) in entries {
        let ostype = match size {
            16 => b"icp4", 32 => b"icp5", 64 => b"icp6",
            128 => b"ic07", 256 => b"ic08", 512 => b"ic09", 1024 => b"ic10",
            _ => continue,
        };
        let entry_size = (8 + png.len()) as u32;
        out.extend_from_slice(ostype);
        out.extend_from_slice(&entry_size.to_be_bytes());
        out.extend_from_slice(png);
    }

    let total = out.len() as u32;
    out[4..8].copy_from_slice(&total.to_be_bytes());
    out
}

fn main() {
    let assets = Path::new("assets");
    fs::create_dir_all(assets).expect("create assets/");

    println!("Generating icons...");

    let png_sizes: &[u32] = &[16, 32, 48, 64, 128, 256, 512];
    let mut png_map: Vec<(u32, Vec<u8>)> = Vec::new();

    for &size in png_sizes {
        let rgba = render_icon(size);
        let png = encode_png(&rgba, size, size);
        let name = format!("icon-{}.png", size);
        let path = assets.join(&name);
        fs::write(&path, &png).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        println!("  {} ({} bytes)", name, png.len());
        png_map.push((size, png));
    }

    let ico = encode_ico(&[16, 32, 48, 256]);
    fs::write(assets.join("icon.ico"), &ico).expect("write icon.ico");
    println!("  icon.ico ({} bytes)", ico.len());

    let icns_entries: Vec<(u32, Vec<u8>)> = png_map.iter()
        .filter(|(s, _)| matches!(s, 16 | 32 | 128 | 256 | 512))
        .cloned()
        .collect();
    let icns = encode_icns(&icns_entries);
    fs::write(assets.join("icon.icns"), &icns).expect("write icon.icns");
    println!("  icon.icns ({} bytes)", icns.len());

    println!("Done!");
}
