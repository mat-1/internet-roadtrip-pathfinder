use tracing::info;

mod v0_to_v1;
mod v1_to_v2;
mod v2_to_v3;
mod v3_to_v4;
mod v4_to_v5;
mod v5_to_v6;

pub const CURRENT_VERSION: u32 = 6;

pub fn try_migrate_from_version(old_version: u32) {
    if old_version > CURRENT_VERSION {
        panic!(
            "Database version {old_version} is greater than the version in the code ({CURRENT_VERSION})."
        )
    }

    info!("Migrating database from version {old_version} to {CURRENT_VERSION}.");

    if old_version < 1 {
        v0_to_v1::migrate().unwrap();
    }
    if old_version < 2 {
        v1_to_v2::migrate().unwrap();
    }
    if old_version < 3 {
        v2_to_v3::migrate().unwrap();
    }
    if old_version < 4 {
        v3_to_v4::migrate().unwrap();
    }
    if old_version < 5 {
        v4_to_v5::migrate().unwrap();
    }
    if old_version < 6 {
        v5_to_v6::migrate().unwrap();
    }
}
