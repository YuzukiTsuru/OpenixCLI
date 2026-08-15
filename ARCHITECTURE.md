# OpenixCLI Architecture

## Runtime Flow

OpenixCLI has two frontends: the CLI commands and the interactive TUI. Both frontends build the same `FlashRequest` and pass it to `Flasher`, so device selection, flash mode, verification, partition filtering, and post-flash action use one shared model. Specialized commands can attach a `CustomFlashLayout` containing replacement MBR bytes and standalone partition files without changing the normal IMAGEWTY path.

The flash flow is:

1. Load firmware with `LoadedFirmware`, which wraps `OpenixPacker` plus image metadata and MBR partition names for UI display. The `raw` command instead loads an IMAGEWTY boot firmware and prepares an external raw-image layout.
2. Open the selected USB device, or the first detected device when no full bus/port selector is provided.
3. Detect device mode.
4. In FEL mode, initialize DRAM, download U-Boot with configuration blobs, then reconnect in FES mode.
5. In FES mode, query boot/storage details, optionally send the erase flag, select the firmware MBR or a custom MBR, plan partition downloads, write MBR, write partitions, and write Boot0/Boot1.
6. Set the requested post-flash device mode.

## Module Responsibilities

- `commands`: CLI command adapters. They parse input and delegate flashing, raw-image flashing, inspection, unpacking, and image conversion to their domain modules.
- `convert`: Raw programmer-image assembly for SPI NOR and block storage, including DTB offset detection, sparse expansion, and GPT adjustment.
- `raw`: Standalone raw-image flashing setup. It validates inputs, creates the single-partition virtual Sunxi MBR, resolves command or logic-offset addressing, and builds the custom flash request.
- `tui`: Interactive terminal frontend. It loads firmware for display, starts flash tasks, and consumes flash events.
- `firmware`: IMAGEWTY file parsing and firmware metadata loading.
- `config`: Allwinner config, MBR, boot header, and partition config parsers.
- `flash`: Device flashing orchestration, shared flash request/event types, and FEL/FES protocol steps.
- `process`: Stage ordering and progress state used by CLI progress rendering and flash events.
- `utils`: Error, logging, and terminal output helpers.

## Partition Data Sources

Normal firmware flashing uses `PartitionSource::Firmware`: the partition planner joins the firmware
MBR, `sys_partition` configuration, and IMAGEWTY entries, then probes each image for Android sparse
format. Raw flashing uses `PartitionSource::ExternalFile`: the FES handler validates the external
partition against the custom MBR, skips sparse and UBIFS probing, and streams the file in 10 MiB
windows.

Logic-offset raw partitions deliberately begin near the end of the 32-bit FES sector space. Their
download addresses use explicit wrapping arithmetic when crossing `u32::MAX`; firmware-backed and
command-mode downloads retain strict overflow checks.

## Event Boundary

Lower-level flash code reports through `Logger`, which emits `FlashEvent` values for stages, logs, partition starts, and progress snapshots. The CLI logger renders those events as terminal output and progress bars. The TUI logger sends them over its app channel and updates UI state without relying on a global log channel.
