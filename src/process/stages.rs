//! Flash stages definition
//!
//! Provides stage definitions for FEL and FES mode flash operations

use super::global_progress::StageType;

/// Flash stages container
///
/// Defines the sequence of stages for different flash modes
pub struct FlashStages {
    stages: Vec<StageType>,
}

impl FlashStages {
    /// Create a new empty flash stages container
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    /// Create stages for FEL mode (USB boot)
    ///
    /// FEL mode requires additional stages for DRAM init and U-Boot download
    pub fn for_fel_mode() -> Self {
        let mut instance = Self::new();
        instance.stages = vec![
            StageType::Init,
            StageType::FelDram,
            StageType::FelUboot,
            StageType::FelReconnect,
            StageType::FesQuery,
            StageType::FesErase,
            StageType::FesMbr,
            StageType::FesPartitions,
            StageType::FesBoot,
            StageType::FesMode,
        ];
        instance
    }

    /// Create stages for FES mode (U-Boot)
    ///
    /// FES mode skips FEL-specific stages
    pub fn for_fes_mode() -> Self {
        let mut instance = Self::new();
        instance.stages = vec![
            StageType::Init,
            StageType::FesQuery,
            StageType::FesErase,
            StageType::FesMbr,
            StageType::FesPartitions,
            StageType::FesBoot,
            StageType::FesMode,
        ];
        instance
    }

    /// Get the stages as a slice
    pub fn stages(&self) -> &[StageType] {
        &self.stages
    }
}

impl Default for FlashStages {
    fn default() -> Self {
        Self::new()
    }
}
