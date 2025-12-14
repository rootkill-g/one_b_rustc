use std::cmp::Ordering as CmpOrd;
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

unsafe extern "C" {
    fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: isize,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, length: usize) -> i32;
    fn madvise(addr: *mut c_void, length: usize, advice: i32) -> i32;
}

const PROT_READ: i32 = 0x1;
const MAP_PRIVATE: i32 = 0x2;
const MAP_FAILED: *mut c_void = !0 as *mut c_void;
const MADV_SEQUENTIAL: i32 = 2;

const SEGMENT_SIZE: usize = 1 << 21;
const HASH_TABLE_SIZE: usize = 1 << 17;
const MERGE_TABLE_SIZE: usize = 1 << 17;
const PROBE_STEP: usize = 1;

const MASK1: [u64; 9] = [
    0x0000_0000_0000_00FF,
    0x0000_0000_0000_FFFF,
    0x0000_0000_00FF_FFFF,
    0x0000_0000_FFFF_FFFF,
    0x0000_00FF_FFFF_FFFF,
    0x0000_FFFF_FFFF_FFFF,
    0x00FF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
];

const MASK2: [u64; 9] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0xFFFF_FFFF_FFFF_FFFF,
];

#[derive(Debug)]
struct MappedFile {
    ptr: *const u8,
    len: usize,
}

impl MappedFile {
    fn open(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len() as usize;
        if len == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty file"));
        }
        let ptr = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                PROT_READ,
                MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        unsafe {
            let _ = madvise(ptr, len, MADV_SEQUENTIAL);
        }
        Ok(Self {
            ptr: ptr as *const u8,
            len,
        })
    }

    #[inline(always)]
    fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.len
    }
}

impl Drop for MappedFile {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.len > 0 {
            unsafe {
                munmap(self.ptr as *mut c_void, self.len);
            }
        }
    }
}

#[derive(Clone, Copy)]
#[derive(Default)]
struct Entry {
    first_word: u64,
    second_word: u64,
    sum: i64,
    name_off: u32,
    count: u32,
    min: i16,
    max: i16,
    name_len: u16, // 0 == empty slot
}


struct Scanner {
    pos: usize,
    end: usize,
    base: *const u8,
}

impl Scanner {
    #[inline(always)]
    fn new(base: *const u8, start: usize, end: usize) -> Self {
        Self { pos: start, end, base }
    }

    #[inline(always)]
    fn has_next(&self) -> bool {
        self.pos < self.end
    }

    #[inline(always)]
    fn add(&mut self, delta: usize) {
        self.pos += delta;
    }

    #[inline(always)]
    unsafe fn get_u64(&self) -> u64 {
        unsafe { ptr::read_unaligned(self.base.add(self.pos) as *const u64) }
    }

    #[inline(always)]
    unsafe fn get_u64_at(&self, pos: usize) -> u64 {
        unsafe { ptr::read_unaligned(self.base.add(pos) as *const u64) }
    }
}

#[inline(always)]
fn find_delim(word: u64, byte: u8) -> u64 {
    let input = word ^ u64::from_ne_bytes([byte; 8]);
    (input.wrapping_sub(0x0101_0101_0101_0101) & !input) & 0x8080_8080_8080_8080
}

#[inline(always)]
fn next_newline(mut pos: usize, file_end: usize, base: *const u8) -> usize {
    while pos + 8 <= file_end {
        let word = unsafe { ptr::read_unaligned(base.add(pos) as *const u64) };
        let m = find_delim(word, b'\n');
        if m != 0 {
            pos += (m.trailing_zeros() as usize) >> 3;
            return pos;
        }
        pos += 8;
    }
    while pos < file_end {
        let b = unsafe { *base.add(pos) };
        if b == b'\n' {
            return pos;
        }
        pos += 1;
    }
    file_end
}

#[inline(always)]
fn hash_to_index_sized(hash: u64, mask: usize) -> usize {
    let h = hash ^ (hash >> 33) ^ (hash >> 15);
    (h as usize) & mask
}

