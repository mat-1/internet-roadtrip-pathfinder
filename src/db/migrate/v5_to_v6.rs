//! Database compaction after the little-endian -> big-endian migration.

use std::fs;

use byteorder::{BE, LE};
use heed::{
    Database, Env, EnvOpenOptions,
    types::{Bytes, Str, U32},
};
use tracing::info;

use crate::model::SizedTile;

const OLD_VERSION: u32 = 5;
const NEW_VERSION: u32 = 6;

struct OldDb {
    env: Env,
    getmetadata_db: Database<U32<BE>, Bytes>,
    listentityphotos_db: Database<SizedTile, Bytes>,
    /// Mapping of Streetview pano IDs into our internal u32 representation.
    pano_ids_db: Database<Str, U32<LE>>,
    settings_db: Database<Str, Bytes>,
}
pub struct NewDb {
    env: Env,
    getmetadata_db: Database<U32<BE>, Bytes>,
    listentityphotos_db: Database<SizedTile, Bytes>,
    /// Mapping of Streetview pano IDs into our internal u32 representation.
    pano_ids_db: Database<Str, U32<LE>>,
    settings_db: Database<Str, Bytes>,
}

pub fn migrate() -> eyre::Result<()> {
    fs::rename("cache", format!("cache-v{OLD_VERSION}")).unwrap();
    fs::create_dir("cache").unwrap();

    let old_db = OldDb::new()?;
    let new_db = NewDb::new()?;

    let old_txn = old_db.env.read_txn()?;
    let mut new_txn = new_db.env.write_txn()?;

    info!("Migrating getmetadata_db");
    // migrate getmetadata
    for entry in old_db.getmetadata_db.iter(&old_txn)? {
        let (key, data) = entry?;
        new_db.getmetadata_db.put(&mut new_txn, &key, data).unwrap();
    }

    // migrate listentityphotos
    for entry in old_db.listentityphotos_db.iter(&old_txn)? {
        let (key, data) = entry?;
        new_db
            .listentityphotos_db
            .put(&mut new_txn, &key, &data)
            .unwrap();
    }

    // migrate pano ids
    for entry in old_db.pano_ids_db.iter(&old_txn)? {
        let (key, data) = entry?;
        new_db.pano_ids_db.put(&mut new_txn, key, &data).unwrap();
    }
    // migrate settings
    for entry in old_db.settings_db.iter(&old_txn)? {
        let (key, data) = entry?;
        new_db.settings_db.put(&mut new_txn, key, data).unwrap();
    }
    new_db
        .settings_db
        .put(
            &mut new_txn,
            "version",
            NEW_VERSION.to_le_bytes().as_slice(),
        )
        .unwrap();

    old_txn.commit()?;
    new_txn.commit()?;

    old_db.env.prepare_for_closing().wait();
    new_db.env.prepare_for_closing().wait();

    Ok(())
}

impl OldDb {
    fn new() -> eyre::Result<OldDb> {
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(4)
                .map_size(1024 * 1024 * 1024 * 128)
                .open(format!("./cache-v{OLD_VERSION}"))?
        };
        let mut wtxn = env.write_txn()?;
        let settings_db = env.create_database(&mut wtxn, Some("settings"))?;
        let getmetadata_db = env.create_database(&mut wtxn, Some("getmetadata"))?;
        let listentityphotos_db = env.create_database(&mut wtxn, Some("listentityphotos"))?;
        let pano_ids_db = env.create_database(&mut wtxn, Some("panoids"))?;
        wtxn.commit()?;

        Ok(OldDb {
            env,
            getmetadata_db,
            listentityphotos_db,
            pano_ids_db,
            settings_db,
        })
    }
}
impl NewDb {
    fn new() -> eyre::Result<NewDb> {
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(4)
                .map_size(1024 * 1024 * 1024 * 128)
                .open("./cache")?
        };
        let mut wtxn = env.write_txn()?;

        let settings_db = env.create_database(&mut wtxn, Some("settings"))?;
        let getmetadata_db = env.create_database(&mut wtxn, Some("getmetadata"))?;
        let listentityphotos_db = env.create_database(&mut wtxn, Some("listentityphotos"))?;
        let pano_ids_db = env.create_database(&mut wtxn, Some("panoids"))?;

        wtxn.commit()?;

        Ok(NewDb {
            env,
            getmetadata_db,
            listentityphotos_db,
            pano_ids_db,
            settings_db,
        })
    }
}
