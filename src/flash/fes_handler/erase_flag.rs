use crate::flash::FlashMode;
use crate::utils::{FlashError, FlashResult, Logger};
use libefex::FesDataType;

pub struct EraseFlag<'a> {
    logger: &'a Logger,
}

impl<'a> EraseFlag<'a> {
    pub fn new(logger: &'a Logger) -> Self {
        Self { logger }
    }

    pub async fn execute(&self, ctx: &libefex::Context, mode: FlashMode) -> FlashResult<()> {
        self.logger.info("Downloading erase flag...");

        let mut erase_data = vec![0u8; 16];
        let erase_flag = mode.erase_flag();
        erase_data[0..4].copy_from_slice(&erase_flag.to_le_bytes());

        ctx.fes_down(&erase_data, 0, FesDataType::Erase)
            .map_err(|e| FlashError::UsbTransferError(e.to_string()))?;

        self.logger.stage_complete("Erase flag downloaded");
        Ok(())
    }
}
