//! The picture inside a PDF page, for scanned maps delivered as PDF
//! (e.g. the Italian cadastral "estratto di mappa": one page holding one
//! large 1-bit image).
//!
//! This is not a PDF renderer: it finds the largest image XObject in the
//! file and decodes it. Supported: FlateDecode (with PNG predictors) and
//! DCTDecode (JPEG) data; DeviceGray, DeviceRGB, DeviceCMYK, ICCBased and
//! Indexed colours; 1, 2, 4, 8 bits per component; `/Decode [1 0]` and
//! image masks. Vector drawings and other compressions (CCITT fax, JBIG2,
//! JPEG 2000) are reported as unsupported. Host-free and unit-tested.

use std::collections::HashMap;

/// A decoded picture: RGBA8, row 0 at the top.
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// Resolution when the picture fills the page (from the MediaBox).
    pub dpi: Option<f64>,
}

/// A JPEG picture is decoded by the caller (with the `image` crate).
pub enum Found {
    Raw(Picture),
    Jpeg { data: Vec<u8>, w: u32, h: u32, dpi: Option<f64> },
}

// ── A minimal PDF object reader ─────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Obj {
    Null,
    Bool(bool),
    Num(f64),
    Name(String),
    Str(Vec<u8>),
    Array(Vec<Obj>),
    Dict(HashMap<String, Obj>),
    Ref(u32, u32),
}

