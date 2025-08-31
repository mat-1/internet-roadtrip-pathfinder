//! listentitymetadata responses are now stored with both types of coordinates
//! to avoid the need to have to look up 'actual' coordinates separately.

use std::{fs, io::Cursor, sync::Arc};

use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use heed::{
    Database, Env, EnvOpenOptions,
    types::{Bytes, Str, U32},
};
use tracing::info;

use crate::model::{PanoId, SizedTile};

const OLD_VERSION: u32 = 3;
const NEW_VERSION: u32 = 4;

struct OldDb {
    env: Env,
    getmetadata_db: Database<U32<LE>, Bytes>,
    listentityphotos_db: Database<SizedTile, Bytes>,
    /// Mapping of Streetview pano IDs into our internal u32 representation.
    pano_ids_db: Database<Str, U32<LE>>,
    settings_db: Database<Str, Bytes>,
}
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

    info!("Migrating getmetadata_db");
    // migrate getmetadata
    for entry in old_db.getmetadata_db.iter(&old_txn)? {
        let (key, data) = entry?;
        new_db.getmetadata_db.put(&mut new_txn, &key, data).unwrap();
    }

    // migrate listentityphotos
    for entry in old_db.listentityphotos_db.iter(&old_txn)? {
        let (key, old_data) = entry?;
        info!("listentityphotos {key:?}");

        let old_listentityphotos = decode_old_listentityphotos(&mut Cursor::new(old_data));
        let new_listentityphotos = if let Some(old_listentityphotos) = old_listentityphotos {
            let mut new_listentityphotos = Vec::new();
            for old_pano in old_listentityphotos.iter() {
                let actual_loc = old_db
                    .getmetadata_db
                    .get(&old_txn, &old_pano.id.0)
                    .unwrap()
                    .map(|data| {
                        let mut cur = Cursor::new(data);
                        read_location(&mut cur)
                    })
                    .unwrap_or(old_pano.loc);

                new_listentityphotos.push(PanoWithBothLocations {
                    id: old_pano.id,
                    search_loc: old_pano.loc,
                    actual_loc,
                });
            }
            Some(new_listentityphotos.into())
        } else {
            None
        };

        let new_data = encode_new_listentityphotos(new_listentityphotos);
        new_db
            .listentityphotos_db
            .put(&mut new_txn, &key, &new_data)
            .unwrap();
    }

    // migrate pano ids
    for entry in old_db.pano_ids_db.iter(&old_txn)? {
        let (key, old_data) = entry?;
        new_db
            .pano_ids_db
            .put(&mut new_txn, key, &old_data)
            .unwrap();
    }
    // migrate settings
    for entry in old_db.settings_db.iter(&old_txn)? {
        let (key, old_data) = entry?;
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

fn read_location(cur: &mut Cursor<&[u8]>) -> Location {
    let lat = cur.read_i32::<LE>().unwrap();
    let lng = cur.read_i32::<LE>().unwrap();
    Location { lat, lng }
}
fn write_location(buf: &mut Vec<u8>, loc: Location) {
    buf.write_i32::<LE>(loc.lat).unwrap();
    buf.write_i32::<LE>(loc.lng).unwrap();
}

fn write_pano_id(buf: &mut Vec<u8>, pano_id: &PanoId) {
    buf.write_u32::<LE>(pano_id.0).unwrap();
}
fn read_pano_id(cur: &mut Cursor<&[u8]>) -> PanoId {
    PanoId(cur.read_u32::<LE>().unwrap())
}

fn encode_new_listentityphotos(panos: Option<Arc<[PanoWithBothLocations]>>) -> Vec<u8> {
    let mut buf = Vec::new();

    if let Some(panos) = panos {
        // 1 = normal
        buf.write_u8(1).unwrap();
        for pano in panos.iter() {
            write_pano_id(&mut buf, &pano.id);
            write_location(&mut buf, pano.search_loc);
            write_location(&mut buf, pano.actual_loc);
        }
    } else {
        // 0 = too big, smaller pano should be checked
        buf.write_u8(0).unwrap();
    }

    buf
}
fn decode_old_listentityphotos(cur: &mut Cursor<&[u8]>) -> Option<Box<[Pano]>> {
    let mut panos = Vec::new();

    let header = cur.read_u8().unwrap();
    if header == 0 {
        return None;
    }

    while cur.position() < cur.get_ref().len() as u64 {
        let id = read_pano_id(cur);
        let loc = read_location(cur);
        let pano = Pano { id, loc };
        panos.push(pano);
    }

    Some(panos.into())
}

#[derive(Clone, Copy)]
struct Location {
    lat: i32,
    lng: i32,
}
struct Pano {
    id: PanoId,
    loc: Location,
}
struct PanoWithBothLocations {
    id: PanoId,
    search_loc: Location,
    actual_loc: Location,
}