#[inline(always)]
fn convert_into_number(decimal_sep_pos: u32, number_word: u64) -> i32 {
    let shift = 28i32 - decimal_sep_pos as i32;
    let signed = ((!number_word) << 59) as i64 >> 63 ;
    let design_mask = !((signed as u64) & 0xFF);
    let digits = ((number_word & design_mask) << shift) & 0x000F_000F_0F00_u64 ;
    let abs_value = (((digits.wrapping_mul(0x640A_0001)) >> 32) & 0x3FF) as i32;
    let s = signed as i32;
    (abs_value ^ s) - s
}

#[inline(always)]
fn scan_number(scanner: &mut Scanner) -> i32 {
    let number_word = unsafe { scanner.get_u64_at(scanner.pos + 1) };
    let decimal_sep_pos = (!(number_word) & 0x1010_1000u64).trailing_zeros() ;
    let number = convert_into_number(decimal_sep_pos, number_word);
    scanner.add(((decimal_sep_pos as usize) >> 3) + 4);
    number
}

#[inline(always)]
unsafe fn name_eq(base: *const u8, off_a: usize, off_b: usize, len_plus1: usize) -> bool {
    let mut i = 0usize;
    while i + 8 <= len_plus1 {
        let wa = unsafe { ptr::read_unaligned(base.add(off_a + i) as *const u64) };
        let wb = unsafe { ptr::read_unaligned(base.add(off_b + i) as *const u64) };
        if wa != wb {
            return false;
        }
        i += 8;
    }
    if i == len_plus1 {
        return true;
    }
    let wa = unsafe { ptr::read_unaligned(base.add(off_a + i) as *const u64) };
    let wb = unsafe { ptr::read_unaligned(base.add(off_b + i) as *const u64) };
    let remaining = len_plus1 - i;
    let shift = 64 - (remaining << 3);
    ((wa ^ wb) << shift) == 0
}

#[inline(always)]
fn record(entry: &mut Entry, number: i32) {
    let num = number as i16;
    if entry.count == 0 {
        entry.min = num;
        entry.max = num;
        entry.sum = number as i64;
        entry.count = 1;
        return;
    }
    if num < entry.min {
        entry.min = num;
    }
    if num > entry.max {
        entry.max = num;
    }
    entry.sum += number as i64;
    entry.count += 1;
}

#[inline(always)]
unsafe fn hash_from_parts(
    base: *const u8,
    off: usize,
    name_len: usize,
    first: u64,
    second: u64,
) -> u64 {
    let total_len = name_len + 1; // include ';'
    let mut hash = first ^ second;
    if total_len <= 16 {
        return hash;
    }

    let end = off + total_len;
    let rem = end & 7;
    let last_start = if rem == 0 { end - 8 } else { end - rem };

    let mut p = off + 16;
    while p < last_start {
        let w = unsafe { ptr::read_unaligned(base.add(p) as *const u64) };
        hash ^= w;
        p += 8;
    }

    let delim_off = off + name_len;
    let k = delim_off - last_start; // 0..7
    let shift = 56usize.wrapping_sub(k << 3);
    let last = unsafe { ptr::read_unaligned(base.add(last_start) as *const u64) };
    hash ^ (last << shift)
}

#[derive(Clone, Copy)]
struct Agg {
    hash: u64,
    first_word: u64,
    second_word: u64,
    name_off: u32,
    name_len: u16,
    min: i16,
    max: i16,
    sum: i64,
    count: u32,
}

#[inline(always)]
fn new_entry(scanner: &Scanner, table: &mut [Entry], used: &mut Vec<u32>, idx: usize, name_addr: usize, name_len: usize) -> usize {
    let total_len = name_len + 1;
    let mut first = unsafe { scanner.get_u64_at(name_addr) };
    let mut second = unsafe { scanner.get_u64_at(name_addr + 8) };
    if total_len <= 8 {
        first &= MASK1[total_len - 1];
        second = 0;
    } else if total_len < 16 {
        second &= MASK1[total_len - 9];
    }

    let entry = unsafe { table.get_unchecked_mut(idx) };
    entry.first_word = first;
    entry.second_word = second;
    entry.name_off = name_addr as u32;
    entry.name_len = name_len as u16;
    entry.sum = 0;
    entry.count = 0;
    entry.min = 0;
    entry.max = 0;
    used.push(idx as u32);
    idx
}