impl Obj {
    fn num(&self) -> Option<f64> {
        match self {
            Obj::Num(n) => Some(*n),
            _ => None,
        }
    }
    fn name(&self) -> Option<&str> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0')
}
fn is_delim(c: u8) -> bool {
    matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

struct Lexer<'a> {
    d: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.d.len() {
            let c = self.d[self.i];
            if is_ws(c) {
                self.i += 1;
            } else if c == b'%' {
                while self.i < self.d.len() && self.d[self.i] != b'\n' && self.d[self.i] != b'\r' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn word(&mut self) -> &'a [u8] {
        let s = self.i;
        while self.i < self.d.len() && !is_ws(self.d[self.i]) && !is_delim(self.d[self.i]) {
            self.i += 1;
        }
        &self.d[s..self.i]
    }

    fn value(&mut self, depth: usize) -> Option<Obj> {
        if depth > 32 {
            return None;
        }
        self.skip_ws();
        let c = *self.d.get(self.i)?;
        match c {
            b'/' => {
                self.i += 1;
                let w = self.word();
                // #xx escapes in names
                let mut n = Vec::with_capacity(w.len());
                let mut k = 0;
                while k < w.len() {
                    if w[k] == b'#' && k + 2 < w.len() {
                        if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&w[k + 1..k + 3]).ok()?, 16) {
                            n.push(b);
                            k += 3;
                            continue;
                        }
                    }
                    n.push(w[k]);
                    k += 1;
                }
                Some(Obj::Name(String::from_utf8_lossy(&n).into_owned()))
            }
            b'<' if self.d.get(self.i + 1) == Some(&b'<') => {
                self.i += 2;
                let mut m = HashMap::new();
                loop {
                    self.skip_ws();
                    if self.d.get(self.i..self.i + 2)? == &b">>"[..] {
                        self.i += 2;
                        return Some(Obj::Dict(m));
                    }
                    let Obj::Name(k) = self.value(depth + 1)? else { return None };
                    let v = self.value(depth + 1)?;
                    m.insert(k, v);
                }
            }
            b'<' => {
                self.i += 1;
                let s = self.i;
                while *self.d.get(self.i)? != b'>' {
                    self.i += 1;
                }
                let hex: Vec<u8> = self.d[s..self.i].iter().copied().filter(|c| c.is_ascii_hexdigit()).collect();
                self.i += 1;
                let mut out = Vec::with_capacity(hex.len() / 2 + 1);
                for pair in hex.chunks(2) {
                    let t = if pair.len() == 2 { [pair[0], pair[1]] } else { [pair[0], b'0'] };
                    out.push(u8::from_str_radix(std::str::from_utf8(&t).ok()?, 16).ok()?);
                }
                Some(Obj::Str(out))
            }
            b'(' => {
                self.i += 1;
                let mut out = Vec::new();
                let mut level = 1;
                loop {
                    let c = *self.d.get(self.i)?;
                    self.i += 1;
                    match c {
                        b'\\' => {
                            let e = *self.d.get(self.i)?;
                            self.i += 1;
                            match e {
                                b'n' => out.push(b'\n'),
                                b'r' => out.push(b'\r'),
                                b't' => out.push(b'\t'),
                                b'b' => out.push(8),
                                b'f' => out.push(12),
                                b'0'..=b'7' => {
                                    let mut v = (e - b'0') as u32;
                                    for _ in 0..2 {
                                        match self.d.get(self.i) {
                                            Some(&d @ b'0'..=b'7') => {
                                                v = v * 8 + (d - b'0') as u32;
                                                self.i += 1;
                                            }
                                            _ => break,
                                        }
                                    }
                                    out.push(v as u8);
                                }
                                b'\r' => {
                                    if self.d.get(self.i) == Some(&b'\n') {
                                        self.i += 1;
                                    }
                                }
                                b'\n' => {}
                                o => out.push(o),
                            }
                        }
                        b'(' => {
                            level += 1;
                            out.push(c);
                        }
                        b')' => {
                            level -= 1;
                            if level == 0 {
                                return Some(Obj::Str(out));
                            }
                            out.push(c);
                        }
                        _ => out.push(c),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.skip_ws();
                    if *self.d.get(self.i)? == b']' {
                        self.i += 1;
                        return Some(Obj::Array(v));
                    }
                    v.push(self.value(depth + 1)?);
                }
            }
            _ => {
                let w = self.word();
                if w.is_empty() {
                    return None;
                }
                let t = std::str::from_utf8(w).ok()?;
                match t {
                    "true" => return Some(Obj::Bool(true)),
                    "false" => return Some(Obj::Bool(false)),
                    "null" => return Some(Obj::Null),
                    _ => {}
                }
                let n: f64 = t.parse().ok()?;
                // "N G R" is a reference
                let save = self.i;
                self.skip_ws();
                let g = self.word();
                if !g.is_empty() && g.iter().all(u8::is_ascii_digit) {
                    self.skip_ws();
                    if self.d.get(self.i) == Some(&b'R') && self.d.get(self.i + 1).is_none_or(|&c| is_ws(c) || is_delim(c)) {
                        self.i += 1;
                        let g: u32 = std::str::from_utf8(g).ok()?.parse().ok()?;
                        return Some(Obj::Ref(n as u32, g));
                    }
                }
                self.i = save;
                Some(Obj::Num(n))
            }
        }
    }
}

/// Position just after `N G obj` for a top-level object, if written plainly.
fn find_object(d: &[u8], num: u32, gen: u32) -> Option<usize> {
    let pat = format!("{num} {gen} obj");
    let p = pat.as_bytes();
    let mut from = 0;
    while let Some(k) = find(&d[from..], p) {
        let at = from + k;
        let ok_before = at == 0 || is_ws(d[at - 1]);
        if ok_before {
            return Some(at + p.len());
        }
        from = at + 1;
    }
    None
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    h.windows(n.len()).position(|w| w == n)
}

/// Follow an indirect reference to a plainly written object.
fn resolve(d: &[u8], o: &Obj) -> Option<Obj> {
    match o {
        Obj::Ref(n, g) => {
            let at = find_object(d, *n, *g)?;
            Lexer { d, i: at }.value(0)
        }
        other => Some(other.clone()),
    }
}

