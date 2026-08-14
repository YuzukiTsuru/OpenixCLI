use libefex::{FesDataType, FesToolMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyResponse {
    pub flag: u32,
    pub media_crc: i32,
}

fn verify_response(response: libefex::FesVerifyResp) -> VerifyResponse {
    VerifyResponse {
        flag: response.flag,
        media_crc: response.media_crc,
    }
}

pub trait FelOps {
    fn fel_exec(&self, addr: u32) -> Result<(), String>;
    fn fel_read(&self, addr: u32, buf: &mut [u8]) -> Result<(), String>;
    fn fel_write(&self, addr: u32, buf: &[u8]) -> Result<(), String>;
}

pub trait FesOps {
    fn fes_query_storage(&self) -> Result<u32, String>;
    fn fes_query_secure(&self) -> Result<u32, String>;
    fn fes_probe_flash_size(&self) -> Result<u32, String>;
    fn fes_flash_set_onoff(&self, storage_type: u32, on: bool) -> Result<(), String>;
    fn fes_down(&self, buf: &[u8], addr: u32, data_type: FesDataType) -> Result<(), String>;
    fn fes_down_with_progress<F>(
        &self,
        buf: &[u8],
        addr: u32,
        data_type: FesDataType,
        progress: F,
    ) -> Result<u64, String>
    where
        F: FnMut(u64, u64);
    fn fes_verify_value(&self, addr: u32, size: u64) -> Result<VerifyResponse, String>;
    fn fes_verify_status(&self, tag: u32) -> Result<VerifyResponse, String>;
    fn fes_tool_mode(&self, tool_mode: FesToolMode, next_mode: FesToolMode) -> Result<(), String>;
}

impl FelOps for libefex::Context {
    fn fel_exec(&self, addr: u32) -> Result<(), String> {
        libefex::Context::fel_exec(self, addr).map_err(|error| error.to_string())
    }

    fn fel_read(&self, addr: u32, buf: &mut [u8]) -> Result<(), String> {
        libefex::Context::fel_read(self, addr, buf).map_err(|error| error.to_string())
    }

    fn fel_write(&self, addr: u32, buf: &[u8]) -> Result<(), String> {
        libefex::Context::fel_write(self, addr, buf).map_err(|error| error.to_string())
    }
}

impl FesOps for libefex::Context {
    fn fes_query_storage(&self) -> Result<u32, String> {
        libefex::Context::fes_query_storage(self).map_err(|error| error.to_string())
    }

    fn fes_query_secure(&self) -> Result<u32, String> {
        libefex::Context::fes_query_secure(self).map_err(|error| error.to_string())
    }

    fn fes_probe_flash_size(&self) -> Result<u32, String> {
        libefex::Context::fes_probe_flash_size(self).map_err(|error| error.to_string())
    }

    fn fes_flash_set_onoff(&self, storage_type: u32, on: bool) -> Result<(), String> {
        libefex::Context::fes_flash_set_onoff(self, storage_type, on)
            .map_err(|error| error.to_string())
    }

    fn fes_down(&self, buf: &[u8], addr: u32, data_type: FesDataType) -> Result<(), String> {
        libefex::Context::fes_down(self, buf, addr, data_type).map_err(|error| error.to_string())
    }

    fn fes_down_with_progress<F>(
        &self,
        buf: &[u8],
        addr: u32,
        data_type: FesDataType,
        progress: F,
    ) -> Result<u64, String>
    where
        F: FnMut(u64, u64),
    {
        libefex::Context::fes_down_with_progress(self, buf, addr, data_type, progress)
            .map_err(|error| error.to_string())
    }

    fn fes_verify_value(&self, addr: u32, size: u64) -> Result<VerifyResponse, String> {
        libefex::Context::fes_verify_value(self, addr, size)
            .map(verify_response)
            .map_err(|error| error.to_string())
    }

    fn fes_verify_status(&self, tag: u32) -> Result<VerifyResponse, String> {
        libefex::Context::fes_verify_status(self, tag)
            .map(verify_response)
            .map_err(|error| error.to_string())
    }

    fn fes_tool_mode(&self, tool_mode: FesToolMode, next_mode: FesToolMode) -> Result<(), String> {
        libefex::Context::fes_tool_mode(self, tool_mode, next_mode)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct FelWrite {
        pub(crate) addr: u32,
        pub(crate) data: Vec<u8>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct FesDownload {
        pub(crate) addr: u32,
        pub(crate) data_type: FesDataType,
        pub(crate) data: Vec<u8>,
    }

    pub(crate) struct MockProtocol {
        pub(crate) fel_writes: RefCell<Vec<FelWrite>>,
        pub(crate) fel_execs: RefCell<Vec<u32>>,
        pub(crate) fel_reads: RefCell<VecDeque<Result<Vec<u8>, String>>>,
        pub(crate) downloads: RefCell<Vec<FesDownload>>,
        pub(crate) verify_status_calls: RefCell<Vec<u32>>,
        pub(crate) verify_statuses: RefCell<VecDeque<Result<VerifyResponse, String>>>,
        pub(crate) verify_value_calls: RefCell<Vec<(u32, u64)>>,
        pub(crate) verify_values: RefCell<VecDeque<Result<VerifyResponse, String>>>,
        pub(crate) flash_switches: RefCell<Vec<(u32, bool)>>,
        pub(crate) tool_modes: RefCell<Vec<(FesToolMode, FesToolMode)>>,
        pub(crate) fail_fel_write: RefCell<Option<String>>,
        pub(crate) fail_fel_exec: RefCell<Option<String>>,
        pub(crate) fail_down: RefCell<Option<String>>,
        pub(crate) fail_flash_switch: RefCell<Option<String>>,
        pub(crate) flash_switch_results: RefCell<VecDeque<Result<(), String>>>,
        pub(crate) fail_tool_mode: RefCell<Option<String>>,
        pub(crate) secure: RefCell<Result<u32, String>>,
        pub(crate) storage: RefCell<Result<u32, String>>,
        pub(crate) flash_size: RefCell<Result<u32, String>>,
        pub(crate) progress_written_override: Cell<Option<u64>>,
    }

    impl Default for MockProtocol {
        fn default() -> Self {
            Self {
                fel_writes: RefCell::new(Vec::new()),
                fel_execs: RefCell::new(Vec::new()),
                fel_reads: RefCell::new(VecDeque::new()),
                downloads: RefCell::new(Vec::new()),
                verify_status_calls: RefCell::new(Vec::new()),
                verify_statuses: RefCell::new(VecDeque::new()),
                verify_value_calls: RefCell::new(Vec::new()),
                verify_values: RefCell::new(VecDeque::new()),
                flash_switches: RefCell::new(Vec::new()),
                tool_modes: RefCell::new(Vec::new()),
                fail_fel_write: RefCell::new(None),
                fail_fel_exec: RefCell::new(None),
                fail_down: RefCell::new(None),
                fail_flash_switch: RefCell::new(None),
                flash_switch_results: RefCell::new(VecDeque::new()),
                fail_tool_mode: RefCell::new(None),
                secure: RefCell::new(Ok(0)),
                storage: RefCell::new(Ok(8)),
                flash_size: RefCell::new(Ok(1024)),
                progress_written_override: Cell::new(None),
            }
        }
    }

    impl MockProtocol {
        pub(crate) fn valid_response(media_crc: i32) -> VerifyResponse {
            VerifyResponse {
                flag: crate::config::mbr_parser::EFEX_CRC32_VALID_FLAG,
                media_crc,
            }
        }
    }

    impl FelOps for MockProtocol {
        fn fel_exec(&self, addr: u32) -> Result<(), String> {
            if let Some(error) = self.fail_fel_exec.borrow_mut().take() {
                return Err(error);
            }
            self.fel_execs.borrow_mut().push(addr);
            Ok(())
        }

        fn fel_read(&self, _addr: u32, buf: &mut [u8]) -> Result<(), String> {
            match self
                .fel_reads
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(vec![0; buf.len()]))
            {
                Ok(data) if data.len() == buf.len() => {
                    buf.copy_from_slice(&data);
                    Ok(())
                }
                Ok(_) => Err("mock read length mismatch".to_string()),
                Err(error) => Err(error),
            }
        }

        fn fel_write(&self, addr: u32, buf: &[u8]) -> Result<(), String> {
            if let Some(error) = self.fail_fel_write.borrow_mut().take() {
                return Err(error);
            }
            self.fel_writes.borrow_mut().push(FelWrite {
                addr,
                data: buf.to_vec(),
            });
            Ok(())
        }
    }

    impl FesOps for MockProtocol {
        fn fes_query_storage(&self) -> Result<u32, String> {
            self.storage.borrow().clone()
        }

        fn fes_query_secure(&self) -> Result<u32, String> {
            self.secure.borrow().clone()
        }

        fn fes_probe_flash_size(&self) -> Result<u32, String> {
            self.flash_size.borrow().clone()
        }

        fn fes_flash_set_onoff(&self, storage_type: u32, on: bool) -> Result<(), String> {
            self.flash_switches.borrow_mut().push((storage_type, on));
            if let Some(result) = self.flash_switch_results.borrow_mut().pop_front() {
                return result;
            }
            if let Some(error) = self.fail_flash_switch.borrow_mut().take() {
                return Err(error);
            }
            Ok(())
        }

        fn fes_down(&self, buf: &[u8], addr: u32, data_type: FesDataType) -> Result<(), String> {
            if let Some(error) = self.fail_down.borrow_mut().take() {
                return Err(error);
            }
            self.downloads.borrow_mut().push(FesDownload {
                addr,
                data_type,
                data: buf.to_vec(),
            });
            Ok(())
        }

        fn fes_down_with_progress<F>(
            &self,
            buf: &[u8],
            addr: u32,
            data_type: FesDataType,
            mut progress: F,
        ) -> Result<u64, String>
        where
            F: FnMut(u64, u64),
        {
            self.fes_down(buf, addr, data_type)?;
            let written = self
                .progress_written_override
                .get()
                .unwrap_or(buf.len() as u64);
            progress(written, buf.len() as u64);
            Ok(written)
        }

        fn fes_verify_value(&self, addr: u32, size: u64) -> Result<VerifyResponse, String> {
            self.verify_value_calls.borrow_mut().push((addr, size));
            self.verify_values
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(Self::valid_response(0)))
        }

        fn fes_verify_status(&self, tag: u32) -> Result<VerifyResponse, String> {
            self.verify_status_calls.borrow_mut().push(tag);
            self.verify_statuses
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(Self::valid_response(0)))
        }

        fn fes_tool_mode(
            &self,
            tool_mode: FesToolMode,
            next_mode: FesToolMode,
        ) -> Result<(), String> {
            if let Some(error) = self.fail_tool_mode.borrow_mut().take() {
                return Err(error);
            }
            self.tool_modes.borrow_mut().push((tool_mode, next_mode));
            Ok(())
        }
    }

    impl<T: FelOps + ?Sized> FelOps for Rc<T> {
        fn fel_exec(&self, addr: u32) -> Result<(), String> {
            (**self).fel_exec(addr)
        }

        fn fel_read(&self, addr: u32, buf: &mut [u8]) -> Result<(), String> {
            (**self).fel_read(addr, buf)
        }

        fn fel_write(&self, addr: u32, buf: &[u8]) -> Result<(), String> {
            (**self).fel_write(addr, buf)
        }
    }

    impl<T: FesOps + ?Sized> FesOps for Rc<T> {
        fn fes_query_storage(&self) -> Result<u32, String> {
            (**self).fes_query_storage()
        }

        fn fes_query_secure(&self) -> Result<u32, String> {
            (**self).fes_query_secure()
        }

        fn fes_probe_flash_size(&self) -> Result<u32, String> {
            (**self).fes_probe_flash_size()
        }

        fn fes_flash_set_onoff(&self, storage_type: u32, on: bool) -> Result<(), String> {
            (**self).fes_flash_set_onoff(storage_type, on)
        }

        fn fes_down(&self, buf: &[u8], addr: u32, data_type: FesDataType) -> Result<(), String> {
            (**self).fes_down(buf, addr, data_type)
        }

        fn fes_down_with_progress<F>(
            &self,
            buf: &[u8],
            addr: u32,
            data_type: FesDataType,
            progress: F,
        ) -> Result<u64, String>
        where
            F: FnMut(u64, u64),
        {
            (**self).fes_down_with_progress(buf, addr, data_type, progress)
        }

        fn fes_verify_value(&self, addr: u32, size: u64) -> Result<VerifyResponse, String> {
            (**self).fes_verify_value(addr, size)
        }

        fn fes_verify_status(&self, tag: u32) -> Result<VerifyResponse, String> {
            (**self).fes_verify_status(tag)
        }

        fn fes_tool_mode(
            &self,
            tool_mode: FesToolMode,
            next_mode: FesToolMode,
        ) -> Result<(), String> {
            (**self).fes_tool_mode(tool_mode, next_mode)
        }
    }

    #[test]
    fn libefex_verify_response_conversion_preserves_protocol_fields() {
        let converted = verify_response(libefex::FesVerifyResp {
            flag: 0x1234,
            fes_crc: -1,
            media_crc: 0x5678,
        });
        assert_eq!(converted.flag, 0x1234);
        assert_eq!(converted.media_crc, 0x5678);
    }

    #[test]
    fn uninitialized_libefex_context_adapters_fail_before_usb_access() {
        let context = libefex::Context::new();
        let mut read_buf = [0; 1];

        assert!(<libefex::Context as FelOps>::fel_exec(&context, 0).is_err());
        assert!(<libefex::Context as FelOps>::fel_read(&context, 0, &mut read_buf).is_err());
        assert!(<libefex::Context as FelOps>::fel_write(&context, 0, &[1]).is_err());

        assert!(<libefex::Context as FesOps>::fes_query_storage(&context).is_err());
        assert!(<libefex::Context as FesOps>::fes_query_secure(&context).is_err());
        assert!(<libefex::Context as FesOps>::fes_probe_flash_size(&context).is_err());
        assert!(<libefex::Context as FesOps>::fes_flash_set_onoff(&context, 8, true).is_err());
        assert!(
            <libefex::Context as FesOps>::fes_down(&context, &[1], 0, FesDataType::Preboot)
                .is_err()
        );

        let mut progress_called = false;
        assert!(<libefex::Context as FesOps>::fes_down_with_progress(
            &context,
            &[1],
            0,
            FesDataType::Flash,
            |_, _| progress_called = true,
        )
        .is_err());
        assert!(!progress_called);

        assert!(<libefex::Context as FesOps>::fes_verify_value(&context, 0, 1).is_err());
        assert!(<libefex::Context as FesOps>::fes_verify_status(&context, 0x7f08).is_err());
        assert!(<libefex::Context as FesOps>::fes_tool_mode(
            &context,
            FesToolMode::Reboot,
            FesToolMode::Reboot,
        )
        .is_err());
    }
}
