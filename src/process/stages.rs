use super::global_progress::StageType;

pub struct FlashStages {
    stages: Vec<StageType>,
}

impl FlashStages {
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

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

    pub fn stages(&self) -> &[StageType] {
        &self.stages
    }
}

impl Default for FlashStages {
    fn default() -> Self {
        Self::new()
    }
}