#[inline(always)]
fn find_result(
    mut word: u64,
    delim_mask: u64,
    word_b: u64,
    delim_mask_b: u64,
    scanner: &mut Scanner,
    table: &mut [Entry],
    used: &mut Vec<u32>,
) -> usize {
    let name_addr = scanner.pos;

    let mut hash: u64;
    let mut first_word: u64 = 0;
    let mut second_word: u64 = 0;
    let mut fast16 = false;

    if (delim_mask | delim_mask_b) != 0 {
        let lc1 = (delim_mask.trailing_zeros() as usize) >> 3;
        let lc2 = (delim_mask_b.trailing_zeros() as usize) >> 3;
        let mask = MASK2[lc1];
        word &= MASK1[lc1];
        let w2 = word_b & MASK1[lc2];
        second_word = w2 & mask;
        first_word = word;
        hash = first_word ^ second_word;
        scanner.add(lc1 + (((lc2 as u64) & mask) as usize));
        fast16 = true;
    } else {
        hash = word ^ word_b;
        scanner.add(16);
        loop {
            word = unsafe { scanner.get_u64() };
            let m = find_delim(word, b';');
            if m != 0 {
                let tz = m.trailing_zeros() as usize;
                word = word.wrapping_shl((63 - tz) as u32);
                scanner.add(tz >> 3);
                hash ^= word;
                break;
            } else {
                scanner.add(8);
                hash ^= word;
            }
        }
    }

    let name_len = scanner.pos - name_addr;
    let total_len = name_len + 1;
    let mut idx = hash_to_index_sized(hash, HASH_TABLE_SIZE - 1);

    loop {
        let entry = unsafe { table.get_unchecked_mut(idx) };
        if entry.name_len == 0 {
            return new_entry(scanner, table, used, idx, name_addr, name_len);
        }

        if fast16 {
            if entry.first_word == first_word && entry.second_word == second_word {
                return idx;
            }
        } else if entry.name_len as usize == name_len
            && unsafe { name_eq(scanner.base, entry.name_off as usize, name_addr, total_len) }
        {
            return idx;
        }

        idx = (idx + PROBE_STEP) & (HASH_TABLE_SIZE - 1);
    }
}

#[inline(always)]
fn parse_segment(base: *const u8, seg_start: usize, seg_end_nl: usize, table: &mut [Entry], used: &mut Vec<u32>) {
    if seg_start >= seg_end_nl {
        return;
    }

    let dist = (seg_end_nl - seg_start) / 3;
    let mid1 = next_newline(seg_start + dist, seg_end_nl, base);
    let mid2 = next_newline(seg_start + dist + dist, seg_end_nl, base);

    let mut s1 = Scanner::new(base, seg_start, mid1);
    let mut s2 = Scanner::new(base, mid1 + 1, mid2);
    let mut s3 = Scanner::new(base, mid2 + 1, seg_end_nl);

    while s1.has_next() && s2.has_next() && s3.has_next() {
        unsafe {
            let w1 = s1.get_u64();
            let w2 = s2.get_u64();
            let w3 = s3.get_u64();
            let d1 = find_delim(w1, b';');
            let d2 = find_delim(w2, b';');
            let d3 = find_delim(w3, b';');
            let w1b = s1.get_u64_at(s1.pos + 8);
            let w2b = s2.get_u64_at(s2.pos + 8);
            let w3b = s3.get_u64_at(s3.pos + 8);
            let d1b = find_delim(w1b, b';');
            let d2b = find_delim(w2b, b';');
            let d3b = find_delim(w3b, b';');

            let i1 = find_result(w1, d1, w1b, d1b, &mut s1, table, used);
            let i2 = find_result(w2, d2, w2b, d2b, &mut s2, table, used);
            let i3 = find_result(w3, d3, w3b, d3b, &mut s3, table, used);

            let n1 = scan_number(&mut s1);
            let n2 = scan_number(&mut s2);
            let n3 = scan_number(&mut s3);

            record(table.get_unchecked_mut(i1), n1);
            record(table.get_unchecked_mut(i2), n2);
            record(table.get_unchecked_mut(i3), n3);
        }
    }

    while s1.has_next() {
        unsafe {
            let w = s1.get_u64();
            let d = find_delim(w, b';');
            let wb = s1.get_u64_at(s1.pos + 8);
            let db = find_delim(wb, b';');
            let idx = find_result(w, d, wb, db, &mut s1, table, used);
            let num = scan_number(&mut s1);
            record(table.get_unchecked_mut(idx), num);
        }
    }
    while s2.has_next() {
        unsafe {
            let w = s2.get_u64();
            let d = find_delim(w, b';');
            let wb = s2.get_u64_at(s2.pos + 8);
            let db = find_delim(wb, b';');
            let idx = find_result(w, d, wb, db, &mut s2, table, used);
            let num = scan_number(&mut s2);
            record(table.get_unchecked_mut(idx), num);
        }
    }
    while s3.has_next() {
        unsafe {
            let w = s3.get_u64();
            let d = find_delim(w, b';');
            let wb = s3.get_u64_at(s3.pos + 8);
            let db = find_delim(wb, b';');
            let idx = find_result(w, d, wb, db, &mut s3, table, used);
            let num = scan_number(&mut s3);
            record(table.get_unchecked_mut(idx), num);
        }
    }
}

