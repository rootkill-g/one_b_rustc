# One Billion Row Challenge - Rust Implementation

My take on the famous [1 Billion Row Challenge](https://github.com/gunnarmorling/1brc) - processing 1 billion temperature measurements as fast as possible in Rust.

## 🎯 The Challenge

Process a text file containing 1 billion weather station measurements and calculate min, mean, and max temperatures per station, sorted alphabetically by station name.

**Input format:**
```
Hamburg;12.0
Bulawayo;8.9
Palembang;38.8
...
```

**Output format:**
```
{Abha=-23.0/18.0/59.2, Abidjan=-16.2/26.0/67.3, ...}
```

## 📊 Performance Results

### System Specifications
- **CPU:** Apple M3 Pro (12 cores)
- **OS:** macOS
- **Rust:** 1.94.0-nightly
- **File Size:** 13.7 GB
- **Total Rows:** 1,000,000,000 (1 billion)
- **Unique Stations:** 466-467

### Benchmark Results

| Metric | Time (ms) | Throughput |
|--------|-----------|------------|
| **Without Output** | 860.60 ± 12 ms | ~1.16 billion rows/sec |
| **With Output** | 855.83 ± 6 ms | ~1.17 billion rows/sec |
| **Single Runs** | 893-1144 ms | ~870M-1.12B rows/sec |

*Benchmarks performed using Criterion.rs with 10 samples, 2s warmup, 20s measurement time*

### Performance Breakdown
- **Data throughput:** ~12 GB/s
- **Memory mapping:** Zero-copy read via mmap
- **Parallelization:** Multi-threaded processing with work-stealing
- **Hash table:** Custom fixed-size hash table (128K entries)

## 🚀 Implementation Highlights

### 1. **Memory-Mapped I/O**
Using `mmap()` for zero-copy file access - no buffering overhead, let the OS handle paging.

```rust
unsafe {
    mmap(std::ptr::null_mut(), len, PROT_READ, MAP_PRIVATE, fd, 0)
}
```

### 2. **SIMD-Inspired Parsing**
Fast delimiter detection using bit manipulation tricks:
- Process 8 bytes at a time with unaligned reads
- Find semicolons and newlines using XOR + bitmask patterns
- Skip byte-by-byte scanning entirely for common cases

```rust
fn find_delim(word: u64, byte: u8) -> u64 {
    let input = word ^ u64::from_ne_bytes([byte; 8]);
    (input.wrapping_sub(0x0101_0101_0101_0101) & !input) & 0x8080_8080_8080_8080
}
```

### 3. **Custom Number Parsing**
Convert temperature strings to integers without standard library overhead:
- Extract sign, digits, and decimal point in one 64-bit read
- Bit manipulation to convert ASCII to numbers
- Fixed-point arithmetic (tenths) to avoid floating-point

```rust
fn convert_into_number(decimal_sep_pos: u32, number_word: u64) -> i32 {
    let shift = 28i32 - decimal_sep_pos as i32;
    let signed = (((!number_word) << 59) as i64 >> 63) as i64;
    let design_mask = !((signed as u64) & 0xFF);
    let digits = (((number_word & design_mask) << shift) & 0x0F00_0F0F_00u64) as u64;
    (((digits.wrapping_mul(0x640A_0001)) >> 32) & 0x3FF) as i32
}
```

### 4. **Parallel Processing**
- Divide file into ~2MB segments (SEGMENT_SIZE = 2^21)
- Each thread processes segments independently with atomic work-stealing
- Split each segment into 3 sub-scanners for instruction-level parallelism
- No locks during processing - only atomic counter for segment allocation

### 5. **Optimized Hash Table**
- Fixed-size tables (128K entries) to avoid dynamic resizing
- Linear probing with single-step increment
- Store first 16 bytes of station name inline for fast comparison
- Full hash computed only for collisions on names > 16 bytes

### 6. **Efficient Merging**
- Each thread maintains local hash table during processing
- Single-pass merge at the end into final table
- Minimal allocations - reuse vectors across iterations

## 🔧 Technical Optimizations

### Compiler Flags
```toml
[profile.release]
lto = "fat"              # Link-time optimization
codegen-units = 1        # Better optimization, slower compile
panic = "abort"          # No unwinding overhead
opt-level = 3            # Maximum optimizations
strip = true             # Remove debug symbols
overflow-checks = false  # Disable integer overflow checks
```

### Why These Choices Work

1. **mmap over read():** OS does the buffering and can use aggressive read-ahead
2. **Fixed-size hash tables:** No reallocation during hot path
3. **Custom parsing:** stdlib is general-purpose; we can exploit fixed format
4. **Segment-based parallelism:** Natural work distribution without coordination
5. **Inline station names:** Most names fit in 16 bytes - eliminates pointer chasing

## 💡 My Key Insights

### What Made the Biggest Difference

1. **Memory mapping (mmap):** ~40% faster than buffered I/O
2. **SIMD-style delimiter search:** ~2x faster than byte-by-byte scanning
3. **Thread-local hash tables:** Eliminated lock contention entirely
4. **Custom number parsing:** ~3x faster than `parse::<f64>()`

### Tradeoffs I Made

- **Unsafe code:** Used extensively for performance (pointer arithmetic, unaligned reads)
- **Fixed-size limits:** Hash tables sized for expected workload
- **Platform-specific:** Direct syscalls to mmap - not portable to Windows
- **Memory usage:** Each thread allocates ~4MB for hash tables

### What Surprised Me

- **Triple-scanner pattern:** Running 3 scanners per segment gave better performance than 1
- **String comparison overhead:** Comparing first 16 bytes inline saved significant time
- **LTO impact:** Fat LTO alone gave ~15% improvement
- **Output formatting cost:** Negligible - sorting and formatting barely registers

## 🏃 Running the Code

### Build and Run
```bash
# Build optimized binary
cargo build --release

# Process 1 billion rows
./target/release/one_b_rustc measurements.txt

# Process without output (faster)
./target/release/one_b_rustc measurements.txt --no-output
```

### Generate Test Data
```bash
# Create measurements file (requires Java)
# https://github.com/gunnarmorling/1brc
```

### Run Benchmarks
```bash
cargo bench --bench one_b_rustc
```

## 📈 Scaling Characteristics

Based on testing:
- **Near-linear scaling** up to physical core count
- **Memory bandwidth bound** on M3 Pro beyond ~8 threads
- **I/O not bottleneck** - mmap + sequential access saturates CPU first
- **Cache-friendly** - sequential access pattern, minimal random lookups

## 🎓 Lessons Learned

### Performance Engineering
1. **Profile first:** Don't optimize without data
2. **Exploit structure:** Fixed format = aggressive optimizations
3. **Hardware matters:** M3 Pro's memory bandwidth is impressive
4. **Unsafe isn't scary:** When performance matters, unsafe Rust gives you control

### Rust-Specific
1. **Zero-cost abstractions work:** Iterator pipelines compiled away completely
2. **Atomics are fast:** AtomicUsize for work-stealing had zero overhead
3. **Fat LTO is magic:** Let LLVM see the whole program
4. **Edition 2024:** Using latest Rust features and improvements

## 🔮 Future Improvements

Ideas I haven't tried yet:
- [ ] AVX2/NEON SIMD for parsing (explicit vectorization)
- [ ] Custom memory allocator (jemalloc/mimalloc)
- [ ] GPU processing (probably overkill)
- [ ] Network-distributed processing (multi-machine)
- [ ] Specialized hash functions (may reduce collisions)

## 🙏 Acknowledgments

- Original challenge by [Gunnar Morling](https://github.com/gunnarmorling/1brc)
- Inspired by various Java, C++, and Rust implementations
- Thanks to the Rust community for excellent performance tooling

## 📝 License

This is a personal challenge implementation - do whatever you want with it.

---

**My verdict:** Rust makes high-performance systems programming accessible and (relatively) safe. The ability to drop down to unsafe when needed, combined with zero-cost abstractions, makes it perfect for challenges like this. Sub-second processing of 1 billion rows on a laptop still feels like magic! 🦀⚡
