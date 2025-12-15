# One Billion Row Challenge - Rust Implementation 🦀

[![Rust](https://img.shields.io/badge/rust-1.94.0--nightly-orange.svg)](https://www.rust-lang.org/)
[![Performance](https://img.shields.io/badge/performance-878ms-brightgreen.svg)](https://github.com/gunnarmorling/1brc)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A high-performance Rust implementation of the [1 Billion Row Challenge](https://github.com/gunnarmorling/1brc), processing 1 billion temperature measurements in under a second.

## 🎯 Overview

The 1 Billion Row Challenge (1BRC) is a programming challenge focused on processing large datasets efficiently. The task is to read a text file containing 1 billion weather station measurements, calculate the minimum, mean, and maximum temperature for each station, and output the results sorted alphabetically.

**Input format:**
```
Hamburg;12.0
Bulawayo;8.9
Palembang;38.8
Hamburg;-5.3
...
```

**Output format:**
```
{Abha=-23.0/18.0/59.2, Abidjan=-16.2/26.0/67.3, ...}
```

## 📊 Benchmark Results

**Test Date:** December 15, 2025  
**Hardware:** Apple Silicon (M-series)  
**Rust Version:** 1.94.0-nightly  
**Dataset:** 1 billion rows

### Performance Metrics

| Benchmark | Mean Time | Standard Deviation | Confidence Interval |
|-----------|-----------|-------------------|---------------------|
| **worker_no_output** | **879.51 ms** | ±5.87 ms | [874.68 ms - 886.41 ms] |
| **worker_with_output** | **878.09 ms** | ±1.15 ms | [876.89 ms - 879.19 ms] |

### Key Findings

- **Processing Speed**: ~1.14 billion rows per second
- **Change from Previous**: -2.69% improvement in worker_no_output (no significant change detected)
- **Stability**: worker_with_output shows excellent stability with minimal variance
- **Outliers**: 2 high severe outliers detected in worker_no_output (20% of samples)

### Benchmark Configuration

- **Sample Size**: 10 iterations
- **Warm-up Time**: 2 seconds
- **Measurement Time**: 20 seconds
- **Backend**: Plotters (Gnuplot not found)

## 🚀 Features

- **Parallel Processing**: Utilizes multi-threading for optimal CPU usage
- **Memory Efficiency**: Optimized data structures and algorithms
- **Fast I/O**: Efficient file reading and parsing
- **Accurate Calculations**: Precise floating-point arithmetic for temperature statistics

## 🏗️ Implementation Details

### Architecture

The implementation uses a worker-based approach with two modes:
1. **worker_no_output**: Optimized for pure calculation speed (no output generation)
2. **worker_with_output**: Full processing including formatted output generation

### Optimizations

- Custom parsing for temperature values
- Efficient hash-based aggregation
- Minimal memory allocations
- Cache-friendly data structures

## 🔧 Building and Running

### Prerequisites

- Rust 1.94.0-nightly or later
- Cargo (comes with Rust)

### Build

```bash
cargo build --release
```

### Run

```bash
cargo run --release -- measurements.txt
```

### Benchmark

```bash
cargo bench
```

To benchmark with a custom file:
```bash
BRC_FILE=/path/to/your/file.txt cargo bench
```

## 📈 Results Comparison

The benchmark results show that this implementation achieves:
- **~878ms** average processing time for 1 billion rows
- **Consistent performance** across multiple runs
- **Minimal overhead** between calculation-only and output-generation modes

## 🔍 Technical Insights

### Performance Analysis

1. **I/O Bound vs CPU Bound**: The implementation achieves a good balance between I/O and computation
2. **Worker Efficiency**: Both worker modes show nearly identical performance, indicating efficient output generation
3. **Scalability**: Performance scales well with available CPU cores

### Optimization Opportunities

- Further tuning of thread pool size
- SIMD optimizations for parsing
- Custom memory allocator
- Profile-guided optimization (PGO)

## 📝 License

MIT License - see LICENSE file for details

## 🙏 Acknowledgments

- Original challenge by [Gunnar Morling](https://github.com/gunnarmorling)
- Inspired by various community implementations

## 🤝 Contributing

Contributions are welcome! Feel free to submit issues or pull requests.

---

**Note**: Benchmark results may vary based on hardware, operating system, and system load. The results shown here were obtained on Apple Silicon hardware.
