# OpenixCLI

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)

A command-line firmware flashing tool for Allwinner chips, written in Rust.

## Overview

OpenixCLI is a powerful and user-friendly CLI tool designed for flashing firmware to devices powered by Allwinner SoCs. It supports both FEL (USB Boot) mode and FES (U-Boot) mode, providing a complete solution for firmware deployment.

## Features

- **Device Scanning**: Automatically detect connected Allwinner devices
- **Firmware Flashing**: Flash firmware images with multiple modes
- **FEL/FES Support**: Handles both FEL (USB Boot) and FES (U-Boot) device modes
- **Verification**: Optional write verification for data integrity
- **Progress Tracking**: Visual progress indicators during flash operations
- **Partition Selection**: Flash specific partitions or entire firmware
- **Verbose Logging**: Detailed debug output for troubleshooting

## Installation

### Prerequisites

- Rust toolchain (1.70 or later)
- libusb development libraries

### Build from Source

```bash
git clone https://github.com/YuzukiTsuru/OpenixCLI
cd OpenixCLI
cargo build --release
```

The compiled binary will be available at `target/release/openixcli`.

## Usage

### Scan for Devices

List all connected Allwinner devices:

```bash
openixcli scan
```

### Flash Firmware

Flash firmware to a device:

```bash
openixcli flash <firmware_file> [options]
```

#### Flash Options

| Option | Short | Description |
|--------|-------|-------------|
| `--bus` | `-b` | USB bus number |
| `--port` | `-P` | USB port number |
| `--verify` | `-V` | Enable verification after write (default: true) |
| `--mode` | `-m` | Flash mode: `partition`, `keep_data`, `partition_erase`, `full_erase` (default: full_erase) |
| `--partitions` | `-p` | Comma-separated list of partitions to flash |
| `--post-action` | `-a` | Post-flash action: `reboot`, `poweroff`, `shutdown` (default: reboot) |
| `--verbose` | `-v` | Enable verbose output |

#### Flash Examples

Flash firmware to a specific device:

```bash
openixcli flash firmware.img --bus 1 --port 5
```

Flash only specific partitions:

```bash
openixcli flash firmware.img --partitions "boot,system"
```

Flash with verification disabled:

```bash
openixcli flash firmware.img --verify false
```

Flash and power off after completion:

```bash
openixcli flash firmware.img --post-action poweroff
```

## Flash Modes

| Mode | Description |
|------|-------------|
| `partition` | Flash specific partitions only |
| `keep_data` | Flash while preserving user data |
| `partition_erase` | Erase and flash specific partitions |
| `full_erase` | Full erase before flashing (default) |

## Device Modes

OpenixCLI supports the following device modes:

- **FEL (USB Boot)**: Initial boot mode for firmware flashing
- **FES (U-Boot)**: Secondary mode after U-Boot is loaded
- **UPDATE_COOL/UPDATE_HOT**: Update modes

## Project Structure

```
OpenixCLI/
├── src/
│   ├── commands/      # CLI command implementations
│   ├── config/        # Configuration parsing (MBR, sys_config)
│   ├── firmware/      # Firmware image handling
│   ├── flash/         # Flashing logic (FEL/FES handlers)
│   ├── utils/         # Utilities (logging, errors)
│   ├── cli.rs         # CLI argument definitions
│   ├── lib.rs         # Library exports
│   └── main.rs        # Application entry point
├── Cargo.toml
└── LICENSE
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with [libefex](https://github.com/YuzukiTsuru/libefex) for Allwinner USB communication
- Inspired by the need for a modern, reliable firmware flashing tool for Allwinner devices
