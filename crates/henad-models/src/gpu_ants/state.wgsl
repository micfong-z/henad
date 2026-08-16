#define_import_path gpu_ants::state

// `state` packs the three per-ant scalars the CPU keeps in separate lanes. `reward` is a bit
// rather than an f32 because the CPU model only ever stores 0.0 or the reward param in it.
const LAST_STEP_MASK: u32 = 0xFFu;
const HAS_FOOD_BIT: u32 = 0x100u;
const HAS_REWARD_BIT: u32 = 0x200u;