fn cmp_name_bytes(base: *const u8, a: &Agg, b: &Agg) -> CmpOrd {
    let ap = unsafe { std::slice::from_raw_parts(base.add(a.name_off as usize), a.name_len as usize) };
    let bp = unsafe { std::slice::from_raw_parts(base.add(b.name_off as usize), b.name_len as usize) };
    ap.cmp(bp)
}

#[inline(always)]
fn push_u64(mut n: u64, out: &mut Vec<u8>) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    if n == 0 {
        out.push(b'0');
        return;
    }
    while n != 0 {
        let q = n / 10;
        let r = (n - q * 10) as u8;
        i -= 1;
        tmp[i] = b'0' + r;
        n = q;
    }
    out.extend_from_slice(&tmp[i..]);
}

#[inline(always)]
fn push_tenths_i64(val: i64, out: &mut Vec<u8>) {
    let negative = val < 0;
    let mut abs = if negative { -val } else { val } as u64;
    let frac = (abs % 10) as u8;
    abs /= 10;
    if negative {
        out.push(b'-');
    }
    push_u64(abs, out);
    out.push(b'.');
    out.push(b'0' + frac);
}

#[inline(always)]
fn push_tenths_i16(val: i16, out: &mut Vec<u8>) {
    push_tenths_i64(val as i64, out);
}

#[derive(Debug)]
pub struct WorkerResult {
    pub checksum: u64,
    pub stations: usize,
    pub output: Option<Vec<u8>>,
}

