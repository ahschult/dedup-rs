# dedup-rs 

**dedup-rs** is a high-performance, CLI-based duplicate file finder written in Rust. It leverages parallel processing to quickly scan directories, compute file hashes, and identify redundant data to help you reclaim disk space.

---

## Features

* **Parallel Processing:** Uses `rayon` to compute hashes across all available CPU cores simultaneously.
* **Multiple Hashing Algorithms:** Supports **BLAKE3** (optimized for speed) and **SHA-256** (industry standard).
* **Smart Filtering:** Filter files by minimum or maximum size to ignore small metadata or massive system files.
* **Safe Deletion:** Includes a `--dry-run` mode to preview deletions and a `--delete` flag for automated cleanup.
* **Visual Feedback:** Features real-time progress bars, color-coded terminal output, and human-readable file sizes.

---

## Installation

Ensure you have the [Rust toolchain](https://rustup.rs/) installed.

1.  **Clone the repository:**
    ```bash
    git clone [https://github.com/yourusername/dedup-rs.git](https://github.com/yourusername/dedup-rs.git)
    cd dedup-rs
    ```
2.  **Build the project:**
    ```bash
    cargo build --release
    ```
3.  **Run the binary:**
    The binary will be located at `./target/release/dedup-rs`.

---

## Usage

### Basic Scan
Scan the current directory and list all duplicates:
```bash
dedup-rs
