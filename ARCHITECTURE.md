# OpenixCLI Architecture

## Runtime Flow

OpenixCLI has two frontends: the CLI commands and the interactive TUI. Both frontends now build the same `FlashRequest` and pass it to `Flasher`, so device selection, flash mode, verification, partition filtering, and post-flash action use one shared model.

The flash flow is:

1. Load firmware with `LoadedFirmware`, which wraps `OpenixPacker` plus image metadata and MBR partition names for UI display.
2. Open the selected USB device, or the first detected device when no full bus/port selector is provided.
3. Detect device mode.
4. In FEL mode, initialize DRAM, download U-Boot with configuration blobs, then reconnect in FES mode.
5. In FES mode, query boot/storage details, optionally send the erase flag, plan partition downloads, write MBR, write partitions, and write Boot0/Boot1.
6. Set the requested post-flash device mode.

## Module Responsibilities

- `commands`: CLI command adapters. They parse input, load firmware, build `FlashRequest`, and delegate to `flash`.
- `tui`: Interactive terminal frontend. It loads firmware for display, starts flash tasks, and consumes flash events.
- `firmware`: IMAGEWTY file parsing and firmware metadata loading.
- `config`: Allwinner config, MBR, boot header, and partition config parsers.
- `flash`: Device flashing orchestration, shared flash request/event types, and FEL/FES protocol steps.
- `process`: Stage ordering and progress state used by CLI progress rendering and flash events.
- `utils`: Error, logging, and terminal output helpers.

## Event Boundary

Lower-level flash code reports through `Logger`, which emits `FlashEvent` values for stages, logs, partition starts, and progress snapshots. The CLI logger renders those events as terminal output and progress bars. The TUI logger sends them over its app channel and updates UI state without relying on a global log channel.