/// Every stream in the file: (dictionary, raw data).
fn streams(d: &[u8]) -> Vec<(HashMap<String, Obj>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = find(&d[from..], b" obj") {
        let start = from + k + 4;
        from = start;
        let mut lx = Lexer { d, i: start };
        let Some(Obj::Dict(dict)) = lx.value(0) else { continue };
        lx.skip_ws();
        if d.get(lx.i..lx.i + 6) != Some(&b"stream"[..]) {
            continue;
        }
        let mut s = lx.i + 6;
        if d.get(s) == Some(&b'\r') {
            s += 1;
        }
        if d.get(s) == Some(&b'\n') {
            s += 1;
        }
        let len = dict.get("Length").and_then(|l| resolve(d, l)).and_then(|l| l.num()).map(|l| l as usize);
        let end = match len {
            Some(l) if s + l <= d.len() && find(&d[s + l..(s + l + 20).min(d.len())], b"endstream").is_some() => s + l,
            _ => match find(&d[s..], b"endstream") {
                Some(e) => {
                    let mut e = s + e;
                    while e > s && (d[e - 1] == b'\n' || d[e - 1] == b'\r') {
                        e -= 1;
                    }
                    e
                }
                None => continue,
            },
        };
        out.push((dict, d[s..end].to_vec()));
        from = end;
    }
    out
}

// ── Inflate (RFC 1950 / 1951) ───────────────────────────────────────────────

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    bit: u32,
    nbits: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Result<(), String> {
        while self.nbits < n {
            let b = *self.d.get(self.pos).ok_or("compressed data ends early")?;
            self.pos += 1;
            self.bit |= (b as u32) << self.nbits;
            self.nbits += 8;
        }
        Ok(())
    }
    fn get(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.nbits -= n;
        Ok(v)
    }
}

struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    fn decode(&self, b: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.get(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return self.symbols.get((index + (code - first)) as usize).copied().ok_or_else(|| "bad code".into());
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("invalid Huffman code".into())
    }
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Decompress zlib (or, when there is no zlib header, raw deflate) data.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let body = if data.len() >= 2 && data[0] & 0x0f == 8 && ((data[0] as u16) << 8 | data[1] as u16) % 31 == 0 {
        &data[2..]
    } else {
        data
    };
    let mut b = Bits { d: body, pos: 0, bit: 0, nbits: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(data.len() * 4);
    loop {
        let last = b.get(1)?;
        match b.get(2)? {
            0 => {
                b.bit = 0;
                b.nbits = 0;
                let p = b.pos;
                let hdr = body.get(p..p + 4).ok_or("stored block ends early")?;
                let len = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
                let chunk = body.get(p + 4..p + 4 + len).ok_or("stored block ends early")?;
                out.extend_from_slice(chunk);
                b.pos = p + 4 + len;
            }
            1 => {
                let mut l = [0u8; 288];
                l[..144].fill(8);
                l[144..256].fill(9);
                l[256..280].fill(7);
                l[280..].fill(8);
                let lit = Huffman::new(&l);
                let dist = Huffman::new(&[5u8; 30]);
                codes(&mut b, &mut out, &lit, &dist)?;
            }
            2 => {
                let hlit = b.get(5)? as usize + 257;
                let hdist = b.get(5)? as usize + 1;
                let hclen = b.get(4)? as usize + 4;
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut cl = [0u8; 19];
                for &o in ORDER.iter().take(hclen) {
                    cl[o] = b.get(3)? as u8;
                }
                let clh = Huffman::new(&cl);
                let mut lens = vec![0u8; hlit + hdist];
                let mut i = 0;
                while i < hlit + hdist {
                    let sym = clh.decode(&mut b)?;
                    match sym {
                        0..=15 => {
                            lens[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            let prev = *lens.get(i.wrapping_sub(1)).ok_or("repeat with no previous length")?;
                            for _ in 0..3 + b.get(2)? {
                                *lens.get_mut(i).ok_or("too many lengths")? = prev;
                                i += 1;
                            }
                        }
                        17 => i += 3 + b.get(3)? as usize,
                        _ => i += 11 + b.get(7)? as usize,
                    }
                }
                if i > hlit + hdist {
                    return Err("too many code lengths".into());
                }
                let lit = Huffman::new(&lens[..hlit]);
                let dist = Huffman::new(&lens[hlit..]);
                codes(&mut b, &mut out, &lit, &dist)?;
            }
            _ => return Err("invalid deflate block".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

fn codes(b: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman) -> Result<(), String> {
    loop {
        let sym = lit.decode(b)? as usize;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let k = sym - 257;
            if k >= 29 {
                return Err("invalid length code".into());
            }
            let len = LBASE[k] as usize + b.get(LEXT[k] as u32)? as usize;
            let dk = dist.decode(b)? as usize;
            if dk >= 30 {
                return Err("invalid distance code".into());
            }
            let d = DBASE[dk] as usize + b.get(DEXT[dk] as u32)? as usize;
            if d > out.len() {
                return Err("distance too far back".into());
            }
            let start = out.len() - d;
            for j in 0..len {
                out.push(out[start + j]);
            }
        }
    }
}

/// Undo PNG row predictors (`/Predictor` 10–15).
fn unpredict(data: &[u8], colors: usize, bpc: usize, columns: usize) -> Result<Vec<u8>, String> {
    let bpp = (colors * bpc).div_ceil(8).max(1);
    let row = (columns * colors * bpc).div_ceil(8);
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; row];
    for chunk in data.chunks(row + 1) {
        if chunk.len() < row + 1 {
            break;
        }
        let ft = chunk[0];
        let mut cur = chunk[1..].to_vec();
        for i in 0..row {
            let a = if i >= bpp { cur[i - bpp] } else { 0 } as i16;
            let b = prev[i] as i16;
            let c = if i >= bpp { prev[i - bpp] } else { 0 } as i16;
            let add = match ft {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => {
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => return Err(format!("unknown PNG predictor {ft}")),
            };
            cur[i] = cur[i].wrapping_add(add as u8);
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(out)
}

// ── The page picture ────────────────────────────────────────────────────────

enum Colours {
    Gray,
    Rgb,
    Cmyk,
    Indexed { base: usize, table: Vec<u8> },
}

impl Colours {
    fn comps(&self) -> usize {
        match self {
            Colours::Gray | Colours::Indexed { .. } => 1,
            Colours::Rgb => 3,
            Colours::Cmyk => 4,
        }
    }
}

fn colours(d: &[u8], cs: Option<&Obj>, depth: usize) -> Option<Colours> {
    if depth > 4 {
        return None;
    }
    let cs = resolve(d, cs?)?;
    match &cs {
        Obj::Name(n) => match n.as_str() {
            "DeviceGray" | "CalGray" | "G" => Some(Colours::Gray),
            "DeviceRGB" | "CalRGB" | "RGB" => Some(Colours::Rgb),
            "DeviceCMYK" | "CMYK" => Some(Colours::Cmyk),
            _ => None,
        },
        Obj::Array(a) => match a.first()?.name()? {
            "ICCBased" => {
                // N from the profile stream's dictionary
                let n = match a.get(1)? {
                    Obj::Ref(num, gen) => {
                        let at = find_object(d, *num, *gen)?;
                        match (Lexer { d, i: at }).value(0)? {
                            Obj::Dict(m) => m.get("N").and_then(|n| n.num()),
                            _ => None,
                        }
                    }
                    _ => None,
                }?;
                match n as u32 {
                    1 => Some(Colours::Gray),
                    3 => Some(Colours::Rgb),
                    4 => Some(Colours::Cmyk),
                    _ => None,
                }
            }
            "Indexed" | "I" => {
                let base = colours(d, a.get(1), depth + 1)?;
                if matches!(base, Colours::Indexed { .. }) {
                    return None;
                }
                let table = match resolve(d, a.get(3)?)? {
                    Obj::Str(s) => s,
                    _ => return None,
                };
                Some(Colours::Indexed { base: base.comps(), table })
            }
            "CalRGB" => Some(Colours::Rgb),
            "CalGray" => Some(Colours::Gray),
            _ => None,
        },
        _ => None,
    }
}

fn filters(d: &[u8], f: Option<&Obj>) -> Vec<String> {
    match f.and_then(|f| resolve(d, f)) {
        Some(Obj::Name(n)) => vec![n],
        Some(Obj::Array(a)) => a.iter().filter_map(|o| o.name().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

fn first_parms(d: &[u8], p: Option<&Obj>) -> Option<HashMap<String, Obj>> {
    match resolve(d, p?)? {
        Obj::Dict(m) => Some(m),
        Obj::Array(a) => a.into_iter().find_map(|o| match resolve(d, &o) {
            Some(Obj::Dict(m)) => Some(m),
            _ => None,
        }),
        _ => None,
    }
}

/// Page width and height in points, looked for in plain objects and inside
/// compressed object streams.
fn page_size(d: &[u8], all: &[(HashMap<String, Obj>, Vec<u8>)]) -> Option<(f64, f64)> {
    let mut texts: Vec<Vec<u8>> = vec![d.to_vec()];
    for (dict, data) in all {
        if dict.get("Type").and_then(Obj::name) == Some("ObjStm") && filters(d, dict.get("Filter")).iter().all(|f| f == "FlateDecode") {
            if let Ok(t) = inflate(data) {
                texts.push(t);
            }
        }
    }
    for t in &texts {
        if let Some(k) = find(t, b"/MediaBox") {
            let mut lx = Lexer { d: t, i: k + 9 };
            let v = lx.value(0)?;
            let v = resolve(t, &v).or(Some(v))?;
            if let Obj::Array(a) = v {
                let n: Vec<f64> = a.iter().filter_map(Obj::num).collect();
                if n.len() == 4 {
                    let (w, h) = ((n[2] - n[0]).abs(), (n[3] - n[1]).abs());
                    if w > 0.0 && h > 0.0 {
                        return Some((w, h));
                    }
                }
            }
        }
    }
    None
}

/// Resolution if the picture fills the page (same proportions within 2%,
/// either orientation).
fn page_dpi(w: u32, h: u32, page: Option<(f64, f64)>) -> Option<f64> {
    let (pw, ph) = page?;
    let (w, h) = (w as f64, h as f64);
    for (a, b) in [(pw, ph), (ph, pw)] {
        if ((w / h) / (a / b) - 1.0).abs() < 0.02 {
            let dpi = w * 72.0 / a;
            return Some((dpi * 100.0).round() / 100.0);
        }
    }
    None
}

/// The largest picture in a PDF file.
pub fn largest_picture(d: &[u8]) -> Result<Found, String> {
    if !d.starts_with(b"%PDF") {
        return Err("not a PDF file".into());
    }
    let all = streams(d);
    let mut best: Option<(u64, usize)> = None;
    for (k, (dict, _)) in all.iter().enumerate() {
        if dict.get("Subtype").and_then(Obj::name) != Some("Image") {
            continue;
        }
        let w = dict.get("Width").and_then(|v| resolve(d, v)).and_then(|v| v.num()).unwrap_or(0.0) as u64;
        let h = dict.get("Height").and_then(|v| resolve(d, v)).and_then(|v| v.num()).unwrap_or(0.0) as u64;
        if w * h > best.map_or(0, |b| b.0) {
            best = Some((w * h, k));
        }
    }
    let Some((_, k)) = best else {
        return Err("no picture in the PDF (a vector drawing cannot be converted)".into());
    };
    let (dict, raw) = &all[k];
    let get = |key: &str| dict.get(key).and_then(|v| resolve(d, v));
    let w = get("Width").and_then(|v| v.num()).ok_or("picture without width")? as u32;
    let h = get("Height").and_then(|v| v.num()).ok_or("picture without height")? as u32;
    if w == 0 || h == 0 || (w as u64) * (h as u64) > 400_000_000 {
        return Err(format!("unusable picture size {w}×{h}"));
    }
    let dpi = page_dpi(w, h, page_size(d, &all));
    let fl = filters(d, dict.get("Filter"));
    let mut data = raw.clone();
    for (i, f) in fl.iter().enumerate() {
        match f.as_str() {
            "FlateDecode" | "Fl" => {
                data = inflate(&data)?;
                if let Some(p) = first_parms(d, dict.get("DecodeParms").or(dict.get("DP"))) {
                    let pred = p.get("Predictor").and_then(Obj::num).unwrap_or(1.0) as u32;
                    if pred >= 10 {
                        let colors = p.get("Colors").and_then(Obj::num).unwrap_or(1.0) as usize;
                        let bpc = p.get("BitsPerComponent").and_then(Obj::num).unwrap_or(8.0) as usize;
                        let cols = p.get("Columns").and_then(Obj::num).unwrap_or(1.0) as usize;
                        data = unpredict(&data, colors, bpc, cols)?;
                    } else if pred == 2 {
                        return Err("TIFF predictor not supported".into());
                    }
                }
            }
            "DCTDecode" | "DCT" if i + 1 == fl.len() => return Ok(Found::Jpeg { data, w, h, dpi }),
            other => return Err(format!("compression {other} not supported")),
        }
    }
    let mask = matches!(get("ImageMask"), Some(Obj::Bool(true)));
    let bpc = if mask { 1 } else { get("BitsPerComponent").and_then(|v| v.num()).unwrap_or(8.0) as usize };
    if !matches!(bpc, 1 | 2 | 4 | 8) {
        return Err(format!("{bpc} bits per component not supported"));
    }
    let cs = if mask {
        Colours::Gray
    } else {
        match colours(d, dict.get("ColorSpace").or(dict.get("CS")), 0) {
            Some(c) => c,
            None => {
                // guess from the data size
                let per_row = |c: usize| (w as usize * c * bpc).div_ceil(8);
                match (1..=4).find(|&c| c != 2 && per_row(c) * h as usize == data.len()) {
                    Some(1) => Colours::Gray,
                    Some(3) => Colours::Rgb,
                    Some(4) => Colours::Cmyk,
                    _ => return Err("colour space not supported".into()),
                }
            }
        }
    };
    // /Decode [1 0] inverts (an image mask paints its 0 samples by default)
    let decode: Vec<f64> = match get("Decode").or(get("D")) {
        Some(Obj::Array(a)) => a.iter().filter_map(Obj::num).collect(),
        _ => Vec::new(),
    };
    let invert = if mask { decode.first() != Some(&1.0) } else { decode.len() >= 2 && decode[0] > decode[1] };
    let rgba = to_rgba(&data, w, h, bpc, &cs, invert)?;
    Ok(Found::Raw(Picture { w, h, rgba, dpi }))
}

fn to_rgba(data: &[u8], w: u32, h: u32, bpc: usize, cs: &Colours, invert: bool) -> Result<Vec<u8>, String> {
    let (w, h) = (w as usize, h as usize);
    let comps = cs.comps();
    let row = (w * comps * bpc).div_ceil(8);
    if data.len() < row * h {
        return Err(format!("picture data too short ({} of {} bytes)", data.len(), row * h));
    }
    let max = ((1u32 << bpc) - 1) as f64;
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let r = &data[y * row..(y + 1) * row];
        let sample = |i: usize| -> u32 {
            match bpc {
                8 => r[i] as u32,
                _ => {
                    let bit = i * bpc;
                    ((r[bit / 8] >> (8 - bpc - bit % 8)) as u32) & ((1 << bpc) - 1)
                }
            }
        };
        for x in 0..w {
            let px = match cs {
                Colours::Indexed { base, table } => {
                    let i = sample(x) as usize * base;
                    let c = |k: usize| table.get(i + k).copied().unwrap_or(0);
                    match base {
                        1 => [c(0), c(0), c(0)],
                        3 => [c(0), c(1), c(2)],
                        _ => cmyk([c(0), c(1), c(2), c(3)]),
                    }
                }
                _ => {
                    let mut v = [0u8; 4];
                    for (k, slot) in v.iter_mut().enumerate().take(comps) {
                        let s = sample(x * comps + k) as f64;
                        let s = if invert { max - s } else { s };
                        *slot = (s * 255.0 / max).round() as u8;
                    }
                    match cs {
                        Colours::Gray => [v[0], v[0], v[0]],
                        Colours::Rgb => [v[0], v[1], v[2]],
                        _ => cmyk(v),
                    }
                }
            };
            out.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    Ok(out)
}

fn cmyk(c: [u8; 4]) -> [u8; 3] {
    let k = 255 - c[3] as u32;
    [0, 1, 2].map(|i| ((255 - c[i] as u32) * k / 255) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflate_stored_and_fixed() {
        // zlib "hello hello hello" (fixed Huffman, with a back reference)
        let z = [0x78, 0xda, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x90, 0x00, 0x3a, 0x2e, 0x06, 0x7d];
        assert_eq!(inflate(&z).unwrap(), b"hello hello hello");
        // a stored block
        let s = [0x78, 0x01, 0x01, 0x03, 0x00, 0xfc, 0xff, 0x61, 0x62, 0x63, 0x02, 0x4d, 0x01, 0x27];
        assert_eq!(inflate(&s).unwrap(), b"abc");
    }

    #[test]
    fn reads_objects() {
        let d = b"<< /Type /XObject /Width 3 /Decode [1 0] /DP << /Predictor 15 >> /L 7 0 R /S (a\\(b\\)) /H <414> >>";
        let Some(Obj::Dict(m)) = (Lexer { d, i: 0 }).value(0) else { panic!() };
        assert_eq!(m["Width"], Obj::Num(3.0));
        assert_eq!(m["L"], Obj::Ref(7, 0));
        assert_eq!(m["S"], Obj::Str(b"a(b)".to_vec()));
        assert_eq!(m["H"], Obj::Str(vec![0x41, 0x40]));
        assert_eq!(m["Decode"], Obj::Array(vec![Obj::Num(1.0), Obj::Num(0.0)]));
    }

    #[test]
    fn png_predictor_up() {
        // 2 rows of 3 bytes, second row "Up"-filtered
        let d = [0, 1, 2, 3, 2, 1, 1, 1];
        assert_eq!(unpredict(&d, 1, 8, 3).unwrap(), vec![1, 2, 3, 2, 3, 4]);
    }

    #[test]
    fn one_bit_gray_picture() {
        // 10 x 2, 1 bit: first row all 0 (black), second all 1 (white);
        // 2 bytes per row, stored deflate block
        let raw = [0x00, 0x00, 0xff, 0xc0];
        let mut z = vec![0x78, 0x01, 0x01, 4, 0, !4u8, 0xff];
        z.extend_from_slice(&raw);
        z.extend_from_slice(&[0, 0, 0, 0]); // adler (not checked)
        let mut pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Page /MediaBox [0 0 100 20] >>\nendobj\n".to_vec();
        pdf.extend_from_slice(
            format!("2 0 obj\n<< /Type /XObject /Subtype /Image /Width 10 /Height 2 /BitsPerComponent 1 /ColorSpace /DeviceGray /Filter /FlateDecode /Length {} >>\nstream\n", z.len()).as_bytes(),
        );
        pdf.extend_from_slice(&z);
        pdf.extend_from_slice(b"\nendstream\nendobj\n%%EOF");
        let Found::Raw(p) = largest_picture(&pdf).unwrap() else { panic!("raw expected") };
        assert_eq!((p.w, p.h), (10, 2));
        assert_eq!(&p.rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&p.rgba[40..44], &[255, 255, 255, 255]);
        assert_eq!(p.dpi, Some(7.2)); // 10 px over 100 pt
        assert!(largest_picture(b"%PDF-1.4\n%%EOF").is_err());
    }
}