pub fn run_worker(path: &str, no_output: bool) -> io::Result<WorkerResult> {
    let mapped = MappedFile::open(path)?;
    let base = mapped.as_ptr() as usize;
    let file_end = mapped.len();

    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);

    let cursor = AtomicUsize::new(0);

    let thread_results = thread::scope(|scope| {
        let cursor = &cursor;
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            handles.push(scope.spawn(move || {
                let base_ptr = base as *const u8;
                let mut table = vec![Entry::default(); HASH_TABLE_SIZE];
                let mut used: Vec<u32> = Vec::with_capacity(10_000);

                loop {
                    let current = cursor.fetch_add(SEGMENT_SIZE, Ordering::Relaxed);
                    if current >= file_end {
                        break;
                    }

                    let seg_end_search = (current + SEGMENT_SIZE).min(file_end.saturating_sub(1));
                    let seg_end_nl = next_newline(seg_end_search, file_end, base_ptr);

                    let seg_start = if current == 0 {
                        0
                    } else {
                        let nl = next_newline(current, file_end, base_ptr);
                        (nl + 1).min(file_end)
                    };

                    parse_segment(base_ptr, seg_start, seg_end_nl, &mut table, &mut used);
                }

                let mut out = Vec::with_capacity(used.len());
                for &idx in &used {
                    let e = unsafe { table.get_unchecked(idx as usize) };
                    if e.count == 0 {
                        continue;
                    }
                    let hash = unsafe {
                        hash_from_parts(
                            base_ptr,
                            e.name_off as usize,
                            e.name_len as usize,
                            e.first_word,
                            e.second_word,
                        )
                    };
                    out.push(Agg {
                        hash,
                        first_word: e.first_word,
                        second_word: e.second_word,
                        name_off: e.name_off,
                        name_len: e.name_len,
                        min: e.min,
                        max: e.max,
                        sum: e.sum,
                        count: e.count,
                    });
                }
                out
            }));
        }

        let mut out = Vec::with_capacity(handles.len());
        for h in handles {
            out.push(h.join().expect("worker panicked"));
        }
        out
    });

    // Merge into a fixed-size table.
    let mut merge_table = vec![Entry::default(); MERGE_TABLE_SIZE];
    let mut merge_used: Vec<u32> = Vec::with_capacity(12_000);
    let merge_mask = MERGE_TABLE_SIZE - 1;
    let base_ptr = base as *const u8;

    for vec in &thread_results {
        for agg in vec {
            let mut idx = hash_to_index_sized(agg.hash, merge_mask);
            loop {
                let entry = unsafe { merge_table.get_unchecked_mut(idx) };
                if entry.name_len == 0 {
                    entry.first_word = agg.first_word;
                    entry.second_word = agg.second_word;
                    entry.name_off = agg.name_off;
                    entry.name_len = agg.name_len;
                    entry.min = agg.min;
                    entry.max = agg.max;
                    entry.sum = agg.sum;
                    entry.count = agg.count;
                    merge_used.push(idx as u32);
                    break;
                }

                if entry.name_len == agg.name_len
                    && entry.first_word == agg.first_word
                    && entry.second_word == agg.second_word
                {
                    let name_len = agg.name_len as usize;
                    let ok = if name_len < 16 {
                        true
                    } else {
                        unsafe {
                            name_eq(
                                base_ptr,
                                entry.name_off as usize,
                                agg.name_off as usize,
                                name_len + 1,
                            )
                        }
                    };
                    if ok {
                        if agg.min < entry.min {
                            entry.min = agg.min;
                        }
                        if agg.max > entry.max {
                            entry.max = agg.max;
                        }
                        entry.sum += agg.sum;
                        entry.count += agg.count;
                        break;
                    }
                }

                idx = (idx + PROBE_STEP) & merge_mask;
            }
        }
    }

    let stations = merge_used.len();
    let mut checksum: u64 = 0;

    if no_output {
        for idx in &merge_used {
            let e = unsafe { merge_table.get_unchecked(*idx as usize) };
            checksum ^= (e.sum as u64)
                .wrapping_mul(0x9E37_79B1_85EB_CA87)
                .rotate_left((e.count & 63) as u32);
            checksum ^= (e.min as u64).wrapping_shl(16) ^ (e.max as u64).wrapping_shl(32);
        }
        return Ok(WorkerResult {
            checksum,
            stations,
            output: None,
        });
    }

    let mut merged: Vec<Agg> = Vec::with_capacity(merge_used.len());
    for idx in merge_used {
        let e = unsafe { merge_table.get_unchecked(idx as usize) };
        if e.name_len == 0 {
            continue;
        }
        merged.push(Agg {
            hash: 0,
            first_word: 0,
            second_word: 0,
            name_off: e.name_off,
            name_len: e.name_len,
            min: e.min,
            max: e.max,
            sum: e.sum,
            count: e.count,
        });
    }

    merged.sort_by(|a, b| cmp_name_bytes(base_ptr, a, b));

    let mut output: Vec<u8> = Vec::with_capacity(merged.len() * 40);
    output.push(b'{');
    for (i, a) in merged.iter().enumerate() {
        if i != 0 {
            output.extend_from_slice(b", ");
        }
        let name_bytes =
            unsafe { std::slice::from_raw_parts(base_ptr.add(a.name_off as usize), a.name_len as usize) };
        let sum = a.sum;
        let count = a.count as i64;
        let half = count / 2;
        let avg_tenths = if sum >= 0 { (sum + half) / count } else { (sum - half) / count };

        output.extend_from_slice(name_bytes);
        output.push(b'=');
        push_tenths_i16(a.min, &mut output);
        output.push(b'/');
        push_tenths_i64(avg_tenths, &mut output);
        output.push(b'/');
        push_tenths_i16(a.max, &mut output);

        checksum ^= (a.sum as u64)
            .wrapping_mul(0x9E37_79B1_85EB_CA87)
            .rotate_left(a.count & 63  );
    }
    output.push(b'}');

    Ok(WorkerResult {
        checksum,
        stations,
        output: Some(output),
    })
}
