//! Reset the cache.

use std::fs;

use byteorder::LE;
use heed::{
    Database, Env, EnvOpenOptions,
    types::{Bytes, Str, U32},
};

use crate::model::SizedTile;

const OLD_VERSION: u32 = 1;
const NEW_VERSION: u32 = 2;

#[allow(unused)]
pub struct OldDb {
    env: Env,
    getmetadata_db: Database<U32<LE>, Bytes>,
    listentityphotos_db: Database<SizedTile, Bytes>,
    /// Mapping of Streetview pano IDs into our internal u32 representation.
    pano_ids_db: Database<Str, U32<LE>>,
    settings_db: Database<Str, Bytes>,
}
#[allow(unused)]
pub struct NewDb {
    env: Env,
    getmetadata_db: Database<U32<LE>, Bytes>,
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

    // new database is incompatible with the old one, so don't migrate getmetadata
    // and listentityphotos

    // migrate settings, except for next-pano-id
    for entry in old_db.settings_db.iter(&old_txn)? {
        let (key, old_data) = entry?;
        if key == "next-pano-id" {
            continue;
        }
        new_db.settings_db.put(&mut new_txn, key, old_data).unwrap();
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
