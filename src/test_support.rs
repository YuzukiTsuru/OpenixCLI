use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::config::mbr_parser::{
    SunxiMbrRaw, SunxiPartitionRaw, MBR_MAGIC, MBR_MAX_PART_CNT, MBR_SIZE, MBR_VERSION,
    PART_NAME_MAX_LEN, PART_SIZE_RES_LEN,
};
use crate::firmware::types::{
    FileHeader, FileHeaderV1, FileHeaderVersionData, ImageHeader, ImageHeaderV1,
    ImageHeaderVersionData, IMAGEWTY_FHDR_FILENAME_LEN, IMAGEWTY_FHDR_MAINTYPE_LEN,
    IMAGEWTY_FHDR_SUBTYPE_LEN, IMAGEWTY_FILEHDR_LEN, IMAGEWTY_MAGIC_LEN,
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) static GLOBAL_STATE_TEST_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct TestFile {
    path: PathBuf,
}

pub(crate) struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let temp_root = std::env::temp_dir();
        if self.path.starts_with(&temp_root) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

impl TestFile {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FirmwareEntry<'a> {
    pub(crate) filename: &'a str,
    pub(crate) maintype: &'a str,
    pub(crate) subtype: &'a str,
    pub(crate) data: &'a [u8],
}

pub(crate) fn temp_file(label: &str, data: &[u8]) -> TestFile {
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("openixcli-{label}-{}-{id}.bin", std::process::id()));
    fs::write(&path, data).expect("write test file");
    TestFile { path }
}

pub(crate) fn temp_dir(label: &str) -> TestDir {
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("openixcli-{label}-{}-{id}", std::process::id()));
    fs::create_dir(&path).expect("create test directory");
    TestDir { path }
}

pub(crate) fn test_firmware(entries: &[FirmwareEntry<'_>]) -> TestFile {
    let data_offset = (entries.len() + 1) * IMAGEWTY_FILEHDR_LEN;
    let image_size = data_offset + entries.iter().map(|entry| entry.data.len()).sum::<usize>();

    let image_header = ImageHeader {
        magic: fixed_bytes::<IMAGEWTY_MAGIC_LEN>(b"IMAGEWTY"),
        header_version: 0x0100,
        header_size: IMAGEWTY_FILEHDR_LEN as u32,
        attr: 0,
        version: 1,
        data: ImageHeaderVersionData {
            v1: ImageHeaderV1 {
                image_size: image_size as u32,
                align: IMAGEWTY_FILEHDR_LEN as u32,
                pid: 0x1234,
                vid: 0x5678,
                hardware_id: 0x9abc,
                firmware_id: 0xdef0,
                file_attr: 0,
                file_size: image_size as u32,
                file_count: entries.len() as u32,
                file_offset: IMAGEWTY_FILEHDR_LEN as u32,
                attr: 0,
                ext_size: 0,
                ext_offset: 0,
                reverse: [0; 4],
            },
        },
    };

    let mut bytes = vec![0u8; image_size];
    write_struct(&mut bytes[..IMAGEWTY_FILEHDR_LEN], &image_header);

    let mut offset = data_offset;
    for (index, entry) in entries.iter().enumerate() {
        let file_header = FileHeader {
            filename_len: entry.filename.len() as u32,
            total_header_size: IMAGEWTY_FILEHDR_LEN as u32,
            maintype: fixed_bytes::<IMAGEWTY_FHDR_MAINTYPE_LEN>(entry.maintype.as_bytes()),
            subtype: fixed_bytes::<IMAGEWTY_FHDR_SUBTYPE_LEN>(entry.subtype.as_bytes()),
            data: FileHeaderVersionData {
                v1: FileHeaderV1 {
                    attr: 0,
                    stored_length: entry.data.len() as u32,
                    original_length: entry.data.len() as u32,
                    offset: offset as u32,
                    checksum: 0,
                    filename: fixed_bytes::<IMAGEWTY_FHDR_FILENAME_LEN>(entry.filename.as_bytes()),
                },
            },
        };
        let header_start = (index + 1) * IMAGEWTY_FILEHDR_LEN;
        write_struct(
            &mut bytes[header_start..header_start + IMAGEWTY_FILEHDR_LEN],
            &file_header,
        );
        bytes[offset..offset + entry.data.len()].copy_from_slice(entry.data);
        offset += entry.data.len();
    }

    temp_file("firmware", &bytes)
}

pub(crate) fn mbr_bytes(partitions: &[(&str, u64, u64, bool)]) -> Vec<u8> {
    assert!(partitions.len() <= MBR_MAX_PART_CNT);
    let empty = SunxiPartitionRaw {
        addrhi: 0,
        addrlo: 0,
        lenhi: 0,
        lenlo: 0,
        classname: [0; PART_NAME_MAX_LEN],
        name: [0; PART_NAME_MAX_LEN],
        user_type: 0,
        keydata: 0,
        ro: 0,
        reserved: [0; PART_SIZE_RES_LEN],
    };
    let mut raw_partitions = [empty; MBR_MAX_PART_CNT];
    for (raw, (name, address, length, readonly)) in
        raw_partitions.iter_mut().zip(partitions.iter().copied())
    {
        raw.addrhi = (address >> 32) as u32;
        raw.addrlo = address as u32;
        raw.lenhi = (length >> 32) as u32;
        raw.lenlo = length as u32;
        raw.classname = fixed_bytes::<PART_NAME_MAX_LEN>(b"DISK");
        raw.name = fixed_bytes::<PART_NAME_MAX_LEN>(name.as_bytes());
        raw.ro = u32::from(readonly);
    }
    let raw = SunxiMbrRaw {
        crc32: 0,
        version: MBR_VERSION,
        magic: fixed_bytes::<8>(MBR_MAGIC.as_bytes()),
        copy: 1,
        index: 0,
        part_count: partitions.len() as u32,
        stamp: 0,
        partitions: raw_partitions,
    };
    let mut bytes = vec![0u8; MBR_SIZE];
    write_struct(&mut bytes, &raw);
    bytes
}

pub(crate) fn fixed_bytes<const N: usize>(value: &[u8]) -> [u8; N] {
    let mut result = [0; N];
    let len = value.len().min(N);
    result[..len].copy_from_slice(&value[..len]);
    result
}

pub(crate) fn write_struct<T: Copy>(target: &mut [u8], value: &T) {
    assert!(target.len() >= std::mem::size_of::<T>());
    unsafe {
        std::ptr::copy_nonoverlapping(
            value as *const T as *const u8,
            target.as_mut_ptr(),
            std::mem::size_of::<T>(),
        );
    }
}
