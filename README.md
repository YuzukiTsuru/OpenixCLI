# OpenixCLI

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)

A command-line & tui firmware flashing tool for Allwinner chips, written in Rust.

<img width="1090" height="583" alt="4610154f-10af-4016-afdd-cf5bdebb59e2" src="https://github.com/user-attachments/assets/39ae92c1-2ff7-45bf-95f8-361327f44ae6" />

## Overview

OpenixCLI is a powerful and user-friendly CLI tool designed for flashing firmware to devices powered by Allwinner SoCs. It supports both FEL (USB Boot) mode and FES (U-Boot) mode, providing a complete solution for firmware deployment.

## Features

- **Device Scanning**: Automatically detect connected Allwinner devices
- **Firmware Flashing**: Flash firmware images with multiple modes
- **FEL/FES Support**: Handles both FEL (USB Boot) and FES (U-Boot) device modes
- **Verification**: Optional write verification for data integrity
- **Progress Tracking**: Visual progress indicators during flash operations
- **Partition Selection**: Flash specific partitions or entire firmware
- **Raw Image Flashing**: Flash standalone disk images through a virtual Sunxi MBR
- **Firmware Conversion**: Convert IMAGEWTY packages to raw SPI NOR, SD card, eMMC, SD NAND, or UFS programmer images
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

Launch the interactive TUI:

```bash
openixcli
```

You can also launch it explicitly:

```bash
openixcli tui
```

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

### Flash a Raw Image

The `raw` command uses a private IMAGEWTY firmware package as the boot loader to flash a standard
firmware image directly into the storage, block by block — analogous to a `dd` operation. The boot
firmware supplies FES, U-Boot, Boot0/Boot1, and related board configuration; its normal partition
images are not written. OpenixCLI creates a temporary Sunxi MBR containing one `raw` partition and
streams the whole image into the target storage.

> **Warning:** The `raw` command always performs a full erase, disables download verification, and
> reboots the board after flashing. All existing data on the target storage will be destroyed.

```bash
openixcli raw <boot_firmware> <raw_image> [options]
```

The default mode is `logic-offset` for SD/eMMC with a 40960-sector offset:

```bash
openixcli raw boot.img disk.img
openixcli raw boot.img disk.img --mode logic-offset --storage ufs
```

Use `command` mode to place the virtual raw partition at sector 0:

```bash
openixcli raw boot.img disk.img --mode command
```

Override the logical offset or select the NOR default (2106 sectors):

```bash
openixcli raw boot.img disk.img --logic-offset 8192
openixcli raw boot.img disk.img --storage nor
```

#### Raw Options

| Option | Short | Description |
|--------|-------|-------------|
| `--mode` | | `command` or `logic-offset` (default: `logic-offset`) |
| `--storage` | | `sdmmc`, `ufs`, or `nor` (default: `sdmmc`) |
| `--logic-offset` | | Override the logical offset in 512-byte sectors; valid range: 0 through `0x100000000` |
| `--bus` | `-b` | USB bus number |
| `--port` | `-P` | USB port number |
| `--verbose` | `-v` | Enable verbose output |

In `logic-offset` mode, the virtual partition starts at `0x100000000 - logic_offset`. The MBR copy
count is inherited from the boot firmware when its MBR is valid, otherwise it defaults to one. The
default logical offset is 40960 sectors for `sdmmc` and `ufs`, and 2106 sectors for `nor`.

### Convert Firmware

Convert an IMAGEWTY firmware package to a raw programmer image. The command reads the embedded
partition configuration, expands Android sparse partitions, detects secure firmware, and uses the
DTB flash map when available.

```bash
openixcli convert firmware.img
```

The default target is eMMC and the default output is `firmware_programmer.bin`. Select another
storage layout or output path with `--target` and `--output`:

```bash
openixcli convert firmware.img --target ufs --output firmware-ufs.img
openixcli convert firmware.img --target spinor --nor-size 32
```

#### Convert Options

| Option | Description |
|--------|-------------|
| `--output`, `-o` | Output path (defaults to `*_programmer.bin`, or `*_full_img.bin` for SPI NOR) |
| `--target`, `-t` | `spinor`, `sdcard`, `emmc`, `sdnand`, or `ufs` (default: `emmc`) |
| `--logic-offset` | Override the DTB/default logical offset, in 512-byte sectors |
| `--uboot-start` | Override the SPI NOR U-Boot offset, in 512-byte sectors |
| `--nor-size` | SPI NOR capacity in MiB (default: 16) |
| `--storage-size` | GPT target size such as `8GB` or `512MB` (default: `auto`) |
| `--secure` | `auto`, `enabled`, or `disabled` (default: `auto`) |

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
│   ├── process/       # Stage and progress tracking
│   ├── raw/           # Virtual MBR and standalone raw-image flashing
│   ├── tui/           # Interactive terminal UI
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
